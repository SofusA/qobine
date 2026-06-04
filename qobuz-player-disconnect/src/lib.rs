use qobuz_player_controls::{
    AppResult, PositionReceiver, StatusReceiver, TracklistReceiver, VolumeReceiver,
    controls::{ControlCommand, Controls},
};
use tokio::sync::broadcast;

use crate::client::DisconnectClient;

pub mod client;

struct DisconnectState {
    client: DisconnectClient,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    controls_rx: broadcast::Receiver<ControlCommand>,
}

impl DisconnectState {
    async fn start(&mut self) {
        tokio::spawn({
            let client = self.client.clone();
            async move {
                client.connect_and_listen().await.unwrap();
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
                Ok(notification) = self.controls_rx.recv() => {
                    println!("got control command: {:?}", notification);
                    self.client.control(&notification).await.unwrap();
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn init(
    disconnect_client: DisconnectClient,
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
) -> AppResult<()> {
    let mut state = DisconnectState {
        client: disconnect_client.clone(),
        position_receiver: position_receiver.clone(),
        tracklist_receiver: tracklist_receiver.clone(),
        status_receiver: status_receiver.clone(),
        volume_receiver: volume_receiver.clone(),
        controls_rx: controls.subscribe(),
    };

    state.start().await;

    Ok(())
}
