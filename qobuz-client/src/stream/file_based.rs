use std::path::PathBuf;

use crate::{Error, Result, stream::flac_source_stream::SeekableStreamReader};

use stream_download::http::HttpStream;
use stream_download::http::reqwest::{Client as SdClient, Url as SdUrl};
use stream_download::source::SourceStream;
use stream_download::{Settings, StreamDownload};

use crate::stream::passthrough_storage::PassthroughStorageProvider;

pub async fn stream_track_file_based(
    url: &str,
    cache_path: &std::path::Path,
) -> Result<SeekableStreamReader> {
    let url_parsed: SdUrl = url.parse().map_err(|e: url::ParseError| Error::Stream {
        message: format!("invalid track URL: {e}"),
    })?;

    let stream = HttpStream::new(SdClient::new(), url_parsed)
        .await
        .map_err(|e| Error::Stream {
            message: format!("failed to open HTTP stream: {e}"),
        })?;

    let content_length = stream.content_length().unwrap_or(0);

    let partial_path = cache_path.with_extension("partial");
    let provider = PassthroughStorageProvider {
        partial_path: partial_path.clone(),
    };

    let download = StreamDownload::from_stream(
        stream,
        provider,
        Settings::default().prefetch_bytes(64 * 1024),
    )
    .await
    .map_err(|e| Error::Stream {
        message: format!("failed to create stream-download: {e}"),
    })?;

    if content_length > 0 {
        let handle = download.handle();
        let final_path = cache_path.to_path_buf();
        tokio::spawn(async move {
            handle.wait_for_completion().await;
            finalize_cache(&partial_path, &final_path, content_length);
        });
    }

    Ok(SeekableStreamReader::new(download, content_length))
}

fn finalize_cache(partial: &PathBuf, final_path: &PathBuf, expected: u64) {
    match std::fs::metadata(partial) {
        Ok(meta) if meta.len() == expected => {
            if let Err(e) = std::fs::rename(partial, final_path) {
                tracing::warn!("Failed to finalize cache: {e}");
                let _ = std::fs::remove_file(partial);
            } else {
                tracing::info!("Cached: {} ({} bytes)", final_path.display(), expected);
            }
        }
        Ok(meta) => {
            tracing::debug!(
                "Stream incomplete ({} of {} bytes), discarding partial",
                meta.len(),
                expected
            );
            let _ = std::fs::remove_file(partial);
        }
        Err(_) => {}
    }
}
