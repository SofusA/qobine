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
    n_segments: u8,
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
    pub n_segments: u8,
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

    async fn create(params: Self::Params) -> Result<Self, Self::StreamCreationError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<io::Result<Bytes>>(4);
        let flac_header_len = params.flac_header.len() as u64;
        let total_segs = (params.n_segments - 1) as usize;

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

        let shared_clone = shared.clone();
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

    async fn seek_range(&mut self, start: u64, _end: Option<u64>) -> io::Result<()> {
        let data_offset = start.saturating_sub(self.flac_header_len);

        let seg_idx = self
            .shared
            .segment_map
            .iter()
            .position(|s| data_offset < s.byte_offset + s.byte_len)
            .unwrap_or(self.shared.segment_map.len().saturating_sub(1));

        let target_seg = seg_idx as u8 + 1;
        let seg_byte_start = self.flac_header_len + self.shared.segment_map[seg_idx].byte_offset;
        let skip_bytes = start.saturating_sub(seg_byte_start) as usize;

        self.rx.close();
        while self.rx.try_recv().is_ok() {}

        let (tx, rx) = tokio::sync::mpsc::channel(4);
        self.rx = rx;

        let shared = self.shared.clone();
        tokio::spawn(async move {
            run_download_from(shared, tx, target_seg, skip_bytes).await;
        });

        tracing::debug!("seek: respawned from segment {target_seg} (skip {skip_bytes} bytes)");
        Ok(())
    }

    async fn reconnect(&mut self, current_position: u64) -> io::Result<()> {
        self.seek_range(current_position, None).await
    }

    async fn on_finish(
        &mut self,
        result: io::Result<()>,
        _outcome: StreamOutcome,
    ) -> io::Result<()> {
        result
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

    pub fn content_length(&self) -> u64 {
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
                let target = (self.content_length as i64 + offset).max(0) as u64;
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
    start_seg: u8,
    skip_first_bytes: usize,
) {
    let n = shared.n_segments;
    download_segments(&shared, &tx, start_seg, n, skip_first_bytes).await;
    maybe_spawn_gap_fill(shared, start_seg);
}

/// Spawn gap-fill only if the forward pass completed (all segments from start_seg onward
/// are downloaded) and no other gap-fill is already running.
fn maybe_spawn_gap_fill(shared: Arc<SharedDownloadState>, start_seg: u8) {
    let n = shared.n_segments;
    let forward_complete = {
        let downloaded = shared.downloaded.lock();
        (start_seg..n).all(|seg| downloaded[(seg - 1) as usize].is_some())
    };
    if !forward_complete {
        return;
    }
    if shared.gap_fill_running.swap(true, Ordering::AcqRel) {
        return; // another gap-fill already in progress
    }
    tokio::spawn(async move {
        fill_missing_segments(&shared).await;
        shared.try_write_cache();
        shared.gap_fill_running.store(false, Ordering::Release);
    });
}

/// Resolution order per segment: downloaded (complete) → in_progress (partial) → network.
/// Prefetches the next segment in parallel for faster buffering.
async fn download_segments(
    shared: &Arc<SharedDownloadState>,
    tx: &tokio::sync::mpsc::Sender<io::Result<Bytes>>,
    from_seg: u8,
    to_seg: u8,
    skip_first_bytes: usize,
) {
    let mut prefetch: Option<JoinHandle<()>> = None;

    for seg in from_seg..to_seg {
        if tx.is_closed() {
            if let Some(h) = prefetch.take() {
                h.abort();
            }
            return;
        }

        if let Some(h) = prefetch.take() {
            let _ = h.await;
        }

        let idx = (seg - 1) as usize;
        let skip = if seg == from_seg { skip_first_bytes } else { 0 };

        // Prefetch next segment in background
        let next_seg = seg + 1;
        if next_seg < to_seg && shared.downloaded.lock()[(next_seg - 1) as usize].is_none() {
            let shared_clone = shared.clone();
            prefetch = Some(tokio::spawn(async move {
                prefetch_segment(&shared_clone, next_seg).await;
            }));
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
        let mut already_sent: usize = 0;
        if let Some(data) = partial {
            already_sent = if skip < data.len() {
                data.len() - skip
            } else {
                0
            };
            if !send_with_skip(tx, &data, skip, shared.n_segments, seg, "partial").await {
                return;
            }
        }

        match fetch_and_stream_segment(shared, seg, skip, already_sent, tx).await {
            Ok(()) => {}
            Err(e) => {
                if tx.is_closed() {
                    return;
                }
                let _ = tx.send(Err(io::Error::other(e))).await;
                return;
            }
        }

        if seg == from_seg {
            tokio::task::yield_now().await;
        }
    }
}

/// Download any segments not yet in `downloaded` (for cache completeness).
/// Runs in background after the main download pass — doesn't send to channel.
async fn fill_missing_segments(shared: &Arc<SharedDownloadState>) {
    let total = (shared.n_segments - 1) as usize;
    let missing: Vec<u8> = (0..total)
        .filter(|i| shared.downloaded.lock()[*i].is_none())
        .map(|i| i as u8 + 1)
        .collect();

    if missing.is_empty() {
        return;
    }

    tracing::info!("Filling {} missing segments for cache", missing.len());
    for seg in missing {
        let shared_clone = shared.clone();
        prefetch_segment(&shared_clone, seg).await;
    }
}

/// Prefetch a segment into `downloaded` without sending to the channel.
async fn prefetch_segment(shared: &SharedDownloadState, seg: u8) {
    let idx = (seg - 1) as usize;
    if shared.downloaded.lock()[idx].is_some() {
        return;
    }

    let url = shared.url_template.replace("$SEGMENT$", &seg.to_string());
    let resp = match reqwest::get(&url).await {
        Ok(r) => r,
        Err(_) => return,
    };
    let seg_bytes = match resp.bytes().await {
        Ok(b) => b.to_vec(),
        Err(_) => return,
    };

    let crypto = match cmaf::parse_segment_crypto(&seg_bytes) {
        Ok(c) => c,
        Err(_) => return,
    };

    let key = shared.content_key.unwrap_or([0u8; 16]);
    let mut all_decrypted = Vec::new();
    let mut data_pos = crypto.data_offset;

    for entry in &crypto.entries {
        let frame_end = data_pos + entry.size as usize;
        if frame_end > seg_bytes.len() {
            return;
        }
        let mut frame = seg_bytes[data_pos..frame_end].to_vec();
        if entry.flags != 0 {
            crypto::decrypt_frame(&key, &entry.iv, &mut frame);
        }
        all_decrypted.extend_from_slice(&frame);
        data_pos = frame_end;
    }

    // Trailing mdat data after last frame entry (unencrypted)
    let mdat_end = crypto.mdat_end.min(seg_bytes.len());
    if data_pos < mdat_end {
        all_decrypted.extend_from_slice(&seg_bytes[data_pos..mdat_end]);
    }

    shared.downloaded.lock()[idx] = Some(all_decrypted);
    shared.in_progress.lock()[idx] = None;
    tracing::debug!("Segment {seg}/{}: prefetched", shared.n_segments - 1);
}

/// Returns true if send succeeded, false if channel closed.
async fn send_with_skip(
    tx: &tokio::sync::mpsc::Sender<io::Result<Bytes>>,
    frames: &[u8],
    skip: usize,
    n_segments: u8,
    seg: u8,
    source: &str,
) -> bool {
    let data = if skip > 0 && skip < frames.len() {
        &frames[skip..]
    } else {
        frames
    };
    if tx.send(Ok(Bytes::copy_from_slice(data))).await.is_err() {
        return false;
    }
    tracing::debug!(
        "Segment {seg}/{}: {} bytes (from {source})",
        n_segments - 1,
        data.len(),
    );
    true
}

/// Streams a segment from the network, decrypting FLAC frames incrementally.
/// `already_sent`: bytes already sent from partial data (not re-sent, but still decrypted).
/// Partial progress is stored in `shared.in_progress` to survive task cancellation.
async fn fetch_and_stream_segment(
    shared: &SharedDownloadState,
    seg: u8,
    skip_bytes: usize,
    already_sent: usize,
    tx: &tokio::sync::mpsc::Sender<io::Result<Bytes>>,
) -> Result<(), String> {
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
                if let Ok(c) = cmaf::parse_segment_crypto(&buf) {
                    break c;
                }
            }
            None => return Err(format!("Segment {seg}: truncated before header")),
        }
    };

    let key = shared.content_key.unwrap_or([0u8; 16]);
    let idx = (seg - 1) as usize;
    let total_skip = skip_bytes + already_sent;

    let mut all_decrypted = Vec::new();
    let mut data_pos = segment_crypto.data_offset;
    let mut bytes_accumulated: usize = 0;
    let mut entry_idx = 0;
    let mut last_persisted_len: usize = 0;
    let entries = &segment_crypto.entries;

    while entry_idx < entries.len() {
        let mut batch = Vec::new();

        while entry_idx < entries.len() {
            let entry = &entries[entry_idx];
            let frame_end = data_pos + entry.size as usize;

            if buf.len() < frame_end {
                break;
            }

            let mut frame = buf[data_pos..frame_end].to_vec();
            if entry.flags != 0 {
                crypto::decrypt_frame(&key, &entry.iv, &mut frame);
            }

            all_decrypted.extend_from_slice(&frame);
            let frame_len = frame.len();

            if bytes_accumulated + frame_len <= total_skip {
                bytes_accumulated += frame_len;
            } else if bytes_accumulated < total_skip {
                let offset = total_skip - bytes_accumulated;
                bytes_accumulated += frame_len;
                batch.extend_from_slice(&frame[offset..]);
            } else {
                bytes_accumulated += frame_len;
                batch.extend_from_slice(&frame);
            }

            data_pos = frame_end;
            entry_idx += 1;
        }

        // Persist to in_progress periodically (not every batch) to reduce cloning
        if all_decrypted.len() - last_persisted_len > 512 * 1024 {
            let mut progress = shared.in_progress.lock();
            let existing_len = progress[idx].as_ref().map_or(0, |d| d.len());
            if all_decrypted.len() > existing_len {
                progress[idx] = Some(all_decrypted.clone());
                last_persisted_len = all_decrypted.len();
            }
        }

        if !batch.is_empty() && tx.send(Ok(Bytes::copy_from_slice(&batch))).await.is_err() {
            // Persist before exit so data survives for future seeks
            let mut progress = shared.in_progress.lock();
            let existing_len = progress[idx].as_ref().map_or(0, |d| d.len());
            if all_decrypted.len() > existing_len {
                progress[idx] = Some(all_decrypted);
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
            let existing_len = progress[idx].as_ref().map_or(0, |d| d.len());
            if all_decrypted.len() > existing_len {
                progress[idx] = Some(all_decrypted);
            }
            return Ok(());
        }
    }

    // Trailing mdat data after last frame entry (unencrypted)
    let mdat_end = segment_crypto.mdat_end.min(buf.len());
    if data_pos < mdat_end {
        let trailing = &buf[data_pos..mdat_end];
        all_decrypted.extend_from_slice(trailing);

        if bytes_accumulated + trailing.len() > total_skip {
            let send_start = total_skip.saturating_sub(bytes_accumulated);
            if send_start < trailing.len() {
                let _ = tx
                    .send(Ok(Bytes::copy_from_slice(&trailing[send_start..])))
                    .await;
            }
        }
        bytes_accumulated += trailing.len();
    }

    shared.downloaded.lock()[idx] = Some(all_decrypted);
    shared.in_progress.lock()[idx] = None;

    let total_sent = bytes_accumulated.saturating_sub(skip_bytes);
    tracing::debug!(
        "Segment {seg}/{}: {total_sent} bytes streamed",
        shared.n_segments - 1,
    );

    Ok(())
}

impl SharedDownloadState {
    fn try_write_cache(&self) {
        if self.cache_written.swap(true, Ordering::AcqRel) {
            return;
        }

        let downloaded = self.downloaded.lock();
        if !downloaded.iter().all(|f| f.is_some()) {
            self.cache_written.store(false, Ordering::Release);
            return;
        }

        let mut cache_data = Vec::with_capacity(
            self.flac_header.len()
                + self
                    .segment_map
                    .iter()
                    .map(|s| s.byte_len as usize)
                    .sum::<usize>(),
        );
        cache_data.extend_from_slice(&self.flac_header);
        for f in downloaded.iter().flatten() {
            cache_data.extend_from_slice(f);
        }
        drop(downloaded);

        if let Some(parent) = self.cache_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let tmp = self.cache_path.with_extension("partial");
        if let Err(e) = fs::write(&tmp, &cache_data) {
            tracing::warn!("Failed to write cache: {e}");
        } else if let Err(e) = fs::rename(&tmp, &self.cache_path) {
            let _ = fs::remove_file(&tmp);
            tracing::warn!("Failed to finalize cache: {e}");
        } else {
            tracing::info!(
                "Cached: {} ({} bytes)",
                self.cache_path.display(),
                cache_data.len()
            );
        }
    }
}
