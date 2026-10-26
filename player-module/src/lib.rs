use std::path::{Path, PathBuf};

use crate::error::PlayerError;

pub use qobuz_client::client::AudioQuality;

pub mod client;
pub mod database;
mod downloader;
pub mod error;
pub mod notification;
pub mod player;
mod prefetch;
mod report;
mod simple_cache;
mod sink;
mod stderr_redirect;

pub type AppResult<T, E = PlayerError> = std::result::Result<T, E>;

#[must_use]
pub fn expand_tilde(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();

    if (path_str == "~" || path_str.starts_with("~/"))
        && let Some(home) = dirs::home_dir()
    {
        return home.join(path_str.trim_start_matches("~/"));
    }

    path.to_path_buf()
}
