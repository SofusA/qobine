use controls_module::models::Track;
use player_module::client::StreamClient;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use controls_module::{
    PositionReceiver, Status, StatusReceiver, TracklistReceiver, VolumeReceiver,
    controls::{Controls, NewQueueItem},
    tracklist::Tracklist,
};

use num_traits::ToPrimitive;
use player_module::{AppResult, AudioQuality, error::PlayerError};
use qonductor::{
    ActivationState, BufferState, Command, DeviceConfig, DeviceSession, Notification, PlayingState,
    SessionEvent, SessionManager,
    msg::{self, Position, QueueRendererState, report::VolumeChanged},
};

struct ConnectState {
    controls: Controls,
    client: Arc<StreamClient>,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    audio_quality: i32,
    connected: bool,
    queue_ids: Vec<u64>,
}

pub async fn init(
    app_id: &str,
    connect_name: String,
    connect_port: u16,
    controls: Controls,
    client: Arc<StreamClient>,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    max_audio_quality: AudioQuality,
) -> AppResult<()> {
    let audio_quality = convert_audio_quality(max_audio_quality);

    let mut connect_state = ConnectState {
        controls,
        client,
        position_receiver,
        tracklist_receiver,
        status_receiver,
        volume_receiver: volume_receiver.clone(),
        audio_quality,
        connected: false,
        queue_ids: vec![],
    };

    connect_state
        .run(app_id, connect_name, connect_port)
        .await
        .map_err(|x| map_err(&x))?;

    Ok(())
}

fn get_queue_index(queue_ids: &Vec<u64>, id: u32) -> Option<usize> {
    queue_ids
        .into_iter()
        .enumerate()
        .find(|(_i, x)| **x == id as u64)
        .map(|x| x.0)
}

fn get_queue_item_id(queue_ids: &Vec<u64>, tracklist: &Tracklist, id: u32) -> Option<u64> {
    let queue_index = get_queue_index(queue_ids, id);

    if let Some(queue_index) = queue_index {
        return Some(tracklist.queue()[queue_index].queue_id);
    }

    None
}

fn current_state(
    status: Status,
    position: &Duration,
    tracklist: &Tracklist,
    queue_ids: Vec<u64>,
) -> QueueRendererState {
    let mut response_state = msg::QueueRendererState::default();

    let current_state = match status {
        Status::Playing => PlayingState::Playing,
        Status::Buffering | Status::Paused => PlayingState::Paused,
    };

    let buffering_state = match status {
        Status::Buffering => BufferState::Buffering,
        _ => BufferState::Ok,
    };

    response_state.current_queue_item_id = get_queue_item_id(
        &queue_ids,
        tracklist,
        tracklist.current_queue_id().unwrap_or(0) as u32,
    )
    .map(|x| x as i32);
    response_state.next_queue_item_id = get_queue_item_id(
        &queue_ids,
        tracklist,
        tracklist.next_track_queue_id().unwrap_or(0) as u32,
    )
    .map(|x| x as i32);

    response_state.set_playing_state(current_state);
    response_state.set_buffer_state(buffering_state);

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|x| u64::try_from(x.as_millis()).ok());

    let position = u32::try_from(position.as_millis()).ok();
    response_state.current_position = Some(Position {
        timestamp,
        value: position,
    });

    let current_duration_ms = tracklist
        .current_track()
        .map(|x| x.duration_seconds.saturating_mul(1000));
    response_state.duration = current_duration_ms;

    response_state
}

const fn convert_audio_quality(max_audio_quality: AudioQuality) -> i32 {
    match max_audio_quality {
        AudioQuality::Mp3 => 1,
        AudioQuality::CD => 2,
        AudioQuality::HIFI96 => 3,
        AudioQuality::HIFI192 => 4,
    }
}

fn convert_volume(volume: f32) -> u32 {
    (volume * 100.0).clamp(0.0, 100.0).to_u32().unwrap_or(0)
}

impl ConnectState {
    pub fn queue_ids(&self) -> Vec<u64> {
        self.queue_ids.clone()
    }

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

        let new_state = current_state(status, &position, &tracklist, self.queue_ids());

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
        let new_state = current_state(status, &position, &tracklist, self.queue_ids());

        tracing::info!("after tracklist queue: {:?}", tracklist.queue().iter().map(|q| q.queue_id).collect::<Vec<_>>());

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
        let new_state = current_state(status, &position, &tracklist, self.queue_ids());
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
                    self.handle_event(event).await;
                }
                Ok(()) = self.position_receiver.changed() => {
                    self.handle_position_changed(&session).await?;
                },
                Ok(()) = self.tracklist_receiver.changed() => {
                    self.handle_tracklist_changed(&session).await?;
                },
                Ok(()) = self.volume_receiver.changed() => {
                    self.handle_volume_changed(&session).await?;
                }
                Ok(()) = self.status_receiver.changed() => {
                    self.handle_status_changed(&session).await?;
                }
            }
        }
    }

    async fn handle_event(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Command(command) => match command {
                Command::SetState { cmd, respond } => {
                    tracing::info!("Set state message received");
                    tracing::info!("{:?}", cmd);
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
                    }

                    let position = cmd
                        .current_position
                        .map(|x| Duration::from_millis(x.into()));

                    if let Some(position) = position {
                        tracing::info!("Seeking from connect message");
                        self.controls.seek(position);
                    }

                    let current_position =
                        self.tracklist_receiver.borrow().current_position() as usize;
                    tracing::info!("current_position: {:?}", current_position);

                    let tracklist_position = cmd
                        .current_queue_item
                        .map(|x| x.queue_item_id)
                        .and_then(|x| u32::try_from(x).ok());
                    tracing::info!("tracklist_position: {:?}", tracklist_position);

                    if let Some(tracklist_position) = tracklist_position {
                        let queue_position = get_queue_index(&self.queue_ids(), tracklist_position);
                        tracing::info!("queue_position: {:?}", queue_position);

                        if let Some(queue_position) = queue_position
                            && current_position != queue_position
                        {
                            tracing::info!("Skipping to {:?}", queue_position);
                            self.controls.skip_to_position(queue_position, true);
                            self.controls.seek(Duration::from_secs(0));
                        }
                    }

                    let queue_ids = self.queue_ids();
                    respond.send(current_state(
                        *self.status_receiver.borrow(),
                        &self.position_receiver.borrow(),
                        &self.tracklist_receiver.borrow(),
                        queue_ids,
                    ));
                }
                Command::SetActive { respond, cmd: _cmd } => {
                    tracing::info!("Device activated!");

                    let current_volume = convert_volume(*self.volume_receiver.borrow());
                    let position = self.position_receiver.borrow();
                    let tracklist = self.tracklist_receiver.borrow();
                    let response = current_state(
                        *self.status_receiver.borrow(),
                        &position,
                        &tracklist,
                        self.queue_ids(),
                    );

                    respond.send(ActivationState {
                        muted: false,
                        volume: current_volume,
                        max_quality: self.audio_quality,
                        playback: response,
                    });
                }
                Command::SetVolume { cmd, respond } => {
                    let volume = cmd.volume.and_then(|x| x.to_f32());
                    tracing::info!("Volume command received: {:?}", volume);

                    let current_volume = *self.volume_receiver.borrow() * 100.0;

                    if let Some(volume) = volume
                        && (volume - current_volume).abs() > 1.0
                    {
                        self.controls.set_volume(volume / 100.0);
                    }

                    let volume = volume.and_then(|x| x.to_u32());

                    respond.send(VolumeChanged { volume });
                }
                Command::Heartbeat { respond } => {
                    let status = self.status_receiver.borrow();
                    let position = self.position_receiver.borrow();
                    let tracklist = self.tracklist_receiver.borrow();
                    let response = match *status {
                        Status::Playing | Status::Buffering => Some(current_state(
                            *status,
                            &position,
                            &tracklist,
                            self.queue_ids(),
                        )),
                        Status::Paused => None,
                    };

                    tracing::info!("Sending heartbeat");
                    respond.send(response);
                }
            },
            SessionEvent::Notification(n) => match n {
                Notification::Connected => {
                    self.connected = true;
                    tracing::info!("Connected!");
                }
                Notification::DeviceRegistered { renderer_id, .. } => {
                    tracing::info!("Ignoring device registered as renderer {}", renderer_id);
                }
                Notification::QueueState(queue) => {
                    tracing::info!("Set queue state: {:?}", queue);
                    let mut queue_items: Vec<NewQueueItem> = vec![];

                    for track in queue.tracks {
                        queue_items.push(NewQueueItem {
                            track_id: track.track_id(),
                            queue_id: track.queue_item_id,
                        });
                        self.queue_ids.push(track.queue_item_id);
                    }

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

                    let mut queue_items: Vec<NewQueueItem> = vec![];
                    self.queue_ids = vec![];

                    for track in queue.tracks {
                        queue_items.push(NewQueueItem {
                            track_id: track.track_id(),
                            queue_id: track.queue_item_id,
                        });
                        self.queue_ids.push(track.queue_item_id);
                    }

                    let start_index = queue.queue_position.and_then(|x| usize::try_from(x).ok());
                    self.controls.new_queue(queue_items, false, start_index);

                    self.controls.play();
                }
                Notification::QueueTracksAdded(queue_tracks_added) => {
                    // Added in end of queue
                    tracing::info!("Queue tracks added: {:?}", queue_tracks_added);

                    let mut tracks: Vec<Track> = vec![];
                    for track in queue_tracks_added.tracks {
                        tracks.push(self.client.track(track.track_id()).await.unwrap());
                        self.queue_ids.push(track.queue_item_id);
                    }

                    self.controls.add_tracks_to_queue(tracks);
                }
                Notification::QueueTracksInserted(queue_tracks_inserted) => {
                    // Next in queue
                    tracing::info!("Queue tracks inserted: {:?}", queue_tracks_inserted);

                    let insert_after = queue_tracks_inserted.insert_after.map(|x| x as usize);
                    tracing::info!("Have to insert after {:?}", insert_after);

                    let mut new_tracks: Vec<Track> = vec![];
                    for track in &queue_tracks_inserted.tracks {
                        new_tracks.push(self.client.track(track.track_id()).await.unwrap());
                    }

                    if let Some(insert_after) = insert_after {
                        tracing::info!("before tracklist queue: {:?}", self.tracklist_receiver.borrow().queue().iter().map(|q| q.queue_id).collect::<Vec<_>>());
                        self.controls
                            .insert_tracks_to_queue(new_tracks, insert_after + 1);

                        let insert_at_index = match self
                            .queue_ids
                            .clone()
                            .into_iter()
                            .find(|x| insert_after as u64 == *x)
                        {
                            Some(idx) => idx + 1,
                            None => 0,
                        };
                        tracing::info!("Insert at {:?}", insert_at_index);

                        for (i, track) in queue_tracks_inserted.tracks.into_iter().enumerate() {
                            self.queue_ids
                                .insert(insert_at_index as usize + i, track.queue_item_id);
                            tracing::info!("Queue ids after: {:?}", self.queue_ids);
                        }
                    }
                }
                Notification::QueueTracksRemoved(queue_tracks_removed) => {
                    tracing::info!("Queue tracks removed: {:?}", queue_tracks_removed);

                    for id in queue_tracks_removed.queue_item_ids {
                        let queue_index = get_queue_index(&self.queue_ids(), id);
                        if let Some(queue_index) = queue_index {
                            self.controls.remove_index_from_queue(queue_index);
                            self.queue_ids.remove(queue_index);
                        }
                    }
                }
                Notification::QueueTracksReordered(reordered) => {
                    tracing::info!("Queue tracks reordered: {:?}", reordered);

                    if reordered.queue_item_ids.len() == 0 {
                        return;
                    }

                    let insert_after =
                        match get_queue_index(&self.queue_ids(), reordered.insert_after()) {
                            Some(x) => x + 1,
                            None => 0,
                        };

                    let start = get_queue_index(&self.queue_ids(), reordered.queue_item_ids[0]);
                    let end = match reordered.queue_item_ids.len() {
                        1 => start,
                        _ => get_queue_index(
                            &self.queue_ids(),
                            reordered.queue_item_ids[reordered.queue_item_ids.len() - 1],
                        ),
                    };

                    if let Some(start) = start
                        && let Some(end) = end
                    {
                        let mut indexes: Vec<usize> = (0..self.queue_ids.len()).collect();
                        let removed: Vec<usize> = indexes.drain(start..end + 1).collect();
                        indexes.splice(insert_after..insert_after, removed);

                        self.controls.reorder_queue(indexes.clone());

                        let reordered: Vec<_> =
                            indexes.iter().map(|&i| self.queue_ids[i].clone()).collect();
                        self.queue_ids = reordered;
                    }
                }
                Notification::VolumeChanged(volume) => {
                    tracing::info!("Volume changed: {:?}", volume);
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
                    // TODO: This will be needed when qobine is used as a controller
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

fn map_err(err: &qonductor::Error) -> PlayerError {
    PlayerError::ConnectError {
        error: err.to_string(),
    }
}
