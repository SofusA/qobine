use std::time::{Duration, SystemTime};

use controls_module::{
    PositionReceiver, Status, StatusReceiver, TracklistReceiver, VolumeReceiver,
    controls::{Controls, NewQueueItem},
    tracklist::Tracklist,
};

use player_module::{AppResult, AudioQuality, error::Error};
use qonductor::{
    ActivationState, BufferState, Command, DeviceConfig, DeviceSession, Notification, PlayingState,
    SessionEvent, SessionManager,
    msg::{self, Position, QueueRendererState, report::VolumeChanged},
};

struct ConnectState {
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    audio_quality: i32,
    connected: bool,
}

#[allow(clippy::too_many_arguments)]
pub async fn init(
    app_id: &str,
    connect_name: String,
    connect_port: u16,
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    max_audio_quality: AudioQuality,
) -> AppResult<()> {
    let audio_quality = convert_audio_quality(max_audio_quality);

    let mut connect_state = ConnectState {
        controls,
        position_receiver,
        tracklist_receiver,
        status_receiver,
        volume_receiver: volume_receiver.clone(),
        audio_quality,
        connected: false,
    };

    connect_state
        .run(app_id, connect_name, connect_port)
        .await
        .map_err(map_err)?;

    Ok(())
}

fn current_state(
    status: &Status,
    position: &Duration,
    tracklist: &Tracklist,
) -> QueueRendererState {
    let mut response_state = msg::QueueRendererState::default();

    let current_state = match status {
        Status::Playing => PlayingState::Playing,
        Status::Buffering | Status::Paused => PlayingState::Paused,
    };

    let buffering_state = match status {
        Status::Playing | Status::Paused => BufferState::Ok,
        Status::Buffering => BufferState::Buffering,
    };

    response_state.current_queue_item_id = tracklist.current_queue_id().map(|x| x as i32);
    response_state.next_queue_item_id = tracklist.next_track_queue_id().map(|x| x as i32);

    response_state.set_playing_state(current_state);
    response_state.set_buffer_state(buffering_state);

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|x| x.as_millis() as u64);

    let position = Some(position.as_millis() as u32);
    response_state.current_position = Some(Position {
        timestamp,
        value: position,
    });

    let current_duration_ms = tracklist.current_track().map(|x| x.duration_seconds * 1000);
    response_state.duration = current_duration_ms;

    response_state
}

fn convert_audio_quality(max_audio_quality: AudioQuality) -> i32 {
    match max_audio_quality {
        AudioQuality::Mp3 => 1,
        AudioQuality::CD => 2,
        AudioQuality::HIFI96 => 3,
        AudioQuality::HIFI192 => 4,
    }
}

fn convert_volume(volume: f32) -> u32 {
    ((volume * 100.0) as u32).clamp(0, 100)
}

impl ConnectState {
    async fn handle_position_changed(&mut self, session: &DeviceSession) -> qonductor::Result<()> {
        if !self.connected {
            return Ok(());
        }
        let position = {
            let position = self.position_receiver.borrow_and_update();
            *position
        };
        let status = { *self.status_receiver.borrow() };
        let tracklist = self.tracklist_receiver.borrow().clone();

        let new_state = current_state(&status, &position, &tracklist);

        session.report_state(new_state).await?;
        Ok(())
    }

    async fn handle_tracklist_changed(&mut self, session: &DeviceSession) -> qonductor::Result<()> {
        if !self.connected {
            return Ok(());
        }
        let tracklist = self.tracklist_receiver.borrow_and_update().clone();
        let position = {
            let position = self.position_receiver.borrow();
            *position
        };
        let status = { *self.status_receiver.borrow() };
        let new_state = current_state(&status, &position, &tracklist);

        tracing::info!("Updating current state after tracklist change");
        session.report_state(new_state).await?;
        Ok(())
    }

    async fn handle_volume_changed(&mut self, session: &DeviceSession) -> qonductor::Result<()> {
        if !self.connected {
            return Ok(());
        }
        let volume = convert_volume(*self.volume_receiver.borrow_and_update());
        tracing::info!("Updating volume state after volume change");
        session.report_volume(volume).await?;
        Ok(())
    }

    async fn handle_status_changed(&mut self, session: &DeviceSession) -> qonductor::Result<()> {
        if !self.connected {
            return Ok(());
        }
        let position = {
            let position = self.position_receiver.borrow();
            *position
        };
        let status = { *self.status_receiver.borrow_and_update() };
        let tracklist = self.tracklist_receiver.borrow().clone();
        let new_state = current_state(&status, &position, &tracklist);
        session.report_state(new_state).await?;
        Ok(())
    }

    async fn run(
        &mut self,
        app_id: &str,
        connect_name: String,
        connect_port: u16,
    ) -> qonductor::Result<()> {
        let mut manager = SessionManager::start(connect_port, app_id).await?;

        let mut session = manager.add_device(DeviceConfig::new(connect_name)).await?;

        tokio::spawn(async move { manager.run().await });

        loop {
            tokio::select! {
                Some(event) = session.recv() => {
                    self.handle_event(event);
                }
                Ok(_) = self.position_receiver.changed() => {
                    self.handle_position_changed(&session).await?;
                },
                Ok(_) = self.tracklist_receiver.changed() => {
                    self.handle_tracklist_changed(&session).await?;
                },
                Ok(_) = self.volume_receiver.changed() => {
                    self.handle_volume_changed(&session).await?;
                }
                Ok(_) = self.status_receiver.changed() => {
                    self.handle_status_changed(&session).await?;
                }
            }
        }
    }

    fn handle_event(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Command(command) => match command {
                Command::SetState { cmd, respond } => {
                    tracing::info!("Set state message received");
                    tracing::info!("{:?}", cmd);
                    let response = msg::QueueRendererState::default();

                    match cmd.playing_state() {
                        PlayingState::Stopped | PlayingState::Paused => {
                            self.controls.pause();
                        }
                        PlayingState::Playing => {
                            self.controls.play();
                        }
                        PlayingState::Unknown => {
                            // don't change current playing state, used for seeking
                        }
                    };

                    let position = cmd
                        .current_position
                        .map(|x| Duration::from_millis(x.into()));

                    if let Some(position) = position {
                        tracing::info!("Seeking from connect message");
                        self.controls.seek(position);
                    }

                    let current_queue_id = self.tracklist_receiver.borrow().current_queue_id();

                    let tracklist_position = cmd
                        .current_queue_item
                        .map(|x| x.queue_item_id)
                        .map(|x| x as usize);

                    if let Some(tracklist_position) = tracklist_position
                        && let Some(current_queue_id) = current_queue_id
                        && current_queue_id != tracklist_position as u64
                    {
                        self.controls.skip_to_position(tracklist_position, true);
                    };

                    respond.send(response);
                }
                Command::SetActive { respond, cmd: _cmd } => {
                    tracing::info!("Device activated!");

                    let current_volume = convert_volume(*self.volume_receiver.borrow());
                    let status = self.status_receiver.borrow();
                    let position = self.position_receiver.borrow();
                    let tracklist = self.tracklist_receiver.borrow();
                    let response = current_state(&status, &position, &tracklist);

                    respond.send(ActivationState {
                        muted: false,
                        volume: current_volume,
                        max_quality: self.audio_quality,
                        playback: response,
                    });
                }
                Command::SetVolume { cmd, respond } => {
                    let volume = cmd.volume;
                    tracing::info!("Volume command received: {:?}", volume);

                    let current_volume = convert_volume(*self.volume_receiver.borrow());

                    if let Some(volume) = volume
                        && volume != current_volume
                    {
                        self.controls.set_volume(volume as f32 / 100.0);
                    }

                    respond.send(VolumeChanged { volume });
                }
                Command::Heartbeat { respond } => {
                    let status = self.status_receiver.borrow();
                    let position = self.position_receiver.borrow();
                    let tracklist = self.tracklist_receiver.borrow();
                    let response = match *status {
                        Status::Playing | Status::Buffering => {
                            Some(current_state(&status, &position, &tracklist))
                        }
                        Status::Paused => None,
                    };

                    tracing::info!("Sending heartbeat");
                    respond.send(response);
                }
            },
            SessionEvent::Notification(n) => match n {
                Notification::Connected => {
                    self.connected = true;
                    tracing::info!("Connected!")
                }
                Notification::DeviceRegistered { renderer_id, .. } => {
                    tracing::info!("Ignoring device registered as renderer {}", renderer_id);
                }
                Notification::QueueState(queue) => {
                    let queue_items = queue
                        .tracks
                        .into_iter()
                        .map(|x| NewQueueItem {
                            track_id: x.track_id(),
                            queue_id: x.queue_item_id,
                        })
                        .collect();
                    self.controls.new_queue(queue_items, false, None);
                }
                Notification::SessionState(session_state) => {
                    tracing::info!("Ignoring session state message: {:?}", session_state);
                }
                Notification::QueueCleared(_) => {
                    self.controls.clear_queue();
                }
                Notification::QueueLoadTracks(queue) => {
                    tracing::info!("Queue load tracks: {:?}", queue);

                    let queue_items = queue
                        .tracks
                        .into_iter()
                        .map(|x| NewQueueItem {
                            track_id: x.track_id(),
                            queue_id: x.queue_item_id,
                        })
                        .collect();

                    let start_index = queue.queue_position.map(|x| x as usize);
                    self.controls.new_queue(queue_items, false, start_index);

                    self.controls.play();
                }
                Notification::QueueTracksAdded(queue_tracks_added) => {
                    // Added in end of queue
                    tracing::info!("Queue tracks added: {:?}", queue_tracks_added);
                }
                Notification::QueueTracksInserted(queue_tracks_inserted) => {
                    // Next in queue
                    tracing::info!("Queue tracks inserted: {:?}", queue_tracks_inserted);
                }
                Notification::QueueTracksRemoved(queue_tracks_removed) => {
                    tracing::info!("Queue tracks removed: {:?}", queue_tracks_removed);
                }
                Notification::QueueTracksReordered(reordered) => {
                    tracing::info!("Queue tracks reordered: {:?}", reordered);
                }
                Notification::VolumeChanged(volume) => {
                    let volume = volume.volume;
                    tracing::info!("Volume received: {:?}", volume);

                    let current_volume = convert_volume(*self.volume_receiver.borrow());

                    if let Some(volume) = volume
                        && volume != current_volume
                    {
                        self.controls.set_volume(volume as f32 / 100.0);
                    }
                }
                Notification::AutoplayModeSet(_) => {
                    tracing::info!("Error. Autoplay not supported");
                }
                Notification::AutoplayTracksLoaded(_) => {
                    tracing::info!("Error. Autoplay not supported");
                }
                Notification::LoopModeSet(_) => {
                    tracing::info!("Error. Loop mode not supported");
                }
                Notification::ShuffleModeSet(_) => {
                    tracing::info!("Error. Shuffle not supported");
                }
                Notification::ActiveRendererChanged(_) => {
                    tracing::info!("Error. Active renderer not supported");
                }
                Notification::AddRenderer(_) => {
                    tracing::info!("Error. Add renderer not supported");
                }
                Notification::UpdateRenderer(_) => {
                    tracing::info!("Error. Update renderer not supported");
                }
                Notification::RemoveRenderer(_) => {
                    tracing::info!("Error. Remove renderer not supported");
                }
                Notification::RendererStateUpdated(_state_msg) => {
                    // TODO: This will be needed when qobuz-player is used as a controller
                    // let state = state_msg.state;
                    // tracing::info!("Error. Renderer state not supported: {:?}", state);
                }
                Notification::VolumeMuted(_) => {
                    tracing::info!("Error. Muting not supported");
                }
                Notification::MaxAudioQualityChanged(_) => {
                    tracing::info!("Error. Audio quality change in runtime is not supported");
                }
                Notification::FileAudioQualityChanged(_) => {
                    tracing::info!("Error. Audio quality change in runtime is not supported");
                }
                Notification::DeviceAudioQualityChanged(_) => {
                    tracing::info!("Error. Audio quality change in runtime is not supported");
                }
                Notification::Deactivated => {
                    tracing::info!("Error. Deactivate not supported. Exit?");
                }
                Notification::RestoreState(srvr_ctrl_renderer_state_updated) => {
                    tracing::info!("Restore state: {:?}", srvr_ctrl_renderer_state_updated);
                }
                Notification::Disconnected { session_id, reason } => {
                    tracing::info!("Disconnect: {}, {:?}", session_id, reason);
                    self.controls.clear_queue();
                    self.connected = false;
                }
                Notification::SessionClosed { device_uuid } => {
                    tracing::info!("Session closed: {:?}", device_uuid);
                }
                _ => {}
            },
        }
    }
}

fn map_err(err: qonductor::Error) -> Error {
    Error::ConnectError {
        error: err.to_string(),
    }
}
