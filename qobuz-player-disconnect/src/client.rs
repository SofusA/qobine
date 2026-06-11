use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use qobuz_player_controls::{
    Status,
    controls::{ControlCommand, Controls},
    tracklist::Tracklist,
};
use qobuz_player_disconnect_server::DisconnectServerEvent;
use qobuz_player_player::AppResult;
use reqwest::Client;
use tokio::sync::{mpsc, watch};

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
    available_devices_sender: watch::Sender<Vec<String>>,
    active_device_sender: watch::Sender<String>,
}

#[allow(clippy::too_many_arguments)]
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
        available_devices_sender: watch::Sender<Vec<String>>,
        active_device_sender: watch::Sender<String>,
    ) -> Self {
        let secret = format!("{:x}", md5::compute(password));

        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
            secret,
            controls,
            device_name: device_name.to_string(),
            tracklist_sender,
            position_sender,
            volume_sender,
            auto_play_sender,
            status_sender,
            active_sender,
            available_devices_sender,
            active_device_sender,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}?secret={}", self.base_url, path, self.secret)
    }

    fn device_url(&self, path: &str) -> String {
        format!("{}&device_id={}", self.url(path), self.device_name)
    }

    pub async fn get_state(&self) -> AppResult<qobuz_player_disconnect_server::DisconnectState> {
        let res = self
            .client
            .get(self.url("/state"))
            .send()
            .await?
            .json::<qobuz_player_disconnect_server::DisconnectState>()
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
        &mut self,
        mut set_active_device_receiver: mpsc::UnboundedReceiver<String>,
    ) -> AppResult<()> {
        let url = format!(
            "{}/stream?secret={}&device_id={}",
            self.base_url, self.secret, self.device_name
        );

        let resp = self.client.get(url).send().await?;
        let mut stream = resp.bytes_stream().eventsource();

        let initial_state = self.get_state().await?;
        _ = self
            .active_sender
            .send(initial_state.active_device == self.device_name);
        _ = self.active_device_sender.send(initial_state.active_device);
        _ = self.tracklist_sender.send(initial_state.tracklist);
        _ = self.status_sender.send(initial_state.playback_status);
        _ = self.volume_sender.send(initial_state.volume);
        _ = self.position_sender.send(initial_state.position);
        _ = self.auto_play_sender.send(initial_state.auto_play);

        loop {
            tokio::select! {
                changed = set_active_device_receiver.recv() => {
                    match changed {
                        Some(device) => {
                            tracing::info!("Setting current device to {:?}", device);

                            if let Err(err) = self.set_active_device(&device).await {
                                tracing::error!("Failed setting current device: {:?}", err);
                            }
                        }
                        None => {
                            tracing::warn!("Active device sender dropped");
                            break;
                        }
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
                                        "New active device: {:?}. I am {}, and therefore i am active: {}",
                                        device,
                                        self.device_name,
                                        is_active
                                    );

                                    _ = self.active_sender.send(is_active);
                                    _ = self.active_device_sender.send(device);
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
                                    _ = self.available_devices_sender.send(devices);
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
