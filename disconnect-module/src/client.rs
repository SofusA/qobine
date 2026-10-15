use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use cli_module::{SharedPlaybackDevice, SharedPlaybackDeviceId};
use controls_module::{
    Status,
    controls::{ControlCommand, Controls},
    tracklist::Tracklist,
};
use disconnect_server_module::DisconnectServerEvent;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use player_module::AppResult;
use reqwest::Client;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Default)]
struct DeviceState {
    available: Vec<String>,
    active: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DisconnectClient {
    client: Client,
    device_name: String,
    base_url: String,
    secret: String,
    controls: Controls,
    tracklist_sender: watch::Sender<Tracklist>,
    position_sender: watch::Sender<Duration>,
    volume_sender: watch::Sender<f32>,
    auto_play_sender: watch::Sender<bool>,
    status_sender: watch::Sender<Status>,
    active_sender: watch::Sender<bool>,
    devices_sender: watch::Sender<Vec<SharedPlaybackDevice>>,
    device_state: Arc<Mutex<DeviceState>>,
}

impl DisconnectClient {
    pub fn new(
        base_url: &str,
        password: &str,
        device_name: &str,
        controls: Controls,
        tracklist_sender: watch::Sender<Tracklist>,
        position_sender: watch::Sender<Duration>,
        volume_sender: watch::Sender<f32>,
        auto_play_sender: watch::Sender<bool>,
        status_sender: watch::Sender<Status>,
        active_sender: watch::Sender<bool>,
        devices_sender: watch::Sender<Vec<SharedPlaybackDevice>>,
    ) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
            secret: password.to_string(),
            controls,
            device_name: device_name.to_string(),
            tracklist_sender,
            position_sender,
            volume_sender,
            auto_play_sender,
            status_sender,
            active_sender,
            devices_sender,
            device_state: Arc::new(Mutex::new(DeviceState::default())),
        }
    }

    pub fn clear_devices(&self) {
        let mut state = self
            .device_state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        *state = DeviceState::default();
        self.devices_sender.send_replace(Vec::new());
    }

    fn update_available_devices(&self, available: Vec<String>) {
        let devices = {
            let mut state = self
                .device_state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);

            state.available = available;
            self.shared_devices(&state)
        };

        self.devices_sender.send_replace(devices);
    }

    fn update_active_device(&self, active: String) {
        let devices = {
            let mut state = self
                .device_state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);

            state.active = Some(active);
            self.shared_devices(&state)
        };

        self.devices_sender.send_replace(devices);
    }

    fn shared_devices(&self, state: &DeviceState) -> Vec<SharedPlaybackDevice> {
        state
            .available
            .iter()
            .map(|name| SharedPlaybackDevice {
                id: SharedPlaybackDeviceId::Disconnect(name.clone()),
                name: name.clone(),
                active: state.active.as_deref() == Some(name.as_str()),
                is_self: name == &self.device_name,
            })
            .collect()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}?secret={}", self.base_url, path, self.secret)
    }

    fn device_url(&self, path: &str) -> String {
        format!("{}&device_id={}", self.url(path), self.device_name)
    }

    pub async fn get_state(&self) -> AppResult<disconnect_server_module::DisconnectState> {
        let res = self
            .client
            .get(self.url("/state"))
            .send()
            .await?
            .json::<disconnect_server_module::DisconnectState>()
            .await?;

        Ok(res)
    }

    async fn set_active_device(&self, device_id: &str) -> AppResult<()> {
        self.client
            .post(self.url("/active-device"))
            .json(&serde_json::json!({ "device_id": device_id }))
            .send()
            .await?;

        Ok(())
    }

    pub async fn set_tracklist(&self, tracklist: &Tracklist) -> AppResult<()> {
        self.client
            .post(self.device_url("/tracklist"))
            .json(tracklist)
            .send()
            .await?;

        Ok(())
    }

    pub async fn set_playback_status(&self, status: &Status) -> AppResult<()> {
        self.client
            .post(self.device_url("/status"))
            .json(status)
            .send()
            .await?;

        Ok(())
    }

    pub async fn set_position(&self, position: &Duration) -> AppResult<()> {
        self.client
            .post(self.device_url("/position"))
            .json(position)
            .send()
            .await?;

        Ok(())
    }

    pub async fn set_volume(&self, volume: &f32) -> AppResult<()> {
        self.client
            .post(self.device_url("/volume"))
            .json(volume)
            .send()
            .await?;

        Ok(())
    }

    pub async fn set_auto_play(&self, enable: &bool) -> AppResult<()> {
        self.client
            .post(self.device_url("/autoplay"))
            .json(enable)
            .send()
            .await?;

        Ok(())
    }

    pub async fn control(&self, command: &ControlCommand) -> AppResult<()> {
        self.client
            .post(self.device_url("/control"))
            .json(command)
            .send()
            .await?;

        Ok(())
    }

    pub async fn connect_and_listen(
        &self,
        set_active_device_receiver: &mut mpsc::UnboundedReceiver<String>,
    ) -> AppResult<()> {
        let url = format!(
            "{}/stream?secret={}&device_id={}",
            self.base_url, self.secret, self.device_name
        );

        let resp = self.client.get(url).send().await?;
        let mut stream = resp.bytes_stream().eventsource();

        let initial_state = self.get_state().await?;
        let is_active = initial_state.active_device == self.device_name;

        self.active_sender.send_if_modified(|current| {
            if *current == is_active {
                false
            } else {
                *current = is_active;
                true
            }
        });

        self.update_active_device(initial_state.active_device);
        _ = self.tracklist_sender.send(initial_state.tracklist);
        _ = self.status_sender.send(initial_state.playback_status);
        _ = self.volume_sender.send(initial_state.volume);
        _ = self.position_sender.send(initial_state.position);
        _ = self.auto_play_sender.send(initial_state.auto_play);

        loop {
            tokio::select! {
                changed = set_active_device_receiver.recv() => {
                    if let Some(device) = changed {
                        tracing::info!("Setting current device to {:?}", device);

                        if let Err(err) = self.set_active_device(&device).await {
                            tracing::error!("Failed setting current device: {:?}", err);
                        }
                    } else {
                        tracing::warn!("Active device sender dropped");
                        break;
                    }
                }

                event = stream.next() => {
                    match event {
                        Some(Ok(ev)) => {
                            let parsed: DisconnectServerEvent =
                                match serde_json::from_str(&ev.data) {
                                    Ok(res) => res,
                                    Err(err) => {
                                        tracing::error!(
                                            "Error parsing Disconnect event: {err}"
                                        );
                                        continue;
                                    }
                                };

                            match parsed {
                                DisconnectServerEvent::Status(status) => {
                                    tracing::info!("Status update: {:?}", status);
                                    _ = self.status_sender.send(status);
                                }

                                DisconnectServerEvent::Tracklist(tracklist) => {
                                    tracing::info!("Tracklist update: {:?}", tracklist);
                                    _ = self.tracklist_sender.send(tracklist);
                                }

                                DisconnectServerEvent::Position(duration) => {
                                    tracing::info!("Position update: {:?}", duration);
                                    _ = self.position_sender.send(duration);
                                }

                                DisconnectServerEvent::ActiveDevice(device) => {
                                    let is_active = device == self.device_name;

                                    tracing::info!(
                                        "New active device: {:?}. I am {}, and therefore I am active: {}",
                                        device,
                                        self.device_name,
                                        is_active
                                    );

                                    self.active_sender.send_if_modified(|current| {
                                        if *current == is_active {
                                            false
                                        } else {
                                            *current = is_active;
                                            true
                                        }
                                    });

                                    self.update_active_device(device);
                                }

                                DisconnectServerEvent::Volume(volume) => {
                                    tracing::info!("Volume update: {:?}", volume);
                                    _ = self.volume_sender.send(volume);
                                }

                                DisconnectServerEvent::AutoPlay(enable) => {
                                    tracing::info!("Auto play update: {:?}", enable);
                                    _ = self.auto_play_sender.send(enable);
                                }

                                DisconnectServerEvent::Control(control_command) => {
                                    tracing::info!("Control: {:?}", control_command);
                                    self.controls.send(control_command);
                                }

                                DisconnectServerEvent::AvailableDevices(devices) => {
                                    tracing::info!("New available devices: {:?}", devices);
                                    self.update_available_devices(devices);
                                }
                            }
                        }

                        Some(Err(err)) => {
                            tracing::error!("Disconnect SSE error: {:?}", err);
                        }

                        None => {
                            tracing::warn!("Disconnect SSE stream ended");
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
