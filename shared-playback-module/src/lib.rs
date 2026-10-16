use tokio::sync::{mpsc, watch};

use std::sync::Arc;

use cli_module::{SharedPlaybackConfig, SharedPlaybackDevice, SharedPlaybackDeviceId};
use player_module::{AppResult, AudioQuality, client::StreamClient, player::PlayerBindings};

// TODO: Restart connect when max_audio_quality changes

pub async fn init(
    player_bindings: PlayerBindings,
    client: Arc<StreamClient>,
    mut client_config: watch::Receiver<SharedPlaybackConfig>,
    max_audio_quality: AudioQuality,
    devices: watch::Sender<Vec<SharedPlaybackDevice>>,
    mut activations: mpsc::UnboundedReceiver<SharedPlaybackDeviceId>,
    connect_port: u16,
) -> AppResult<()> {
    let mut activations_open = true;

    loop {
        let config = client_config.borrow_and_update().clone();

        match config {
            SharedPlaybackConfig::None => {
                tracing::info!("Shared playback disabled");
                devices.send_replace(Vec::new());

                tokio::select! {
                    changed = client_config.changed() => {
                        if changed.is_err() {
                            return Ok(());
                        }
                    }

                    activation = activations.recv(), if activations_open => {
                        if activation.is_none() {
                            activations_open = false;
                        }
                    }
                }
            }

            SharedPlaybackConfig::Connect(connect_name) => {
                devices.send_replace(Vec::new());
                tracing::info!("Starting Qobuz Connect as {connect_name}");

                let (activation_sender, activation_receiver) = mpsc::unbounded_channel();

                let backend = connect_module::run_connect(
                    client.clone(),
                    connect_name.clone(),
                    player_bindings.controls.clone(),
                    player_bindings.position_receiver.clone(),
                    player_bindings.tracklist_receiver.clone(),
                    player_bindings.status_receiver.clone(),
                    player_bindings.volume_receiver.clone(),
                    max_audio_quality,
                    devices.clone(),
                    activation_receiver,
                    connect_port,
                );

                tokio::pin!(backend);

                loop {
                    tokio::select! {
                        result = &mut backend => {
                            devices.send_replace(Vec::new());
                            return result;
                        }

                        changed = client_config.changed() => {
                            if changed.is_err() {
                                devices.send_replace(Vec::new());
                                return Ok(());
                            }

                            let next =
                                client_config.borrow_and_update().clone();

                            if next == SharedPlaybackConfig::Connect(
                                connect_name.clone()
                            ) {
                                continue;
                            }

                            tracing::info!(
                                "Qobuz Connect configuration changed"
                            );

                            break;
                        }

                        activation = activations.recv(),
                            if activations_open =>
                        {
                            match activation {
                                Some(
                                    SharedPlaybackDeviceId::Connect(id)
                                ) => {
                                    if activation_sender.send(id).is_err() {
                                        tracing::debug!(
                                            "Qobuz Connect activation \
                                             receiver closed"
                                        );
                                    }
                                }

                                Some(other) => {
                                    tracing::debug!(
                                        "Ignoring activation for inactive \
                                         backend: {other:?}"
                                    );
                                }

                                None => {
                                    activations_open = false;
                                }
                            }
                        }
                    }
                }

                /*
                 * `backend` is dropped here. This drops the Connect session,
                 * discovery service and temporary activation receiver.
                 */
                devices.send_replace(Vec::new());
            }

            SharedPlaybackConfig::Disconnect(disconnect_config) => {
                devices.send_replace(Vec::new());

                tracing::info!(
                    "Starting Disconnect client for {}",
                    disconnect_config.server_url
                );

                let (activation_sender, activation_receiver) = mpsc::unbounded_channel();

                let backend = disconnect_module::init(
                    &player_bindings,
                    disconnect_config.clone(),
                    devices.clone(),
                    activation_receiver,
                );

                tokio::pin!(backend);

                loop {
                    tokio::select! {
                        result = &mut backend => {
                            devices.send_replace(Vec::new());
                            return result;
                        }

                        changed = client_config.changed() => {
                            if changed.is_err() {
                                devices.send_replace(Vec::new());
                                return Ok(());
                            }

                            let next =
                                client_config.borrow_and_update().clone();

                            if next == SharedPlaybackConfig::Disconnect(
                                disconnect_config.clone()
                            ) {
                                continue;
                            }

                            tracing::info!(
                                "Disconnect configuration changed"
                            );

                            break;
                        }

                        activation = activations.recv(),
                            if activations_open =>
                        {
                            match activation {
                                Some(
                                    SharedPlaybackDeviceId::Disconnect(id)
                                ) => {
                                    if activation_sender.send(id).is_err() {
                                        tracing::debug!(
                                            "Disconnect activation receiver \
                                             closed"
                                        );
                                    }
                                }

                                Some(other) => {
                                    tracing::debug!(
                                        "Ignoring activation for inactive \
                                         backend: {other:?}"
                                    );
                                }

                                None => {
                                    activations_open = false;
                                }
                            }
                        }
                    }
                }

                /*
                 * Dropping `backend` must also stop its network connection.
                 * Disconnect must therefore not detach its main client loop
                 * using `tokio::spawn`.
                 */
                devices.send_replace(Vec::new());
            }
        }
    }
}
