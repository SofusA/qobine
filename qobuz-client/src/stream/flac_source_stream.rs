use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use parking_lot::{Condvar, Mutex};

use crate::stream::{cmaf, crypto};

const FETCH_ATTEMPTS: u32 = 3;
const CHUNK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct SegmentByteInfo {
    pub byte_offset: u64,
    pub byte_len: u64,
}

pub struct FlacSourceParams {
    pub url_template: String,
    pub content_key: Option<[u8; 16]>,
    pub flac_header: Vec<u8>,
    pub cache_path: PathBuf,
    pub segment_map: Vec<SegmentByteInfo>,
}

#[derive(Default)]
struct Segment {
    data: Vec<u8>,
    complete: bool,
}

struct SharedDownloadState {
    url_template: String,
    content_key: Option<[u8; 16]>,
    flac_header: Vec<u8>,
    cache_path: PathBuf,
    segment_map: Vec<SegmentByteInfo>,
    segments: Mutex<Vec<Segment>>,
    ready: Condvar,
    wanted: AtomicUsize,
    failed: AtomicBool,
    closed: AtomicBool,
}

pub struct SegmentReader {
    shared: Arc<SharedDownloadState>,
    position: u64,
}

impl SegmentReader {
    #[must_use]
    pub fn new(params: FlacSourceParams) -> Self {
        let segments = params
            .segment_map
            .iter()
            .map(|_| Segment::default())
            .collect();

        let shared = Arc::new(SharedDownloadState {
            url_template: params.url_template,
            content_key: params.content_key,
            flac_header: params.flac_header,
            cache_path: params.cache_path,
            segment_map: params.segment_map,
            segments: Mutex::new(segments),
            ready: Condvar::new(),
            wanted: AtomicUsize::new(0),
            failed: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        });

        tokio::spawn(download(Arc::clone(&shared)));

        Self {
            shared,
            position: 0,
        }
    }
}

impl Read for SegmentReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let header_len = u64::try_from(self.shared.flac_header.len()).map_err(io::Error::other)?;

        if self.position < header_len {
            let start = usize::try_from(self.position).map_err(io::Error::other)?;
            let mut header = self.shared.flac_header.get(start..).unwrap_or_default();
            let read = header.read(buf)?;
            self.position = self
                .position
                .saturating_add(u64::try_from(read).map_err(io::Error::other)?);
            return Ok(read);
        }

        let offset = self.position.saturating_sub(header_len);
        let segment_map = &self.shared.segment_map;
        let index = segment_map.partition_point(|segment| {
            segment.byte_offset.saturating_add(segment.byte_len) <= offset
        });

        let Some(info) = segment_map.get(index) else {
            return Ok(0);
        };

        let within =
            usize::try_from(offset.saturating_sub(info.byte_offset)).map_err(io::Error::other)?;

        self.shared.wanted.store(index, Ordering::Relaxed);

        let mut segments = self.shared.segments.lock();

        loop {
            let segment = segments
                .get(index)
                .ok_or_else(|| io::Error::other("segment index out of bounds"))?;

            if let Some(mut available) = segment.data.get(within..).filter(|x| !x.is_empty()) {
                let read = available.read(buf)?;
                self.position = self
                    .position
                    .saturating_add(u64::try_from(read).map_err(io::Error::other)?);
                return Ok(read);
            }

            if segment.complete {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "segment shorter than its table entry",
                ));
            }

            if self.shared.failed.load(Ordering::Relaxed) {
                return Err(io::Error::other("segment download failed"));
            }

            self.shared.ready.wait(&mut segments);
        }
    }
}

impl Seek for SegmentReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let position = match pos {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::End(_) => None,
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid seek position"))?;

        self.position = position;
        Ok(position)
    }
}

impl Drop for SegmentReader {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Relaxed);
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

async fn download(shared: Arc<SharedDownloadState>) {
    while let Some(index) = shared.next_missing() {
        if shared.closed.load(Ordering::Relaxed) {
            return;
        }

        let mut attempt = 1;

        while let Err(error) = fetch_segment(&shared, index).await {
            if attempt >= FETCH_ATTEMPTS {
                tracing::error!("Giving up on segment download: {error}");
                shared.fail();
                return;
            }

            tracing::warn!("Retrying segment download: {error}");
            attempt = attempt.saturating_add(1);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    shared.write_cache();
}

async fn fetch_segment(shared: &SharedDownloadState, index: usize) -> Result<(), String> {
    let number = index.checked_add(1).ok_or("segment number overflow")?;
    let url = shared
        .url_template
        .replace("$SEGMENT$", &number.to_string());

    let mut response = tokio::time::timeout(CHUNK_TIMEOUT, reqwest::get(&url))
        .await
        .map_err(|_| format!("Segment {number}: timed out"))?
        .map_err(|e| format!("Failed to fetch segment {number}: {e}"))?;

    let mut buf = Vec::new();

    let segment_crypto = loop {
        let chunk = next_chunk(&mut response, number)
            .await?
            .ok_or_else(|| format!("Segment {number}: truncated before header"))?;

        buf.extend_from_slice(&chunk);

        if let Ok(segment_crypto) = cmaf::parse_segment_crypto(&buf) {
            break segment_crypto;
        }
    };

    let key = shared.content_key.unwrap_or([0_u8; 16]);
    let mut entries = segment_crypto.entries.iter().peekable();
    let mut data_pos = segment_crypto.data_offset;
    let mut produced = 0_usize;
    let mut ended = false;

    loop {
        let mut output = Vec::new();

        while let Some(entry) = entries.peek() {
            let frame_size = usize::try_from(entry.size)
                .map_err(|_| format!("Segment {number}: frame size does not fit usize"))?;

            let frame_end = data_pos
                .checked_add(frame_size)
                .ok_or_else(|| format!("Segment {number}: frame position overflow"))?;

            let Some(frame) = buf.get(data_pos..frame_end) else {
                break;
            };

            let mut frame = frame.to_vec();

            if entry.flags != 0 {
                crypto::decrypt_frame(&key, &entry.iv, &mut frame);
            }

            output.extend_from_slice(&frame);
            data_pos = frame_end;
            entries.next();
        }

        let complete = ended && entries.peek().is_none();

        if complete {
            let mdat_end = cmaf::parse_segment_crypto(&buf).map_or(data_pos, |x| x.mdat_end);

            if let Some(trailing) = buf.get(data_pos..mdat_end) {
                output.extend_from_slice(trailing);
            }
        } else if ended {
            return Err(format!("Segment {number}: truncated at frame"));
        }

        produced = shared.append(index, produced, &output, complete);

        if complete || shared.abandoned(index) {
            return Ok(());
        }

        match next_chunk(&mut response, number).await? {
            Some(chunk) => buf.extend_from_slice(&chunk),
            None => ended = true,
        }
    }
}

async fn next_chunk(
    response: &mut reqwest::Response,
    number: usize,
) -> Result<Option<bytes::Bytes>, String> {
    tokio::time::timeout(CHUNK_TIMEOUT, response.chunk())
        .await
        .map_err(|_| format!("Segment {number}: timed out"))?
        .map_err(|e| format!("Segment {number}: {e}"))
}

impl SharedDownloadState {
    fn next_missing(&self) -> Option<usize> {
        let segments = self.segments.lock();
        let wanted = self.wanted.load(Ordering::Relaxed);

        (wanted..segments.len())
            .chain(0..wanted)
            .find(|index| segments.get(*index).is_some_and(|x| !x.complete))
    }

    fn abandoned(&self, index: usize) -> bool {
        if self.closed.load(Ordering::Relaxed) {
            return true;
        }

        let wanted = self.wanted.load(Ordering::Relaxed);

        wanted != index
            && self
                .segments
                .lock()
                .get(wanted)
                .is_some_and(|x| !x.complete)
    }

    fn append(&self, index: usize, produced: usize, output: &[u8], complete: bool) -> usize {
        let mut segments = self.segments.lock();

        if let Some(segment) = segments.get_mut(index) {
            let start = segment.data.len().saturating_sub(produced);

            if let Some(new_data) = output.get(start..) {
                segment.data.extend_from_slice(new_data);
            }

            segment.complete = complete;

            if complete
                && let Some(info) = self.segment_map.get(index)
                && u64::try_from(segment.data.len()).ok() != Some(info.byte_len)
            {
                tracing::warn!(
                    "Segment {index}: {} bytes, table says {}",
                    segment.data.len(),
                    info.byte_len
                );
            }
        }

        self.ready.notify_all();
        produced.saturating_add(output.len())
    }

    fn fail(&self) {
        let _segments = self.segments.lock();
        self.failed.store(true, Ordering::Relaxed);
        self.ready.notify_all();
    }

    fn write_cache(&self) {
        let mut cache_data = self.flac_header.clone();

        for segment in self.segments.lock().iter() {
            cache_data.extend_from_slice(&segment.data);
        }

        if let Some(parent) = self.cache_path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            tracing::warn!("Failed to create cache directory: {error}");
            return;
        }

        let temporary_path = self.cache_path.with_extension("partial");

        if let Err(error) = fs::write(&temporary_path, &cache_data) {
            tracing::warn!("Failed to write cache: {error}");
        } else if let Err(error) = fs::rename(&temporary_path, &self.cache_path) {
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
