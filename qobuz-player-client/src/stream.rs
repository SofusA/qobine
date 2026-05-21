use crate::Error;

pub mod cmaf;
pub mod crypto;
pub mod flac_source_stream;
pub mod passthrough_storage;

pub async fn fetch_segment(url: &str, index: u8) -> Result<Vec<u8>, Error> {
    let bytes = reqwest::get(url)
        .await
        .map_err(|e| Error::StreamError {
            message: format!("Failed to fetch segment {index}: {e}"),
        })?
        .bytes()
        .await
        .map_err(|e| Error::StreamError {
            message: format!("Failed to read segment {index} bytes: {e}"),
        })?;
    Ok(bytes.to_vec())
}
