use std::time::Duration;

use controls_module::{
    AutoPlayReceiver, PositionReceiver, Status, StatusReceiver, TracklistReceiver, VolumeReceiver,
    controls::{ControlCommand, Controls},
    tracklist::Tracklist,
};
use player_module::{AppResult, player::Player};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::client::DisconnectClient;

mod client;

pub fn spawn_disconnect(
    player: &Player,
    client_config: watch::Receiver<Option<DisconnectClientConfig>>,
    available_devices_sender: watch::Sender<Vec<String>>,
    active_device_sender: watch::Sender<String>,
    set_active_device_receiver: mpsc::UnboundedReceiver<String>,
) {
    let position_receiver = player.position();
    let position_sender = player.position_sender();
    let tracklist_receiver = player.tracklist();
    let tracklist_sender = player.tracklist_sender();
    let volume_receiver = player.volume();
    let volume_sender = player.volume_sender();
    let status_receiver = player.status();
    let status_sender = player.status_sender();
    let controls = player.controls();
    let active_sender = player.active_sender();
    let auto_play_sender = player.auto_play_sender();
    let auto_play_receiver = player.auto_play();

    tokio::spawn(async move {
        if let Err(error) = init(
            client_config,
            controls,
            tracklist_sender,
            position_sender,
            volume_sender,
            auto_play_sender,
            status_sender,
            active_sender,
            available_devices_sender,
            active_device_sender,
            position_receiver,
            tracklist_receiver,
            status_receiver,
            volume_receiver,
            auto_play_receiver,
            set_active_device_receiver,
        )
        .await
        {
            eprintln!("{error}");
            std::process::exit(1);
        }
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisconnectClientConfig {
    pub server_url: String,
    pub password: String,
    pub device_name: String,
}

struct DisconnectState {
    client_config: watch::Receiver<Option<DisconnectClientConfig>>,

    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    auto_play_receiver: AutoPlayReceiver,
    controls_rx: broadcast::Receiver<ControlCommand>,

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

impl DisconnectState {
    async fn start(&mut self, set_active_device_receiver: mpsc::UnboundedReceiver<String>) {
        let mut set_active_device_receiver = Some(set_active_device_receiver);

        loop {
            let Some(config) = self.client_config.borrow().clone() else {
                tracing::info!("Disconnect disabled");

                if self.client_config.changed().await.is_err() {
                    return;
                }

                continue;
            };

            tracing::info!("Connecting to disconnect server {}", config.server_url);

            let disconnect_client = DisconnectClient::new(
                &config.server_url,
                &config.password,
                &config.device_name,
                self.controls.clone(),
                self.tracklist_sender.clone(),
                self.position_sender.clone(),
                self.volume_sender.clone(),
                self.auto_play_sender.clone(),
                self.status_sender.clone(),
                self.active_sender.clone(),
                self.available_devices_sender.clone(),
                self.active_device_sender.clone(),
            );

            let mut listen_client = disconnect_client.clone();

            let rx = set_active_device_receiver
                .take()
                .expect("set_active_device_receiver missing");

            let (stop_tx, stop_rx) = oneshot::channel();

            let client_task = tokio::spawn(async move {
                let mut rx = rx;
                let mut stop_rx = stop_rx;

                loop {
                    tracing::info!("Attempting to connect to disconnect server");

                    tokio::select! {
                        _ = &mut stop_rx => {
                            tracing::info!("Stopping disconnect client");
                            break;
                        }

                        result = listen_client.connect_and_listen(&mut rx) => {
                            match result {
                                Ok(_) => {
                                    tracing::warn!(
                                        "Disconnect client disconnected, retrying in 5 seconds"
                                    );
                                }
                                Err(err) => {
                                    tracing::error!(
                                        "Disconnect client error: {:?}, retrying in 5 seconds",
                                        err
                                    );
                                }
                            }
                        }
                    }

                    tokio::select! {
                        _ = &mut stop_rx => break,
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                    }
                }

                rx
            });

            loop {
                tokio::select! {
                    Ok(_) = self.client_config.changed() => {
                        tracing::info!("Disconnect config changed");

                        let _ = stop_tx.send(());

                        match client_task.await {
                            Ok(rx) => {
                                set_active_device_receiver = Some(rx);
                            }
                            Err(err) => {
                                tracing::error!("Disconnect client task failed: {:?}", err);
                                return;
                            }
                        }

                        break;
                    }

                    Ok(_) = self.position_receiver.changed() => {
                        let position = *self.position_receiver.borrow_and_update();
                        let _ = disconnect_client.set_position(&position).await;
                    }

                    Ok(_) = self.tracklist_receiver.changed() => {
                        let tracklist =
                            self.tracklist_receiver.borrow_and_update().clone();

                        let _ = disconnect_client.set_tracklist(&tracklist).await;
                    }

                    Ok(_) = self.status_receiver.changed() => {
                        let status =
                            *self.status_receiver.borrow_and_update();

                        let _ = disconnect_client.set_playback_status(&status).await;
                    }

                    Ok(_) = self.volume_receiver.changed() => {
                        let volume =
                            *self.volume_receiver.borrow_and_update();

                        let _ = disconnect_client.set_volume(&volume).await;
                    }

                    Ok(_) = self.auto_play_receiver.changed() => {
                        let auto_play =
                            *self.auto_play_receiver.borrow_and_update();

                        let _ = disconnect_client.set_auto_play(&auto_play).await;
                    }

                    Ok(notification) = self.controls_rx.recv() => {
                        let _ = disconnect_client.control(&notification).await;
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn init(
    client_config: watch::Receiver<Option<DisconnectClientConfig>>,
    controls: Controls,

    tracklist_sender: watch::Sender<Tracklist>,
    position_sender: watch::Sender<Duration>,
    volume_sender: watch::Sender<f32>,
    auto_play_sender: watch::Sender<bool>,
    status_sender: watch::Sender<Status>,
    active_sender: watch::Sender<bool>,
    available_devices_sender: watch::Sender<Vec<String>>,
    active_device_sender: watch::Sender<String>,

    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    auto_play_receiver: AutoPlayReceiver,
    set_active_device_receiver: mpsc::UnboundedReceiver<String>,
) -> AppResult<()> {
    let controls_rx = controls.subscribe();

    let mut state = DisconnectState {
        client_config,

        position_receiver,
        tracklist_receiver,
        status_receiver,
        volume_receiver,
        auto_play_receiver,
        controls_rx,

        controls,

        tracklist_sender,
        position_sender,
        volume_sender,
        auto_play_sender,
        status_sender,
        active_sender,
        available_devices_sender,
        active_device_sender,
    };

    state.start(set_active_device_receiver).await;

    Ok(())
}
