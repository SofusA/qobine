use crate::error::PlayerError;

pub use qobuz_client::client::AudioQuality;

pub mod client;
pub mod database;
mod downloader;
pub mod error;
pub mod notification;
pub mod player;
mod simple_cache;
mod sink;
mod stderr_redirect;

pub type AppResult<T, E = PlayerError> = std::result::Result<T, E>;
