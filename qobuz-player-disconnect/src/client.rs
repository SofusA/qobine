use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use qobuz_player_controls::{
    AppResult, Status,
    controls::{ControlCommand, Controls},
    tracklist::Tracklist,
};
use qobuz_player_disconnect_server::DisconnectServerEvent;
use reqwest::Client;
use tokio::sync::watch;

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
    status_sender: watch::Sender<Status>,
    active_sender: watch::Sender<bool>,
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
        status_sender: watch::Sender<Status>,
        active_sender: watch::Sender<bool>,
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
            status_sender,
            active_sender,
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

    pub async fn set_current_device(&self, device_id: &str) -> AppResult<()> {
        self.client
            .post(self.url("/current-device"))
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

    pub async fn control(&self, command: &ControlCommand) -> AppResult<()> {
        self.client
            .post(self.device_url("/control"))
            .json(command)
            .send()
            .await?;

        Ok(())
    }

    pub async fn connect_and_listen(&self) -> AppResult<()> {
        let url = format!(
            "{}/stream?secret={}&device_id={}",
            self.base_url, self.secret, self.device_name
        );

        let resp = self.client.get(url).send().await?;

        let mut stream = resp.bytes_stream().eventsource();

        while let Some(event) = stream.next().await {
            match event {
                Ok(ev) => {
                    let parsed: DisconnectServerEvent = match serde_json::from_str(&ev.data) {
                        Ok(res) => res,
                        Err(err) => {
                            tracing::error!("Error parsing Disconnect event: {err}");
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
                        }
                        DisconnectServerEvent::Volume(volume) => {
                            tracing::info!("Volume update: {:?}", volume);
                            _ = self.volume_sender.send(volume);
                        }
                        DisconnectServerEvent::Control(control_command) => {
                            tracing::info!("Control: {:?}", control_command);
                            self.controls.send(control_command);
                        }
                    }
                }

                Err(err) => {
                    tracing::error!("Disconnect SSE error: {:?}", err);
                }
            }
        }

        Ok(())
    }
}
