use std::time::Duration;

use qobuz_player_controls::{
    AutoPlayReceiver, PositionReceiver, Status, StatusReceiver, TracklistReceiver, VolumeReceiver,
    controls::{ControlCommand, Controls},
    tracklist::Tracklist,
};
use qobuz_player_player::AppResult;
use tokio::sync::{broadcast, mpsc, watch};

use crate::client::DisconnectClient;

mod client;

struct DisconnectState {
    client: DisconnectClient,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    auto_play_receiver: AutoPlayReceiver,
    controls_rx: broadcast::Receiver<ControlCommand>,
}

impl DisconnectState {
    async fn start(&mut self, set_active_device_receiver: mpsc::UnboundedReceiver<String>) {
        tokio::spawn({
            let mut client = self.client.clone();
            async move {
                client
                    .connect_and_listen(set_active_device_receiver)
                    .await
                    .unwrap();
            }
        });

        loop {
            tokio::select! {
                Ok(_) = self.position_receiver.changed() => {
                    let position = *self.position_receiver.borrow_and_update();
                    self.client.set_position(&position).await.unwrap();
                },
                Ok(_) = self.tracklist_receiver.changed() => {
                    let tracklist = self.tracklist_receiver.borrow_and_update().clone();
                    self.client.set_tracklist(&tracklist).await.unwrap();
                },
                Ok(_) = self.status_receiver.changed() => {
                    let status = *self.status_receiver.borrow_and_update();
                    self.client.set_playback_status(&status).await.unwrap();
                }
                Ok(_) = self.volume_receiver.changed() => {
                    let volume = *self.volume_receiver.borrow_and_update();
                    self.client.set_volume(&volume).await.unwrap();
                }
                Ok(_) = self.auto_play_receiver.changed() => {
                    let auto_play = *self.auto_play_receiver.borrow_and_update();
                    self.client.set_auto_play(&auto_play).await.unwrap();
                }
                Ok(notification) = self.controls_rx.recv() => {
                    self.client.control(&notification).await.unwrap();
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn init(
    server_url: &str,
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

    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    auto_play_receiver: AutoPlayReceiver,
    set_active_device_receiver: mpsc::UnboundedReceiver<String>,
) -> AppResult<()> {
    let controls_rx = controls.subscribe();
    let disconnect_client = DisconnectClient::new(
        server_url,
        password,
        device_name,
        controls,
        tracklist_sender,
        position_sender,
        volume_sender,
        auto_play_sender,
        status_sender,
        active_sender,
        available_devices_sender,
        active_device_sender,
    );

    let mut state = DisconnectState {
        client: disconnect_client,
        position_receiver,
        tracklist_receiver,
        status_receiver,
        volume_receiver,
        auto_play_receiver,
        controls_rx,
    };

    state.start(set_active_device_receiver).await;

    Ok(())
}
