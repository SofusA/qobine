use std::time::Duration;

use controls_module::{Status, controls::ControlCommand, tracklist::Tracklist};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DisconnectServerEvent {
    Tracklist(Tracklist),
    Status(Status),
    Position(Duration),
    Volume(f32),
    AutoPlay(bool),
    ActiveDevice(String),
    Control(ControlCommand),
    AvailableDevices(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DisconnectState {
    pub active_device: String,
    pub available_devices: Vec<String>,
    pub playback_status: Status,
    pub tracklist: Tracklist,
    pub position: Duration,
    pub volume: f32,
    pub auto_play: bool,
}
