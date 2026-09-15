use std::sync::Arc;
use std::time::{Duration, Instant};

use controls_module::{
    PositionReceiver, Status, StatusReceiver, TracklistReceiver, VolumeReceiver,
    controls::{Controls, NewQueueItem},
};
use num_traits::ToPrimitive;
use player_module::{AppResult, AudioQuality, client::StreamClient, error::PlayerError};
use qobuz_connect::proto::qconnect::{
    AudioQuality as ConnectQuality, BufferState, DeviceType, NetworkType, PlayingState,
    QueueTrackRef,
};
use qobuz_connect::{
    Autoplay, ControllerCommand, Credentials, Device, Error, Event, PlayerState, QueueEvent,
    RendererCommand, RendererReport, Session,
};

const REPORT_INTERVAL: Duration = Duration::from_secs(1);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

struct Connect {
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    max_audio_quality: ConnectQuality,
    muted: bool,
    volume_before_mute: f32,
    reported_volume: Option<u32>,
    reported_state_at: Instant,
    adopted: bool,
    session_queue: Vec<(i32, u32)>,
    seen: Vec<u64>,
    pending: Option<Pending>,
    deferred: Option<Deferred>,
}

struct Deferred {
    playing: Option<PlayingState>,
    position: Option<Duration>,
    current: QueueTrackRef,
}

struct Pending {
    action: Vec<u8>,
    queue_ids: Vec<u64>,
    since: Instant,
}

type LocalItem = (u64, Option<i32>, u32);

pub async fn init(
    client: Arc<StreamClient>,
    connect_name: String,
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    max_audio_quality: AudioQuality,
) -> AppResult<()> {
    let device = device(connect_name, max_audio_quality);
    let session = Session::join_with(
        move || {
            let client = client.clone();
            async move { credentials(&client).await }
        },
        device,
    )
    .await
    .map_err(|err| map_err(&err))?;

    let mut connect = Connect {
        controls,
        position_receiver,
        tracklist_receiver,
        status_receiver,
        volume_receiver,
        max_audio_quality: connect_quality(max_audio_quality),
        muted: false,
        volume_before_mute: 1.0,
        reported_volume: None,
        reported_state_at: Instant::now(),
        adopted: false,
        session_queue: Vec::new(),
        seen: Vec::new(),
        pending: None,
        deferred: None,
    };
    connect.run(session).await.map_err(|err| map_err(&err))
}

async fn credentials(client: &StreamClient) -> Result<Credentials, Error> {
    let body = client
        .connect_token()
        .await
        .map_err(|err| Error::Token(err.to_string()))?;
    Credentials::from_json(body.as_bytes())
}

fn device(name: String, max_audio_quality: AudioQuality) -> Device {
    let host = hostname::get()
        .map(|host| host.to_string_lossy().into_owned())
        .unwrap_or_default();
    let uuid = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("{host}/{name}").as_bytes(),
    );
    Device {
        uuid: uuid.into_bytes(),
        name,
        brand: "qobine".to_owned(),
        model: "qobine".to_owned(),
        kind: DeviceType::Computer,
        max_audio_quality: connect_quality(max_audio_quality),
        volume_remote_control: true,
        software_version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

const fn connect_quality(quality: AudioQuality) -> ConnectQuality {
    match quality {
        AudioQuality::Mp3 => ConnectQuality::Mp3,
        AudioQuality::CD => ConnectQuality::Cd,
        AudioQuality::HIFI96 => ConnectQuality::HiresLevel1,
        AudioQuality::HIFI192 => ConnectQuality::HiresLevel2,
    }
}

const fn audio_quality(quality: ConnectQuality) -> AudioQuality {
    match quality {
        ConnectQuality::Mp3 => AudioQuality::Mp3,
        ConnectQuality::Cd => AudioQuality::CD,
        ConnectQuality::HiresLevel1 => AudioQuality::HIFI96,
        ConnectQuality::HiresLevel2 | ConnectQuality::HiresLevel3 | ConnectQuality::Unknown => {
            AudioQuality::HIFI192
        }
    }
}

fn convert_volume(volume: f32) -> u32 {
    (volume * 100.0)
        .round()
        .clamp(0.0, 100.0)
        .to_u32()
        .unwrap_or(0)
}

fn items(tracks: &[QueueTrackRef]) -> Vec<NewQueueItem> {
    tracks
        .iter()
        .map(|track| NewQueueItem {
            track_id: track.track_id,
            connect_id: track.queue_item_id,
        })
        .collect()
}

impl Connect {
    async fn run(&mut self, mut session: Session) -> Result<(), Error> {
        loop {
            tokio::select! {
                event = session.recv() => {
                    let Some(event) = event else { return Err(Error::Closed) };
                    self.handle_event(&mut session, event).await?;
                }
                Ok(()) = self.position_receiver.changed() => {
                    if self.reported_state_at.elapsed() >= REPORT_INTERVAL {
                        self.report_state(&session).await?;
                    }
                }
                Ok(()) = self.status_receiver.changed() => {
                    self.report_state(&session).await?;
                }
                Ok(()) = self.tracklist_receiver.changed() => {
                    self.report_state(&session).await?;
                    self.mirror(&mut session).await?;
                    self.apply_deferred(&session).await?;
                }
                Ok(()) = self.volume_receiver.changed() => {
                    self.report_volume(&session).await?;
                }
            }
        }
    }

    async fn handle_event(&mut self, session: &mut Session, event: Event) -> Result<(), Error> {
        match event {
            Event::Command(command) => self.handle_command(session, command).await,
            Event::Queue(queue) => self.handle_queue(session, queue).await,
            Event::Registered { renderer_id } => {
                tracing::info!("Registered as Qobuz Connect renderer {renderer_id}");
                Ok(())
            }
            Event::Reconnected => {
                self.pending = None;
                Ok(())
            }
            other => {
                tracing::debug!("Ignoring Qobuz Connect event: {other:?}");
                Ok(())
            }
        }
    }

    async fn handle_command(
        &mut self,
        session: &Session,
        command: RendererCommand,
    ) -> Result<(), Error> {
        tracing::info!("Qobuz Connect command: {command:?}");
        match command {
            RendererCommand::SetState {
                playing,
                position,
                current,
                next: _,
            } => self.set_state(session, playing, position, current).await,
            RendererCommand::SetVolume(volume) => {
                self.controls
                    .set_volume(volume.to_f32().unwrap_or(0.0) / 100.0);
                Ok(())
            }
            RendererCommand::ChangeVolume(delta) => {
                let volume = *self.volume_receiver.borrow() * 100.0 + delta.to_f32().unwrap_or(0.0);
                self.controls.set_volume(volume.clamp(0.0, 100.0) / 100.0);
                Ok(())
            }
            RendererCommand::Mute(muted) => {
                if muted != self.muted {
                    if muted {
                        self.volume_before_mute = *self.volume_receiver.borrow();
                        self.controls.set_volume(0.0);
                    } else {
                        self.controls.set_volume(self.volume_before_mute);
                    }
                    self.muted = muted;
                }
                session.report(RendererReport::Muted(muted)).await
            }
            RendererCommand::SetActive(true) => {
                let volume = convert_volume(*self.volume_receiver.borrow());
                self.reported_volume = Some(volume);
                session.report(RendererReport::Volume(volume)).await?;
                session.report(RendererReport::Muted(self.muted)).await?;
                session
                    .report(RendererReport::MaxAudioQuality {
                        quality: self.max_audio_quality,
                        network: NetworkType::Wifi,
                    })
                    .await?;
                self.report_state(session).await
            }
            RendererCommand::SetActive(false) => {
                self.controls.pause();
                Ok(())
            }
            RendererCommand::SetMaxAudioQuality(quality) => {
                self.max_audio_quality = quality;
                self.controls.set_audio_max_quality(audio_quality(quality));
                session
                    .report(RendererReport::MaxAudioQuality {
                        quality,
                        network: NetworkType::Wifi,
                    })
                    .await
            }
            RendererCommand::SetLoopMode(_) | RendererCommand::SetShuffleMode(_) => {
                tracing::info!("Loop and shuffle modes are not supported");
                Ok(())
            }
        }
    }

    async fn set_state(
        &mut self,
        session: &Session,
        playing: Option<PlayingState>,
        position: Option<Duration>,
        current: Option<QueueTrackRef>,
    ) -> Result<(), Error> {
        let jump = match current {
            Some(track) if track.queue_item_id < 0 => {
                self.controls.pause();
                return Ok(());
            }
            Some(track) => {
                let tracklist = self.tracklist_receiver.borrow().clone();
                if tracklist.current_connect_id() == Some(track.queue_item_id) {
                    false
                } else {
                    let Some(index) = tracklist.position_of_connect_id(track.queue_item_id) else {
                        return self.defer(session, playing, position, track).await;
                    };
                    self.controls.skip_to_position(index, true);
                    true
                }
            }
            None => false,
        };
        if let Some(position) = position
            && !(jump && position.is_zero())
        {
            self.controls.seek(position);
        }
        let playing = if jump {
            playing.or(Some(PlayingState::Playing))
        } else {
            playing
        };
        match playing {
            Some(PlayingState::Playing) => self.controls.play(),
            Some(PlayingState::Paused | PlayingState::Stopped) => self.controls.pause(),
            Some(PlayingState::Unknown) | None => {}
        }
        Ok(())
    }

    /// Keeps a jump to an item the queue does not hold yet until the queue has caught up.
    async fn defer(
        &mut self,
        session: &Session,
        playing: Option<PlayingState>,
        position: Option<Duration>,
        current: QueueTrackRef,
    ) -> Result<(), Error> {
        let expected = self
            .session_queue
            .iter()
            .any(|(connect_id, _)| *connect_id == current.queue_item_id);
        self.deferred = Some(Deferred {
            playing,
            position,
            current,
        });
        if expected {
            Ok(())
        } else {
            session.ask_queue_state().await
        }
    }

    async fn apply_deferred(&mut self, session: &Session) -> Result<(), Error> {
        let Some(deferred) = self.deferred.take() else {
            return Ok(());
        };
        let known = self
            .tracklist_receiver
            .borrow()
            .position_of_connect_id(deferred.current.queue_item_id)
            .is_some();
        if known {
            self.set_state(
                session,
                deferred.playing,
                deferred.position,
                Some(deferred.current),
            )
            .await
        } else {
            self.deferred = Some(deferred);
            Ok(())
        }
    }

    async fn handle_queue(
        &mut self,
        session: &mut Session,
        queue: QueueEvent,
    ) -> Result<(), Error> {
        let own = self
            .pending
            .as_ref()
            .is_some_and(|pending| Some(pending.action.as_slice()) == queue.action_uuid());
        match queue {
            QueueEvent::State(state) => {
                tracing::info!(
                    "Qobuz Connect queue state with {} tracks",
                    state.tracks.len()
                );
                self.session_queue = state
                    .tracks
                    .iter()
                    .map(|track| (track.queue_item_id, track.track_id))
                    .collect();
                self.pending = None;
                self.seen.clear();
                let items = items(&state.tracks);
                let local_is_foreign = self
                    .tracklist_receiver
                    .borrow()
                    .queue()
                    .iter()
                    .all(|item| item.connect_id.is_none());
                if !self.adopted && !items.is_empty() && local_is_foreign {
                    self.controls.new_queue(items, false, None);
                } else {
                    self.controls.replace_queue(items);
                }
                self.adopted = true;
                Ok(())
            }
            QueueEvent::Loaded(loaded) if !own => {
                tracing::info!(
                    "Qobuz Connect loaded {} tracks, starting at {}",
                    loaded.tracks.len(),
                    loaded.queue_position
                );
                self.session_queue = loaded
                    .tracks
                    .iter()
                    .map(|track| (track.queue_item_id, track.track_id))
                    .collect();
                let items = loaded
                    .tracks
                    .iter()
                    .map(|track| NewQueueItem {
                        track_id: track.track_id,
                        connect_id: track.queue_item_id,
                    })
                    .collect();
                let start = usize::try_from(loaded.queue_position).ok();
                self.controls.new_queue(items, session.is_active(), start);
                Ok(())
            }
            QueueEvent::Loaded(loaded) if own => {
                self.adopt_ids(loaded.tracks.iter().map(|track| track.queue_item_id));
                session.ask_queue_state().await
            }
            QueueEvent::Added(added) if own => {
                self.adopt_ids(added.tracks.iter().map(|track| track.queue_item_id));
                session.ask_queue_state().await
            }
            QueueEvent::Inserted(inserted) if own => {
                self.adopt_ids(inserted.tracks.iter().map(|track| track.queue_item_id));
                session.ask_queue_state().await
            }
            QueueEvent::Cleared(_) => {
                self.session_queue.clear();
                self.controls.replace_queue(Vec::new());
                Ok(())
            }
            QueueEvent::LoopModeSet(_) | QueueEvent::Error(_) => Ok(()),
            _ => session.ask_queue_state().await,
        }
    }

    fn adopt_ids(&mut self, connect_ids: impl Iterator<Item = i32>) {
        let Some(pending) = &self.pending else { return };
        let ids: Vec<(u64, i32)> = pending.queue_ids.iter().copied().zip(connect_ids).collect();
        self.controls.set_connect_ids(ids);
    }

    async fn report_state(&mut self, session: &Session) -> Result<(), Error> {
        let Some(state) = self.player_state() else {
            return Ok(());
        };
        self.reported_state_at = Instant::now();
        session.report(RendererReport::State(state)).await
    }

    fn player_state(&self) -> Option<PlayerState> {
        let tracklist = self.tracklist_receiver.borrow();
        let current = tracklist.current_track();
        if current.is_some() && tracklist.current_connect_id().is_none() {
            return None;
        }
        let status = *self.status_receiver.borrow();
        let playing = match status {
            _ if current.is_none() => PlayingState::Stopped,
            Status::Playing => PlayingState::Playing,
            Status::Buffering | Status::Paused => PlayingState::Paused,
        };
        let buffer = if status == Status::Buffering {
            BufferState::Buffering
        } else {
            BufferState::Ok
        };
        Some(PlayerState {
            playing,
            buffer,
            position: *self.position_receiver.borrow(),
            duration: Duration::from_secs(u64::from(
                current.map_or(0, |track| track.duration_seconds),
            )),
            current_queue_item_id: tracklist.current_connect_id(),
            next_queue_item_id: tracklist.next_connect_id(),
        })
    }

    async fn report_volume(&mut self, session: &Session) -> Result<(), Error> {
        let volume = convert_volume(*self.volume_receiver.borrow());
        if self.muted || self.reported_volume == Some(volume) {
            return Ok(());
        }
        self.reported_volume = Some(volume);
        session.report(RendererReport::Volume(volume)).await
    }

    async fn mirror(&mut self, session: &mut Session) -> Result<(), Error> {
        let (local, current): (Vec<LocalItem>, usize) = {
            let tracklist = self.tracklist_receiver.borrow();
            let local = tracklist
                .queue()
                .iter()
                .map(|item| (item.queue_id, item.connect_id, item.track.id))
                .collect();
            (local, tracklist.current_position())
        };
        let queue_ids: Vec<u64> = local.iter().map(|item| item.0).collect();
        if queue_ids == self.seen {
            return Ok(());
        }
        if let Some(pending) = &self.pending {
            if pending.since.elapsed() < ANSWER_TIMEOUT {
                return Ok(());
            }
            tracing::warn!("Qobuz Connect did not answer a queue change, resynchronizing");
            self.pending = None;
            return session.ask_queue_state().await;
        }
        self.seen = queue_ids;
        let Some((command, queue_ids)) = delta(&self.session_queue, &local, current) else {
            return Ok(());
        };
        tracing::info!("Mirroring queue change to Qobuz Connect: {command:?}");
        let fresh_queue = local.iter().all(|item| item.1.is_none());
        if fresh_queue && !session.is_active() && session.renderer_id().is_some() {
            tracing::info!("Taking over as the active Qobuz Connect renderer");
            session.activate().await?;
        }
        if let Some(action) = session.control(command).await? {
            self.pending = Some(Pending {
                action,
                queue_ids,
                since: Instant::now(),
            });
        }
        Ok(())
    }
}

fn delta(
    session_queue: &[(i32, u32)],
    local: &[LocalItem],
    current: usize,
) -> Option<(ControllerCommand, Vec<u64>)> {
    let session_ids: Vec<i32> = session_queue.iter().map(|item| item.0).collect();
    let known: Vec<i32> = local.iter().filter_map(|item| item.1).collect();
    if local.is_empty() {
        return (!session_queue.is_empty()).then_some((ControllerCommand::ClearQueue, Vec::new()));
    }
    if known == session_ids && !known.is_empty() {
        return first_new_run(local);
    }
    let fresh = local.iter().any(|item| item.1.is_none());
    if !fresh && known.len() < session_ids.len() {
        let remaining: Vec<i32> = session_ids
            .iter()
            .copied()
            .filter(|id| known.contains(id))
            .collect();
        if remaining == known {
            let queue_item_ids = session_ids
                .iter()
                .copied()
                .filter(|id| !known.contains(id))
                .collect();
            let command = ControllerCommand::RemoveTracks {
                queue_item_ids,
                autoplay: Autoplay::default(),
            };
            return Some((command, Vec::new()));
        }
    }
    if !fresh
        && known.len() == session_ids.len()
        && let Some(command) = single_move(&session_ids, &known)
    {
        return Some((command, Vec::new()));
    }
    let command = ControllerCommand::LoadTracks {
        track_ids: local.iter().map(|item| item.2).collect(),
        position: u32::try_from(current).unwrap_or_default(),
        shuffle_seed: None,
        shuffle_pivot_index: None,
        autoplay: Autoplay::default(),
    };
    Some((command, local.iter().map(|item| item.0).collect()))
}

fn first_new_run(local: &[LocalItem]) -> Option<(ControllerCommand, Vec<u64>)> {
    let start = local.iter().position(|item| item.1.is_none())?;
    let run: Vec<&LocalItem> = local
        .get(start..)?
        .iter()
        .take_while(|item| item.1.is_none())
        .collect();
    let track_ids = run.iter().map(|item| item.2).collect();
    let queue_ids = run.iter().map(|item| item.0).collect();
    let at_end = start.saturating_add(run.len()) == local.len();
    let command = if at_end {
        ControllerCommand::AddTracks {
            track_ids,
            shuffle_seed: None,
            autoplay: Autoplay::default(),
        }
    } else {
        ControllerCommand::InsertTracks {
            track_ids,
            after: start
                .checked_sub(1)
                .and_then(|before| local.get(before))
                .and_then(|item| item.1),
            shuffle_seed: None,
            autoplay: Autoplay::default(),
        }
    };
    Some((command, queue_ids))
}

fn single_move(session_ids: &[i32], known: &[i32]) -> Option<ControllerCommand> {
    known.iter().enumerate().find_map(|(index, &moved)| {
        let without =
            |ids: &[i32]| -> Vec<i32> { ids.iter().copied().filter(|&id| id != moved).collect() };
        (without(session_ids) == without(known)).then(|| ControllerCommand::ReorderTracks {
            queue_item_ids: vec![moved],
            after: index
                .checked_sub(1)
                .and_then(|before| known.get(before))
                .copied(),
            autoplay: Autoplay::default(),
        })
    })
}

fn map_err(err: &Error) -> PlayerError {
    PlayerError::ConnectError {
        error: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queue_in_sync_needs_nothing() {
        let session = [(10, 1), (11, 2)];
        let local = [(0, Some(10), 1), (1, Some(11), 2)];
        assert!(delta(&session, &local, 0).is_none());
    }

    #[test]
    fn a_fresh_queue_is_loaded_from_the_current_track() {
        let local = [(0, None, 1), (1, None, 2)];
        assert!(matches!(
            delta(&[], &local, 1),
            Some((ControllerCommand::LoadTracks { track_ids, position: 1, .. }, ids))
                if track_ids == vec![1, 2] && ids == vec![0, 1]
        ));
    }

    #[test]
    fn tracks_added_at_the_end_are_added() {
        let session = [(10, 1)];
        let local = [(0, Some(10), 1), (1, None, 2), (2, None, 3)];
        assert!(matches!(
            delta(&session, &local, 0),
            Some((ControllerCommand::AddTracks { track_ids, .. }, ids))
                if track_ids == vec![2, 3] && ids == vec![1, 2]
        ));
    }

    #[test]
    fn tracks_inserted_in_the_middle_follow_their_predecessor() {
        let session = [(10, 1), (11, 2)];
        let local = [(0, Some(10), 1), (2, None, 3), (1, Some(11), 2)];
        assert!(matches!(
            delta(&session, &local, 0),
            Some((ControllerCommand::InsertTracks { track_ids, after: Some(10), .. }, ids))
                if track_ids == vec![3] && ids == vec![2]
        ));
    }

    #[test]
    fn missing_tracks_are_removed() {
        let session = [(10, 1), (11, 2), (12, 3)];
        let local = [(0, Some(10), 1), (2, Some(12), 3)];
        assert!(matches!(
            delta(&session, &local, 0),
            Some((ControllerCommand::RemoveTracks { queue_item_ids, .. }, ids))
                if queue_item_ids == vec![11] && ids.is_empty()
        ));
    }

    #[test]
    fn a_single_moved_track_is_reordered() {
        let session = [(10, 1), (11, 2), (12, 3)];
        let local = [(0, Some(10), 1), (2, Some(12), 3), (1, Some(11), 2)];
        assert!(matches!(
            delta(&session, &local, 0),
            Some((ControllerCommand::ReorderTracks { queue_item_ids, after: Some(10), .. }, _))
                if queue_item_ids == vec![12]
        ));
    }

    #[test]
    fn an_emptied_queue_is_cleared() {
        assert!(matches!(
            delta(&[(10, 1)], &[], 0),
            Some((ControllerCommand::ClearQueue, _))
        ));
    }
}
