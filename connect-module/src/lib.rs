use std::sync::Arc;
use std::time::{Duration, Instant};

use controls_module::{
    PositionReceiver, Status, StatusReceiver, TracklistReceiver, VolumeReceiver,
    controls::{ConnectDevice, Controls, NewQueueItem},
};
use num_traits::ToPrimitive;
use player_module::{AppResult, AudioQuality, client::StreamClient, error::PlayerError};
use qobuz_connect::proto::qconnect::{
    AudioQuality as ConnectQuality, BufferState, DeviceType, NetworkType, PlayingState,
    QueueTrackRef,
};
use qobuz_connect::{
    Autoplay, ControllerCommand, Credentials, Device, Error, Event, PlayerState, QueueEvent,
    RendererCommand, RendererEvent, RendererReport, Session,
};
use tokio::sync::{mpsc, watch};

const REPORT_INTERVAL: Duration = Duration::from_secs(1);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

struct Connect {
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    volume_receiver: VolumeReceiver,
    max_audio_quality: ConnectQuality,
    connected: bool,
    muted: bool,
    volume_before_mute: f32,
    reported_volume: Option<u32>,
    reported_state_at: Instant,
    session_queue: Option<Vec<(i32, u32)>>,
    pending: Option<Pending>,
    refused: bool,
    deferred: Option<Deferred>,
    devices: watch::Sender<Vec<ConnectDevice>>,
    activations: mpsc::UnboundedReceiver<i32>,
    renderers: Vec<ConnectDevice>,
    taking_over: bool,
    remote: Option<Remote>,
    played: Option<u64>,
}

struct Pending {
    action: Vec<u8>,
    since: Instant,
}

struct Deferred {
    playing: Option<PlayingState>,
    position: Option<Duration>,
    current: QueueTrackRef,
}

struct Remote {
    state: PlayerState,
    at: Instant,
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
    devices: watch::Sender<Vec<ConnectDevice>>,
    activations: mpsc::UnboundedReceiver<i32>,
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
        connected: true,
        muted: false,
        volume_before_mute: 1.0,
        reported_volume: None,
        reported_state_at: Instant::now(),
        session_queue: None,
        pending: None,
        refused: false,
        deferred: None,
        devices,
        activations,
        renderers: Vec::new(),
        taking_over: false,
        remote: None,
        played: None,
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

const fn audio_quality(quality: ConnectQuality) -> Option<AudioQuality> {
    match quality {
        ConnectQuality::Mp3 => Some(AudioQuality::Mp3),
        ConnectQuality::Cd => Some(AudioQuality::CD),
        ConnectQuality::HiresLevel1 => Some(AudioQuality::HIFI96),
        ConnectQuality::HiresLevel2 | ConnectQuality::HiresLevel3 => Some(AudioQuality::HIFI192),
        ConnectQuality::Unknown => None,
    }
}

fn convert_volume(volume: f32) -> u32 {
    (volume * 100.0)
        .round()
        .clamp(0.0, 100.0)
        .to_u32()
        .unwrap_or(0)
}

fn ask_remote_state(session: &Session, active: Option<i32>) -> Result<(), Error> {
    match active {
        Some(id) if Some(id) != session.renderer_id() => session.ask_renderer_state(id),
        _ => Ok(()),
    }
}

fn activate(session: &mut Session, id: i32) -> Result<(), Error> {
    tracing::info!("Making Qobuz Connect renderer {id} active");
    if session.renderer_id() == Some(id) {
        return session.activate();
    }
    session.control(ControllerCommand::SetActiveRenderer(id))?;
    Ok(())
}

fn update(renderers: &mut Vec<ConnectDevice>, event: RendererEvent) -> bool {
    match event {
        RendererEvent::Added { id, device } | RendererEvent::Updated { id, device } => {
            match renderers.iter_mut().find(|renderer| renderer.id == id) {
                Some(renderer) => renderer.name = device.name,
                None => renderers.push(ConnectDevice {
                    id,
                    name: device.name,
                    active: false,
                }),
            }
        }
        RendererEvent::Removed { id } => renderers.retain(|renderer| renderer.id != id),
        RendererEvent::ActiveChanged { id } => set_active(renderers, id),
        _ => return false,
    }
    true
}

fn set_active(renderers: &mut [ConnectDevice], id: Option<i32>) {
    for renderer in renderers {
        renderer.active = Some(renderer.id) == id;
    }
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
                    self.handle_event(&mut session, event)?;
                }
                Ok(()) = self.position_receiver.changed() => {
                    if self.reported_state_at.elapsed() >= REPORT_INTERVAL {
                        self.report_state(&session)?;
                    }
                }
                Ok(()) = self.status_receiver.changed() => {
                    self.report_state(&session)?;
                    let current = self.tracklist_receiver.borrow().current_queue_id();
                    if *self.status_receiver.borrow() == Status::Playing && self.played != current {
                        self.played = current;
                        self.take_over(&mut session)?;
                    }
                }
                Ok(()) = self.tracklist_receiver.changed() => {
                    self.report_state(&session)?;
                    self.mirror(&mut session)?;
                    self.apply_deferred(&session)?;
                }
                Ok(()) = self.volume_receiver.changed() => {
                    self.report_volume(&session)?;
                }
                Some(id) = self.activations.recv() => {
                    activate(&mut session, id)?;
                }
            }
        }
    }

    fn handle_event(&mut self, session: &mut Session, event: Event) -> Result<(), Error> {
        match event {
            Event::Command(command) => self.handle_command(session, command),
            Event::Queue(queue) => self.handle_queue(session, queue),
            Event::Registered { renderer_id } => {
                tracing::info!("Registered as Qobuz Connect renderer {renderer_id}");
                let device = session.device().clone();
                update(
                    &mut self.renderers,
                    RendererEvent::Added {
                        id: renderer_id,
                        device,
                    },
                );
                self.publish();
                Ok(())
            }
            Event::Session(state) => {
                set_active(&mut self.renderers, state.active_renderer_id);
                self.publish();
                ask_remote_state(session, state.active_renderer_id)
            }
            Event::Renderer(event) => {
                if let RendererEvent::StateUpdated {
                    id,
                    state: Some(state),
                    ..
                } = &event
                    && session.renderer_id() != Some(*id)
                {
                    self.remote = Some(Remote {
                        state: state.clone(),
                        at: Instant::now(),
                    });
                }
                if let RendererEvent::ActiveChanged { id } = event {
                    self.taking_over = false;
                    ask_remote_state(session, id)?;
                }
                if update(&mut self.renderers, event) {
                    self.publish();
                }
                Ok(())
            }
            Event::Disconnected => {
                self.connected = false;
                Ok(())
            }
            Event::Reconnected => {
                self.connected = true;
                self.pending = None;
                self.taking_over = false;
                self.renderers.clear();
                self.publish();
                Ok(())
            }
            other => {
                tracing::debug!("Ignoring Qobuz Connect event: {other:?}");
                Ok(())
            }
        }
    }

    fn handle_command(&mut self, session: &Session, command: RendererCommand) -> Result<(), Error> {
        tracing::info!("Qobuz Connect command: {command:?}");
        match command {
            RendererCommand::SetState {
                playing,
                position,
                current,
                next: _,
            } => self.set_state(session, playing, position, current),
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
                session.report(RendererReport::Muted(muted))
            }
            RendererCommand::SetActive(true) => {
                self.taking_over = false;
                let volume = convert_volume(*self.volume_receiver.borrow());
                self.reported_volume = Some(volume);
                session.report(RendererReport::Volume(volume))?;
                session.report(RendererReport::Muted(self.muted))?;
                session.report(RendererReport::MaxAudioQuality {
                    quality: self.max_audio_quality,
                    network: NetworkType::Wifi,
                })?;
                if let Some(remote) = self.remote.take() {
                    self.resume(session, remote)?;
                }
                self.report_state(session)
            }
            RendererCommand::SetActive(false) => {
                self.controls.pause();
                Ok(())
            }
            RendererCommand::SetMaxAudioQuality(quality) => {
                let Some(new_quality) = audio_quality(quality) else {
                    return Ok(());
                };
                self.max_audio_quality = quality;
                self.controls.set_audio_max_quality(new_quality);
                session.report(RendererReport::MaxAudioQuality {
                    quality,
                    network: NetworkType::Wifi,
                })
            }
            other => {
                tracing::info!("Unsupported Qobuz Connect command: {other:?}");
                Ok(())
            }
        }
    }

    fn set_state(
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
                        return self.defer(session, playing, position, track);
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
    fn defer(
        &mut self,
        session: &Session,
        playing: Option<PlayingState>,
        position: Option<Duration>,
        current: QueueTrackRef,
    ) -> Result<(), Error> {
        let expected = self
            .session_queue
            .as_ref()
            .is_some_and(|queue| queue.iter().any(|(id, _)| *id == current.queue_item_id));
        self.deferred = Some(Deferred {
            playing,
            position,
            current,
        });
        if expected {
            Ok(())
        } else {
            session.ask_queue_state()
        }
    }

    fn apply_deferred(&mut self, session: &Session) -> Result<(), Error> {
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
        } else {
            self.deferred = Some(deferred);
            Ok(())
        }
    }

    fn handle_queue(&mut self, session: &mut Session, queue: QueueEvent) -> Result<(), Error> {
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
                let tracks: Vec<(i32, u32)> = state
                    .tracks
                    .iter()
                    .map(|track| (track.queue_item_id, track.track_id))
                    .collect();
                self.adopt(tracks, items(&state.tracks));
                Ok(())
            }
            QueueEvent::Loaded(loaded) if !own => {
                tracing::info!(
                    "Qobuz Connect loaded {} tracks, starting at {}",
                    loaded.tracks.len(),
                    loaded.queue_position
                );
                self.session_queue = Some(
                    loaded
                        .tracks
                        .iter()
                        .map(|track| (track.queue_item_id, track.track_id))
                        .collect(),
                );
                let items = loaded
                    .tracks
                    .iter()
                    .map(|track| NewQueueItem {
                        track_id: track.track_id,
                        connect_id: track.queue_item_id,
                    })
                    .collect();
                let start = usize::try_from(loaded.queue_position).ok();
                self.remote = None;
                self.controls.new_queue(items, session.is_active(), start);
                Ok(())
            }
            QueueEvent::Cleared(_) => {
                self.adopt(Vec::new(), Vec::new());
                Ok(())
            }
            QueueEvent::Error(error) if own => {
                tracing::warn!("Qobuz Connect refused a queue change: {error:?}");
                self.refused = true;
                Ok(())
            }
            QueueEvent::LoopModeSet(_) | QueueEvent::Error(_) => Ok(()),
            _ => session.ask_queue_state(),
        }
    }

    /// Takes the session's queue; the local tracks it does not know yet survive only once the session is known and no change of ours was refused.
    fn adopt(&mut self, tracks: Vec<(i32, u32)>, items: Vec<NewQueueItem>) {
        let keep_unknown = self.session_queue.is_some() && !self.refused;
        self.session_queue = Some(tracks);
        self.pending = None;
        self.refused = false;
        self.remote = None;
        self.controls.replace_queue(items, keep_unknown);
    }

    fn report_state(&mut self, session: &Session) -> Result<(), Error> {
        if !session.is_active() {
            return Ok(());
        }
        let Some(state) = self.player_state() else {
            return Ok(());
        };
        self.reported_state_at = Instant::now();
        session.report(RendererReport::State(state))
    }

    fn resume(&mut self, session: &Session, remote: Remote) -> Result<(), Error> {
        let state = remote.state;
        let (Some(id), PlayingState::Playing | PlayingState::Paused) =
            (state.current_queue_item_id, state.playing)
        else {
            return Ok(());
        };
        let track_id = {
            let tracklist = self.tracklist_receiver.borrow();
            let index = tracklist.position_of_connect_id(id);
            index.and_then(|index| tracklist.queue().get(index).map(|item| item.track.id))
        };
        let Some(track_id) = track_id else {
            return Ok(());
        };
        let position = if state.playing == PlayingState::Playing && state.buffer == BufferState::Ok
        {
            state.position.saturating_add(remote.at.elapsed())
        } else {
            state.position
        };
        tracing::info!(
            "Continuing the previous renderer: item {id} at {position:?}, {:?}",
            state.playing
        );
        let current = QueueTrackRef {
            queue_item_id: id,
            track_id,
            context_uuid: None,
        };
        self.set_state(session, Some(state.playing), Some(position), Some(current))
    }

    fn take_over(&mut self, session: &mut Session) -> Result<(), Error> {
        if session.is_active() || self.taking_over || session.renderer_id().is_none() {
            return Ok(());
        }
        tracing::info!("Taking over as the active Qobuz Connect renderer");
        self.taking_over = true;
        self.remote = None;
        session.activate()
    }

    fn publish(&self) {
        self.devices.send_replace(self.renderers.clone());
    }

    fn player_state(&self) -> Option<PlayerState> {
        if !self.connected {
            return None;
        }
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

    fn report_volume(&mut self, session: &Session) -> Result<(), Error> {
        let volume = convert_volume(*self.volume_receiver.borrow());
        if !session.is_active() || self.muted || self.reported_volume == Some(volume) {
            return Ok(());
        }
        self.reported_volume = Some(volume);
        session.report(RendererReport::Volume(volume))
    }

    fn mirror(&mut self, session: &mut Session) -> Result<(), Error> {
        let Some(session_queue) = &self.session_queue else {
            return Ok(());
        };
        if !self.connected {
            return Ok(());
        }
        if let Some(pending) = &self.pending {
            if pending.since.elapsed() < ANSWER_TIMEOUT {
                return Ok(());
            }
            tracing::warn!("Qobuz Connect did not answer a queue change, resynchronizing");
            self.pending = None;
            self.refused = true;
            return session.ask_queue_state();
        }
        let (local, current): (Vec<LocalItem>, usize) = {
            let tracklist = self.tracklist_receiver.borrow();
            let local = tracklist
                .queue()
                .iter()
                .map(|item| (item.queue_id, item.connect_id, item.track.id))
                .collect();
            (local, tracklist.current_position())
        };
        let Some(command) = delta(session_queue, &local, current) else {
            return Ok(());
        };
        tracing::info!("Mirroring queue change to Qobuz Connect: {command:?}");
        if local.iter().all(|item| item.1.is_none()) {
            self.take_over(session)?;
        }
        if let Some(action) = session.control(command)? {
            self.pending = Some(Pending {
                action,
                since: Instant::now(),
            });
        }
        Ok(())
    }
}

/// The controller command that brings the session's queue to the local one, or `None` when they agree or the local queue lags behind an adoption.
fn delta(
    session_queue: &[(i32, u32)],
    local: &[LocalItem],
    current: usize,
) -> Option<ControllerCommand> {
    let session_ids: Vec<i32> = session_queue.iter().map(|item| item.0).collect();
    let known: Vec<i32> = local.iter().filter_map(|item| item.1).collect();
    if local.is_empty() {
        return (!session_queue.is_empty()).then_some(ControllerCommand::ClearQueue);
    }
    if known.iter().any(|id| !session_ids.contains(id)) {
        return None;
    }
    if known.is_empty() {
        return Some(load_tracks(local, current));
    }
    if known == session_ids {
        return first_new_run(local);
    }
    if local.iter().any(|item| item.1.is_none()) {
        return Some(load_tracks(local, current));
    }
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
        return Some(ControllerCommand::RemoveTracks {
            queue_item_ids,
            autoplay: Autoplay::default(),
        });
    }
    if known.len() == session_ids.len()
        && let Some(command) = single_move(&session_ids, &known)
    {
        return Some(command);
    }
    Some(load_tracks(local, current))
}

fn load_tracks(local: &[LocalItem], current: usize) -> ControllerCommand {
    ControllerCommand::LoadTracks {
        track_ids: local.iter().map(|item| item.2).collect(),
        position: u32::try_from(current).unwrap_or_default(),
        shuffle_seed: None,
        shuffle_pivot_index: None,
        autoplay: Autoplay::default(),
    }
}

fn first_new_run(local: &[LocalItem]) -> Option<ControllerCommand> {
    let start = local.iter().position(|item| item.1.is_none())?;
    let run: Vec<&LocalItem> = local
        .get(start..)?
        .iter()
        .take_while(|item| item.1.is_none())
        .collect();
    let track_ids = run.iter().map(|item| item.2).collect();
    let at_end = start.saturating_add(run.len()) == local.len();
    Some(if at_end {
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
    })
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
            Some(ControllerCommand::LoadTracks { track_ids, position: 1, .. })
                if track_ids == vec![1, 2]
        ));
    }

    #[test]
    fn tracks_added_at_the_end_are_added() {
        let session = [(10, 1)];
        let local = [(0, Some(10), 1), (1, None, 2), (2, None, 3)];
        assert!(matches!(
            delta(&session, &local, 0),
            Some(ControllerCommand::AddTracks { track_ids, .. }) if track_ids == vec![2, 3]
        ));
    }

    #[test]
    fn tracks_inserted_in_the_middle_follow_their_predecessor() {
        let session = [(10, 1), (11, 2)];
        let local = [(0, Some(10), 1), (2, None, 3), (1, Some(11), 2)];
        assert!(matches!(
            delta(&session, &local, 0),
            Some(ControllerCommand::InsertTracks { track_ids, after: Some(10), .. })
                if track_ids == vec![3]
        ));
    }

    #[test]
    fn tracks_inserted_at_the_front_have_no_predecessor() {
        let session = [(10, 1)];
        let local = [(2, None, 3), (0, Some(10), 1)];
        assert!(matches!(
            delta(&session, &local, 0),
            Some(ControllerCommand::InsertTracks { after: None, .. })
        ));
    }

    #[test]
    fn missing_tracks_are_removed() {
        let session = [(10, 1), (11, 2), (12, 3)];
        let local = [(0, Some(10), 1), (2, Some(12), 3)];
        assert!(matches!(
            delta(&session, &local, 0),
            Some(ControllerCommand::RemoveTracks { queue_item_ids, .. }) if queue_item_ids == vec![11]
        ));
    }

    #[test]
    fn a_single_moved_track_is_reordered() {
        let session = [(10, 1), (11, 2), (12, 3)];
        let local = [(0, Some(10), 1), (2, Some(12), 3), (1, Some(11), 2)];
        assert!(matches!(
            delta(&session, &local, 0),
            Some(ControllerCommand::ReorderTracks { queue_item_ids, after: Some(10), .. })
                if queue_item_ids == vec![12]
        ));
    }

    #[test]
    fn an_addition_and_a_removal_together_reload_the_queue() {
        let session = [(10, 1), (11, 2)];
        let local = [(0, Some(10), 1), (2, None, 3)];
        assert!(matches!(
            delta(&session, &local, 1),
            Some(ControllerCommand::LoadTracks { position: 1, .. })
        ));
    }

    #[test]
    fn a_queue_behind_an_adoption_waits() {
        let session = [(10, 1)];
        let local = [(0, Some(10), 1), (1, Some(11), 2), (2, None, 3)];
        assert!(delta(&session, &local, 0).is_none());
    }

    #[test]
    fn an_emptied_queue_is_cleared() {
        assert!(matches!(
            delta(&[(10, 1)], &[], 0),
            Some(ControllerCommand::ClearQueue)
        ));
    }

    #[test]
    fn renderers_follow_the_session() {
        let mut renderers = Vec::new();
        let phone = device("phone".to_owned(), AudioQuality::CD);
        let web = device("web".to_owned(), AudioQuality::CD);
        let added = RendererEvent::Added {
            id: 4,
            device: phone.clone(),
        };
        assert!(update(&mut renderers, added));
        assert!(update(
            &mut renderers,
            RendererEvent::Added { id: 6, device: web }
        ));
        assert!(update(
            &mut renderers,
            RendererEvent::ActiveChanged { id: Some(6) }
        ));
        assert!(!update(
            &mut renderers,
            RendererEvent::Volume { id: 6, volume: 3 }
        ));
        let renamed = RendererEvent::Updated {
            id: 4,
            device: Device {
                name: "my phone".to_owned(),
                ..phone
            },
        };
        assert!(update(&mut renderers, renamed));
        assert_eq!(
            renderers.iter().filter(|renderer| renderer.active).count(),
            1
        );
        assert!(update(&mut renderers, RendererEvent::Removed { id: 6 }));
        let expected = ConnectDevice {
            id: 4,
            name: "my phone".to_owned(),
            active: false,
        };
        assert_eq!(renderers, vec![expected]);
    }
}
