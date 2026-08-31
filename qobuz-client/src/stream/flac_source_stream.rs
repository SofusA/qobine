use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::Stream;
use parking_lot::Mutex;
use stream_download::source::{DecodeError, SourceStream, StreamOutcome};
use tokio::task::JoinHandle;

use crate::stream::{cmaf, crypto};

#[derive(Debug, Clone)]
pub struct SegmentByteInfo {
    pub byte_offset: u64,
    pub byte_len: u64,
}

struct SharedDownloadState {
    url_template: String,
    n_segments: u32,
    content_key: Option<[u8; 16]>,
    flac_header: Vec<u8>,
    cache_path: PathBuf,
    segment_map: Vec<SegmentByteInfo>,
    downloaded: Mutex<Vec<Option<Vec<u8>>>>,
    /// Partial decrypted data from cancelled fetches, persists across task respawns.
    in_progress: Mutex<Vec<Option<Vec<u8>>>>,
    cache_written: AtomicBool,
    gap_fill_running: AtomicBool,
}

pub struct FlacSourceParams {
    pub url_template: String,
    pub n_segments: u32,
    pub content_key: Option<[u8; 16]>,
    pub flac_header: Vec<u8>,
    pub cache_path: PathBuf,
    pub segment_map: Vec<SegmentByteInfo>,
}

pub struct FlacSourceStream {
    rx: tokio::sync::mpsc::Receiver<io::Result<Bytes>>,
    flac_header_len: u64,
    shared: Arc<SharedDownloadState>,
}

#[derive(Debug)]
pub struct FlacStreamError(pub String);

impl std::fmt::Display for FlacStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for FlacStreamError {}
impl DecodeError for FlacStreamError {}

impl SourceStream for FlacSourceStream {
    type Params = FlacSourceParams;
    type StreamCreationError = FlacStreamError;

    #[allow(clippy::unused_async_trait_impl)]
    async fn create(params: Self::Params) -> Result<Self, Self::StreamCreationError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<io::Result<Bytes>>(4);

        let flac_header_len = u64::try_from(params.flac_header.len())
            .map_err(|e| FlacStreamError(format!("FLAC header length does not fit in u64: {e}")))?;

        let total_segs = params
            .n_segments
            .checked_sub(1)
            .ok_or_else(|| FlacStreamError("n_segments must be at least 1".into()))
            .and_then(|count| {
                usize::try_from(count).map_err(|e| {
                    FlacStreamError(format!("segment count does not fit in usize: {e}"))
                })
            })?;

        let shared = Arc::new(SharedDownloadState {
            url_template: params.url_template,
            n_segments: params.n_segments,
            content_key: params.content_key,
            flac_header: params.flac_header,
            cache_path: params.cache_path,
            segment_map: params.segment_map,
            downloaded: Mutex::new(vec![None; total_segs]),
            in_progress: Mutex::new(vec![None; total_segs]),
            cache_written: AtomicBool::new(false),
            gap_fill_running: AtomicBool::new(false),
        });

        let shared_clone = Arc::clone(&shared);
        tokio::spawn(async move {
            run_download_initial(shared_clone, tx).await;
        });

        Ok(Self {
            rx,
            flac_header_len,
            shared,
        })
    }

    // Return None: disables stream-download-rs gap-filling which caused 100% CPU
    // (segment table byte_len estimates don't match actual decrypted sizes).
    // SeekableStreamReader handles SeekFrom::End independently.
    fn content_length(&self) -> Option<u64> {
        None
    }

    fn supports_seek(&self) -> bool {
        true
    }

    #[allow(clippy::unused_async_trait_impl)]
    async fn seek_range(&mut self, start: u64, _end: Option<u64>) -> io::Result<()> {
        let overflow = || io::Error::other("seek arithmetic overflow");

        let data_offset = start.saturating_sub(self.flac_header_len);

        let mut seg_idx = None;
        for (index, segment) in self.shared.segment_map.iter().enumerate() {
            let segment_end = segment
                .byte_offset
                .checked_add(segment.byte_len)
                .ok_or_else(overflow)?;

            if data_offset < segment_end {
                seg_idx = Some(index);
                break;
            }
        }

        let seg_idx = seg_idx
            .or_else(|| self.shared.segment_map.len().checked_sub(1))
            .ok_or_else(|| io::Error::other("segment map is empty"))?;

        let segment = self
            .shared
            .segment_map
            .get(seg_idx)
            .ok_or_else(|| io::Error::other("segment index out of bounds"))?;

        let target_seg = u32::try_from(seg_idx)
            .map_err(|_| overflow())?
            .checked_add(1)
            .ok_or_else(overflow)?;

        let seg_byte_start = self
            .flac_header_len
            .checked_add(segment.byte_offset)
            .ok_or_else(overflow)?;

        let skip_bytes =
            usize::try_from(start.saturating_sub(seg_byte_start)).map_err(|_| overflow())?;

        self.rx.close();
        while self.rx.try_recv().is_ok() {}

        let (tx, rx) = tokio::sync::mpsc::channel(4);
        self.rx = rx;

        let shared = Arc::clone(&self.shared);
        tokio::spawn(async move {
            run_download_from(shared, tx, target_seg, skip_bytes).await;
        });

        tracing::debug!("seek: respawned from segment {target_seg} (skip {skip_bytes} bytes)");
        Ok(())
    }

    async fn reconnect(&mut self, current_position: u64) -> io::Result<()> {
        self.seek_range(current_position, None).await
    }

    fn on_finish(
        &mut self,
        result: io::Result<()>,
        _outcome: StreamOutcome,
    ) -> impl Future<Output = io::Result<()>> {
        std::future::ready(result)
    }
}

impl Stream for FlacSourceStream {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

pub struct SeekableStreamReader {
    inner: Box<dyn ReadSeekSend>,
    content_length: u64,
}

pub trait ReadSeekSend: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync + 'static> ReadSeekSend for T {}

impl SeekableStreamReader {
    pub fn new<R: Read + Seek + Send + Sync + 'static>(inner: R, content_length: u64) -> Self {
        Self {
            inner: Box::new(inner),
            content_length,
        }
    }

    #[must_use]
    pub const fn content_length(&self) -> u64 {
        self.content_length
    }
}

impl Read for SeekableStreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Seek for SeekableStreamReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match pos {
            SeekFrom::End(offset) => {
                let content_length = i64::try_from(self.content_length).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "content length exceeds i64::MAX",
                    )
                })?;

                let target = content_length.checked_add(offset).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "seek offset overflow")
                })?;

                let target = u64::try_from(target).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "cannot seek before the start of the stream",
                    )
                })?;

                self.inner.seek(SeekFrom::Start(target))
            }
            other => self.inner.seek(other),
        }
    }
}

// ---------------------------------------------------------------------------
// Download tasks
// ---------------------------------------------------------------------------

async fn run_download_initial(
    shared: Arc<SharedDownloadState>,
    tx: tokio::sync::mpsc::Sender<io::Result<Bytes>>,
) {
    let header_bytes = Bytes::copy_from_slice(&shared.flac_header);
    if tx.send(Ok(header_bytes)).await.is_err() {
        return;
    }

    let n = shared.n_segments;
    download_segments(&shared, &tx, 1, n, 0).await;
    maybe_spawn_gap_fill(shared, 1);
}

async fn run_download_from(
    shared: Arc<SharedDownloadState>,
    tx: tokio::sync::mpsc::Sender<io::Result<Bytes>>,
    start_seg: u32,
    skip_first_bytes: usize,
) {
    let n = shared.n_segments;
    download_segments(&shared, &tx, start_seg, n, skip_first_bytes).await;
    maybe_spawn_gap_fill(shared, start_seg);
}

/// Spawn gap-fill only if the forward pass completed (all segments from `start_seg` onward
/// are downloaded) and no other gap-fill is already running.
fn maybe_spawn_gap_fill(shared: Arc<SharedDownloadState>, start_seg: u32) {
    let forward_complete = {
        let downloaded = shared.downloaded.lock();

        (1..shared.n_segments)
            .zip(downloaded.iter())
            .filter(|(segment, _)| *segment >= start_seg)
            .all(|(_, state)| state.is_some())
    };

    if !forward_complete {
        return;
    }

    if shared.gap_fill_running.swap(true, Ordering::AcqRel) {
        return;
    }

    tokio::spawn(async move {
        if let Err(err) = fill_missing_segments(&shared).await {
            tracing::error!("Error filling missing segments: {err}");
        }
        shared.try_write_cache();
        shared.gap_fill_running.store(false, Ordering::Release);
    });
}

/// Resolution order per segment: downloaded (complete) → `in_progress` (partial) → network.
/// Prefetches the next segment in parallel for faster buffering.
async fn download_segments(
    shared: &Arc<SharedDownloadState>,
    tx: &tokio::sync::mpsc::Sender<io::Result<Bytes>>,
    from_seg: u32,
    to_seg: u32,
    skip_first_bytes: usize,
) {
    let mut prefetch: Option<JoinHandle<()>> = None;

    for seg in from_seg..to_seg {
        if tx.is_closed() {
            if let Some(handle) = prefetch.take() {
                handle.abort();
            }
            return;
        }

        if let Some(handle) = prefetch.take() {
            let _ = handle.await;
        }

        let Some(idx) = seg
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
        else {
            let _ = tx
                .send(Err(io::Error::other("segment index overflow")))
                .await;
            return;
        };

        let skip = if seg == from_seg { skip_first_bytes } else { 0 };

        let Some(next_seg) = seg.checked_add(1) else {
            let _ = tx
                .send(Err(io::Error::other("segment number overflow")))
                .await;
            return;
        };

        if next_seg < to_seg {
            let Some(next_idx) = next_seg
                .checked_sub(1)
                .and_then(|index| usize::try_from(index).ok())
            else {
                let _ = tx
                    .send(Err(io::Error::other("segment index overflow")))
                    .await;
                return;
            };

            let should_prefetch = shared
                .downloaded
                .lock()
                .get(next_idx)
                .is_some_and(Option::is_none);

            if should_prefetch {
                let shared_clone = Arc::clone(shared);
                prefetch = Some(tokio::spawn(async move {
                    prefetch_segment(&shared_clone, next_seg).await;
                }));
            }
        }

        let complete = shared.downloaded.lock().get(idx).cloned().flatten();

        if let Some(frames) = complete {
            if send_with_skip(tx, &frames, skip, shared.n_segments, seg, "memory").await {
                continue;
            }
            return;
        }

        let partial = shared
            .in_progress
            .lock()
            .get(idx)
            .cloned()
            .flatten()
            .filter(|data| data.len() > skip);

        let mut already_sent = 0;

        if let Some(data) = partial {
            already_sent = if let Some(length) = data.len().checked_sub(skip) {
                length
            } else {
                let _ = tx
                    .send(Err(io::Error::other("partial segment offset overflow")))
                    .await;
                return;
            };

            if !send_with_skip(tx, &data, skip, shared.n_segments, seg, "partial").await {
                return;
            }
        }

        if let Err(error) = fetch_and_stream_segment(shared, seg, skip, already_sent, tx).await {
            if !tx.is_closed() {
                let _ = tx.send(Err(io::Error::other(error))).await;
            }
            return;
        }

        if seg == from_seg {
            tokio::task::yield_now().await;
        }
    }
}

/// Download any segments not yet in `downloaded` (for cache completeness).
/// Runs in background after the main download pass — doesn't send to channel.
async fn fill_missing_segments(shared: &Arc<SharedDownloadState>) -> io::Result<()> {
    let total = shared
        .n_segments
        .checked_sub(1)
        .ok_or_else(|| io::Error::other("segment count underflow"))?;

    let total = usize::try_from(total)
        .map_err(|_| io::Error::other("segment count does not fit in usize"))?;

    let missing = {
        let downloaded = shared.downloaded.lock();
        let mut missing = Vec::new();

        for (index, state) in downloaded.iter().take(total).enumerate() {
            if state.is_none() {
                let segment = u32::try_from(index)
                    .map_err(|_| io::Error::other("segment index does not fit in u32"))?
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("segment number overflow"))?;

                missing.push(segment);
            }
        }

        missing
    };

    if missing.is_empty() {
        return Ok(());
    }

    tracing::info!("Filling {} missing segments for cache", missing.len());

    for segment in missing {
        let shared_clone = Arc::clone(shared);
        prefetch_segment(&shared_clone, segment).await;
    }

    Ok(())
}

/// Prefetch a segment into `downloaded` without sending to the channel.
async fn prefetch_segment(shared: &SharedDownloadState, seg: u32) {
    let Some(idx) = seg
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
    else {
        return;
    };

    if shared
        .downloaded
        .lock()
        .get(idx)
        .is_none_or(Option::is_some)
    {
        return;
    }

    let url = shared.url_template.replace("$SEGMENT$", &seg.to_string());

    let Ok(resp) = reqwest::get(&url).await else {
        return;
    };

    let seg_bytes = match resp.bytes().await {
        Ok(bytes) => bytes.to_vec(),
        Err(_) => return,
    };

    let Ok(crypto) = cmaf::parse_segment_crypto(&seg_bytes) else {
        return;
    };

    let key = shared.content_key.unwrap_or([0; 16]);
    let mut all_decrypted = Vec::new();
    let mut data_pos = crypto.data_offset;

    for entry in &crypto.entries {
        let Ok(frame_size) = usize::try_from(entry.size) else {
            return;
        };

        let Some(frame_end) = data_pos.checked_add(frame_size) else {
            return;
        };

        let Some(frame_data) = seg_bytes.get(data_pos..frame_end) else {
            return;
        };

        let mut frame = frame_data.to_vec();

        if entry.flags != 0 {
            crypto::decrypt_frame(&key, &entry.iv, &mut frame);
        }

        all_decrypted.extend_from_slice(&frame);
        data_pos = frame_end;
    }

    let mdat_end = crypto.mdat_end.min(seg_bytes.len());

    if data_pos < mdat_end {
        let Some(trailing_data) = seg_bytes.get(data_pos..mdat_end) else {
            return;
        };

        all_decrypted.extend_from_slice(trailing_data);
    }

    {
        let mut downloaded = shared.downloaded.lock();
        let Some(slot) = downloaded.get_mut(idx) else {
            return;
        };
        *slot = Some(all_decrypted);
    }

    {
        let mut in_progress = shared.in_progress.lock();
        let Some(slot) = in_progress.get_mut(idx) else {
            return;
        };
        *slot = None;
    }

    let segment_total = shared.n_segments.saturating_sub(1);
    tracing::debug!("Segment {seg}/{segment_total}: prefetched");
}

/// Returns true if send succeeded, false if channel closed.
async fn send_with_skip(
    tx: &tokio::sync::mpsc::Sender<io::Result<Bytes>>,
    frames: &[u8],
    skip: usize,
    n_segments: u32,
    seg: u32,
    source: &str,
) -> bool {
    let data = if skip > 0 {
        frames.get(skip..).unwrap_or(frames)
    } else {
        frames
    };

    if tx.send(Ok(Bytes::copy_from_slice(data))).await.is_err() {
        return false;
    }

    let segment_total = n_segments.saturating_sub(1);

    tracing::debug!(
        "Segment {seg}/{segment_total}: {} bytes (from {source})",
        data.len(),
    );

    true
}

/// Streams a segment from the network, decrypting FLAC frames incrementally.
///
/// `already_sent`: bytes already sent from partial data (not re-sent, but still
/// decrypted).
///
/// Partial progress is stored in `shared.in_progress` to survive task
/// cancellation.
async fn fetch_and_stream_segment<E>(
    shared: &SharedDownloadState,
    seg: u32,
    skip_bytes: usize,
    already_sent: usize,
    tx: &tokio::sync::mpsc::Sender<Result<Bytes, E>>,
) -> Result<(), String>
where
    E: Send,
{
    let url = shared.url_template.replace("$SEGMENT$", &seg.to_string());

    let mut resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to fetch segment {seg}: {e}"))?;

    let mut buf = Vec::new();

    let segment_crypto = loop {
        match resp
            .chunk()
            .await
            .map_err(|e| format!("Segment {seg}: {e}"))?
        {
            Some(chunk) => {
                buf.extend_from_slice(&chunk);

                if let Ok(crypto) = cmaf::parse_segment_crypto(&buf) {
                    break crypto;
                }
            }
            None => return Err(format!("Segment {seg}: truncated before header")),
        }
    };

    let key = shared.content_key.unwrap_or([0_u8; 16]);

    let segment_index = seg
        .checked_sub(1)
        .ok_or_else(|| "Segment index must start at 1".to_owned())?;

    let idx = usize::try_from(segment_index)
        .map_err(|_| format!("Segment {seg}: index does not fit usize"))?;

    let total_skip = skip_bytes
        .checked_add(already_sent)
        .ok_or_else(|| format!("Segment {seg}: skip-byte count overflow"))?;

    let mut all_decrypted = Vec::new();
    let mut data_pos = segment_crypto.data_offset;
    let mut bytes_accumulated = 0_usize;
    let mut entry_idx = 0_usize;
    let mut last_persisted_len = 0_usize;
    let entries = &segment_crypto.entries;

    while entry_idx < entries.len() {
        let mut batch = Vec::new();

        while let Some(entry) = entries.get(entry_idx) {
            let frame_size = usize::try_from(entry.size)
                .map_err(|_| format!("Segment {seg}: frame size does not fit usize"))?;

            let frame_end = data_pos
                .checked_add(frame_size)
                .ok_or_else(|| format!("Segment {seg}: frame position overflow"))?;

            if buf.len() < frame_end {
                break;
            }

            let mut frame = buf
                .get(data_pos..frame_end)
                .ok_or_else(|| format!("Segment {seg}: invalid frame range"))?
                .to_vec();

            if entry.flags != 0 {
                crypto::decrypt_frame(&key, &entry.iv, &mut frame);
            }

            all_decrypted.extend_from_slice(&frame);

            let frame_len = frame.len();
            let next_accumulated = bytes_accumulated
                .checked_add(frame_len)
                .ok_or_else(|| format!("Segment {seg}: byte count overflow"))?;

            if next_accumulated <= total_skip {
                bytes_accumulated = next_accumulated;
            } else if bytes_accumulated < total_skip {
                let offset = total_skip
                    .checked_sub(bytes_accumulated)
                    .ok_or_else(|| format!("Segment {seg}: invalid frame offset"))?;

                let remaining = frame
                    .get(offset..)
                    .ok_or_else(|| format!("Segment {seg}: frame offset out of bounds"))?;

                batch.extend_from_slice(remaining);
                bytes_accumulated = next_accumulated;
            } else {
                batch.extend_from_slice(&frame);
                bytes_accumulated = next_accumulated;
            }

            data_pos = frame_end;
            entry_idx = entry_idx
                .checked_add(1)
                .ok_or_else(|| format!("Segment {seg}: entry index overflow"))?;
        }

        if all_decrypted.len().saturating_sub(last_persisted_len) >= 256 * 1024 {
            let mut progress = shared.in_progress.lock();
            let slot = progress
                .get_mut(idx)
                .ok_or_else(|| format!("Segment {seg}: progress index out of bounds"))?;

            let existing_len = slot.as_ref().map_or(0, Vec::len);

            if all_decrypted.len() > existing_len {
                *slot = Some(all_decrypted.clone());
                last_persisted_len = all_decrypted.len();
            }
        }

        if !batch.is_empty() && tx.send(Ok(Bytes::copy_from_slice(&batch))).await.is_err() {
            let mut progress = shared.in_progress.lock();
            let slot = progress
                .get_mut(idx)
                .ok_or_else(|| format!("Segment {seg}: progress index out of bounds"))?;

            let existing_len = slot.as_ref().map_or(0, Vec::len);

            if all_decrypted.len() > existing_len {
                *slot = Some(all_decrypted);
            }

            return Ok(());
        }

        if entry_idx >= entries.len() {
            break;
        }

        match resp
            .chunk()
            .await
            .map_err(|e| format!("Segment {seg}: {e}"))?
        {
            Some(chunk) => buf.extend_from_slice(&chunk),
            None => return Err(format!("Segment {seg}: truncated at frame")),
        }

        if tx.is_closed() {
            let mut progress = shared.in_progress.lock();
            let slot = progress
                .get_mut(idx)
                .ok_or_else(|| format!("Segment {seg}: progress index out of bounds"))?;

            let existing_len = slot.as_ref().map_or(0, Vec::len);

            if all_decrypted.len() > existing_len {
                *slot = Some(all_decrypted);
            }

            return Ok(());
        }
    }

    // Trailing unencrypted mdat data after the final frame entry.
    let mdat_end = segment_crypto.mdat_end.min(buf.len());

    if data_pos < mdat_end {
        let trailing = buf
            .get(data_pos..mdat_end)
            .ok_or_else(|| format!("Segment {seg}: invalid trailing-data range"))?;

        all_decrypted.extend_from_slice(trailing);

        let next_accumulated = bytes_accumulated
            .checked_add(trailing.len())
            .ok_or_else(|| format!("Segment {seg}: byte count overflow"))?;

        if next_accumulated > total_skip {
            let send_start = total_skip.saturating_sub(bytes_accumulated);

            if let Some(to_send) = trailing.get(send_start..)
                && !to_send.is_empty()
            {
                let _ = tx.send(Ok(Bytes::copy_from_slice(to_send))).await;
            }
        }

        bytes_accumulated = next_accumulated;
    }

    {
        let mut downloaded = shared.downloaded.lock();
        let slot = downloaded
            .get_mut(idx)
            .ok_or_else(|| format!("Segment {seg}: download index out of bounds"))?;

        *slot = Some(all_decrypted);
    }

    {
        let mut progress = shared.in_progress.lock();
        let slot = progress
            .get_mut(idx)
            .ok_or_else(|| format!("Segment {seg}: progress index out of bounds"))?;

        *slot = None;
    }

    let total_sent = bytes_accumulated.saturating_sub(skip_bytes);

    tracing::debug!(
        "Segment {seg}/{}: {total_sent} bytes streamed",
        shared.n_segments.saturating_sub(1),
    );

    Ok(())
}

impl SharedDownloadState {
    fn try_write_cache(&self) {
        if self.cache_written.swap(true, Ordering::AcqRel) {
            return;
        }

        let downloaded = self.downloaded.lock();
        if !downloaded.iter().all(Option::is_some) {
            self.cache_written.store(false, Ordering::Release);
            return;
        }

        let segments_len = self.segment_map.iter().try_fold(0_usize, |total, segment| {
            let byte_len = usize::try_from(segment.byte_len).ok()?;
            total.checked_add(byte_len)
        });

        let Some(capacity) = segments_len.and_then(|len| self.flac_header.len().checked_add(len))
        else {
            self.cache_written.store(false, Ordering::Release);
            tracing::warn!("Cache size exceeds the platform limit");
            return;
        };

        let mut cache_data = Vec::with_capacity(capacity);
        cache_data.extend_from_slice(&self.flac_header);

        for data in downloaded.iter().flatten() {
            cache_data.extend_from_slice(data);
        }

        drop(downloaded);

        if let Some(parent) = self.cache_path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            self.cache_written.store(false, Ordering::Release);
            tracing::warn!("Failed to create cache directory: {error}");
            return;
        }

        let temporary_path = self.cache_path.with_extension("partial");

        if let Err(error) = fs::write(&temporary_path, &cache_data) {
            self.cache_written.store(false, Ordering::Release);
            tracing::warn!("Failed to write cache: {error}");
        } else if let Err(error) = fs::rename(&temporary_path, &self.cache_path) {
            self.cache_written.store(false, Ordering::Release);
            let _ = fs::remove_file(&temporary_path);
            tracing::warn!("Failed to finalize cache: {error}");
        } else {
            tracing::info!(
                "Cached: {} ({} bytes)",
                self.cache_path.display(),
                cache_data.len()
            );
        }
    }
}
