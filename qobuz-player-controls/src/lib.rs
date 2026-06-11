use std::time::Duration;

use tokio::sync::{broadcast, watch};

use crate::tracklist::Tracklist;

pub mod controls;
pub mod models;
pub mod tracklist;

pub type PositionReceiver = watch::Receiver<Duration>;
pub type VolumeReceiver = watch::Receiver<f32>;
pub type AutoPlayReceiver = watch::Receiver<bool>;
pub type StatusReceiver = watch::Receiver<Status>;
pub type TracklistReceiver = watch::Receiver<Tracklist>;

#[derive(Default, Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Status {
    Playing,
    Buffering,
    #[default]
    Paused,
}

pub type ExitReceiver = broadcast::Receiver<bool>;
pub type ExitSender = broadcast::Sender<bool>;
