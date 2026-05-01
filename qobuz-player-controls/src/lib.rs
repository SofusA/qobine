use crate::{error::Error, tracklist::Tracklist};

use std::time::Duration;
use tokio::sync::{broadcast, watch};

pub use qobuz_player_client::client::AudioQuality;

pub mod client;
pub mod controls;
pub mod database;
mod downloader;
pub mod error;
pub mod models;
pub mod notification;
pub mod player;
mod simple_cache;
mod sink;
mod stderr_redirect;
pub mod tracklist;

pub type AppResult<T, E = Error> = std::result::Result<T, E>;

pub type PositionReceiver = watch::Receiver<Duration>;
pub type VolumeReceiver = watch::Receiver<f32>;
pub type StatusReceiver = watch::Receiver<Status>;
pub type TracklistReceiver = watch::Receiver<Tracklist>;

#[derive(Default, Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Status {
    Playing,
    Buffering,
    Paused,
    #[default]
    Stopped,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum Notification {
    Error(String),
    Warning(String),
    Success(String),
    Info(String),
}

pub type ExitReceiver = broadcast::Receiver<bool>;
pub type ExitSender = broadcast::Sender<bool>;
