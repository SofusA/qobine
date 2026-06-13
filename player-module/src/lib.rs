use crate::error::Error;

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

pub type AppResult<T, E = Error> = std::result::Result<T, E>;
