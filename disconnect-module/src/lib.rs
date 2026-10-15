use std::time::Duration;

use cli_module::{ParsedDisconnect, SharedPlaybackDevice};
use controls_module::{
    AutoPlayReceiver, PositionReceiver, Status, StatusReceiver, TracklistReceiver, VolumeReceiver,
    controls::{ControlCommand, Controls},
    tracklist::Tracklist,
};
use player_module::{AppResult, player::PlayerBindings};
use tokio::sync::{broadcast, mpsc, watch};

use crate::client::DisconnectClient;

mod client;

struct DisconnectState {
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
    devices: watch::Sender<Vec<SharedPlaybackDevice>>,
    status_sender: watch::Sender<Status>,
    active_sender: watch::Sender<bool>,
}

impl DisconnectState {
    async fn run(
        &mut self,
        config: ParsedDisconnect,
        mut set_active_device_receiver: mpsc::UnboundedReceiver<String>,
    ) -> AppResult<()> {
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
            self.devices.clone(),
        );

        loop {
            tracing::info!("Attempting to connect to Disconnect server");

            let result = self
                .run_connection(&disconnect_client, &mut set_active_device_receiver)
                .await;

            disconnect_client.clear_devices();

            match result {
                Ok(()) => {
                    tracing::warn!("Disconnect client disconnected, retrying in 5 seconds");
                }

                Err(err) => {
                    tracing::error!(
                        "Disconnect client error: {err:?}, \
                         retrying in 5 seconds"
                    );
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn run_connection(
        &mut self,
        disconnect_client: &DisconnectClient,
        set_active_device_receiver: &mut mpsc::UnboundedReceiver<String>,
    ) -> AppResult<()> {
        let listen = disconnect_client.connect_and_listen(set_active_device_receiver);

        tokio::pin!(listen);

        loop {
            tokio::select! {
                result = &mut listen => {
                    return result;
                }

                Ok(()) = self.position_receiver.changed() => {
                    let position =
                        *self.position_receiver.borrow_and_update();

                    let _ = disconnect_client
                        .set_position(&position)
                        .await;
                }

                Ok(()) = self.tracklist_receiver.changed() => {
                    let tracklist = self
                        .tracklist_receiver
                        .borrow_and_update()
                        .clone();

                    let _ = disconnect_client
                        .set_tracklist(&tracklist)
                        .await;
                }

                Ok(()) = self.status_receiver.changed() => {
                    let status =
                        *self.status_receiver.borrow_and_update();

                    let _ = disconnect_client
                        .set_playback_status(&status)
                        .await;
                }

                Ok(()) = self.volume_receiver.changed() => {
                    let volume =
                        *self.volume_receiver.borrow_and_update();

                    let _ = disconnect_client
                        .set_volume(&volume)
                        .await;
                }

                Ok(()) = self.auto_play_receiver.changed() => {
                    let auto_play =
                        *self.auto_play_receiver.borrow_and_update();

                    let _ = disconnect_client
                        .set_auto_play(&auto_play)
                        .await;
                }

                Ok(notification) = self.controls_rx.recv() => {
                    let _ = disconnect_client
                        .control(&notification)
                        .await;
                }
            }
        }
    }
}

pub async fn init(
    player: &PlayerBindings,
    config: ParsedDisconnect,
    devices: watch::Sender<Vec<SharedPlaybackDevice>>,
    set_active_device_receiver: mpsc::UnboundedReceiver<String>,
) -> AppResult<()> {
    let controls = player.controls.clone();

    let mut state = DisconnectState {
        position_receiver: player.position_receiver.clone(),
        tracklist_receiver: player.tracklist_receiver.clone(),
        status_receiver: player.status_receiver.clone(),
        volume_receiver: player.volume_receiver.clone(),
        auto_play_receiver: player.auto_play_receiver.clone(),
        controls_rx: controls.subscribe(),

        controls,

        tracklist_sender: player.tracklist_sender.clone(),
        position_sender: player.position_sender.clone(),
        volume_sender: player.volume_sender.clone(),
        auto_play_sender: player.auto_play_sender.clone(),
        status_sender: player.status_sender.clone(),
        active_sender: player.active_sender.clone(),

        devices,
    };

    state.run(config, set_active_device_receiver).await
}
