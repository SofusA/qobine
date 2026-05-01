use crate::{
    controls::NewQueueItem,
    models::{Album, Track, TrackStatus},
};
use rand::seq::SliceRandom;
use tokio::{
    select,
    sync::{
        mpsc,
        watch::{self, Receiver, Sender},
    },
    time::sleep,
};

use crate::{
    AppResult, ExitReceiver, PositionReceiver, Status, StatusReceiver, TracklistReceiver,
    VolumeReceiver,
    controls::{ControlCommand, Controls},
    database::Database,
    downloader::{DownloadResult, Downloader},
    notification::{Notification, NotificationBroadcast},
    sink::QueryTrackResult,
    tracklist::{QueueItem, TracklistType},
};
use std::{sync::Arc, time::Duration};

use crate::{
    client::Client,
    sink::Sink,
    tracklist::{self, Tracklist},
};

const INTERVAL_MS: u64 = 500;

pub struct Player {
    broadcast: Arc<NotificationBroadcast>,
    tracklist_tx: Sender<Tracklist>,
    tracklist_rx: Receiver<Tracklist>,
    target_status: Sender<Status>,
    client: Arc<Client>,
    sink: Sink,
    volume: Sender<f32>,
    position: Sender<Duration>,
    track_finished: Receiver<()>,
    controls_rx: mpsc::UnboundedReceiver<ControlCommand>,
    controls: Controls,
    database: Arc<Database>,
    next_track_is_queried: bool,
    next_track_in_sink_queue: bool,
    downloader: Downloader,
    state_change_delay: Option<Duration>,
    sample_rate_change_delay: Option<Duration>,
}

impl Player {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tracklist: Tracklist,
        client: Arc<Client>,
        volume: f32,
        broadcast: Arc<NotificationBroadcast>,
        audio_cache_dir: std::path::PathBuf,
        database: Arc<Database>,
        state_change_delay: Option<Duration>,
        sample_rate_change_delay: Option<Duration>,
        preferred_device_id: Option<String>,
    ) -> AppResult<Self> {
        let (volume, volume_receiver) = watch::channel(volume);
        let sink = Sink::new(volume_receiver, preferred_device_id)?;

        let downloader = Downloader::new(audio_cache_dir, database.clone(), client.clone());

        let track_finished = sink.track_finished();

        let (position, _) = watch::channel(Default::default());
        let (target_status, _) = watch::channel(Default::default());
        let (tracklist_tx, tracklist_rx) = watch::channel(tracklist);

        let (controls_tx, controls_rx) = tokio::sync::mpsc::unbounded_channel();
        let controls = Controls::new(controls_tx);

        Ok(Self {
            broadcast,
            tracklist_tx,
            tracklist_rx,
            controls_rx,
            controls,
            target_status,
            client,
            sink,
            volume,
            position,
            track_finished,
            database,
            next_track_in_sink_queue: false,
            next_track_is_queried: false,
            downloader,
            state_change_delay,
            sample_rate_change_delay,
        })
    }

    pub fn controls(&self) -> Controls {
        self.controls.clone()
    }

    pub fn status(&self) -> StatusReceiver {
        self.target_status.subscribe()
    }

    pub fn volume(&self) -> VolumeReceiver {
        self.volume.subscribe()
    }

    pub fn position(&self) -> PositionReceiver {
        self.position.subscribe()
    }

    pub fn tracklist(&self) -> TracklistReceiver {
        self.tracklist_tx.subscribe()
    }

    async fn play_pause(&mut self) -> AppResult<()> {
        let target_status = *self.target_status.borrow();

        match target_status {
            Status::Stopped => self.stop(),
            Status::Playing | Status::Buffering => self.pause(),
            Status::Paused => self.play().await?,
        }

        Ok(())
    }

    async fn play(&mut self) -> AppResult<()> {
        self.wait_for_state_change_delay().await;
        let track = self.tracklist_rx.borrow().current_track().cloned();

        if self.sink.is_empty()
            && let Some(current_track) = track
        {
            tracing::info!("Sink is empty. Query track from play");
            self.set_target_status(Status::Buffering);
            self.query_track(&current_track, false).await?;
        } else {
            self.set_target_status(Status::Playing);
            self.sink.play();
        }

        Ok(())
    }

    async fn wait_for_state_change_delay(&self) {
        if let Some(delay) = self.state_change_delay
            && *self.target_status.borrow() == Status::Paused
        {
            self.set_target_status(Status::Buffering);
            tracing::info!("Waiting for state change delay");
            sleep(delay).await;
        }
    }

    fn stop(&mut self) {
        self.set_target_status(Status::Stopped);
        self.sink.stop();
    }

    fn pause(&mut self) {
        self.set_target_status(Status::Paused);
        self.sink.pause();
    }

    fn set_target_status(&self, status: Status) {
        self.target_status.send(status).expect("infallible");
    }

    async fn query_track(&mut self, track: &Track, next_track: bool) -> AppResult<()> {
        tracing::info!(
            "Querying {} track: {}",
            if next_track { "next" } else { "current" },
            &track.title
        );

        if next_track {
            self.next_track_is_queried = true;
        }

        let download_result = self.downloader.ensure_track_is_downloaded(track).await?;

        self.wait_for_state_change_delay().await;

        let query_result = match download_result {
            DownloadResult::Cached(track_path) => self.sink.query_track(&track_path)?,
            DownloadResult::Streaming(reader) => self.sink.query_track_stream(reader)?,
        };

        if next_track {
            self.next_track_in_sink_queue = match query_result {
                QueryTrackResult::Queued => {
                    tracing::info!("In queue");
                    true
                }
                QueryTrackResult::RecreateStreamRequired => {
                    tracing::info!("Not in queue");
                    false
                }
            };
        }
        self.sink.play();
        self.set_target_status(Status::Playing);

        Ok(())
    }

    async fn set_volume(&self, volume: f32) -> AppResult<()> {
        self.volume.send(volume)?;
        self.sink.sync_volume();
        self.database.set_volume(volume).await?;
        Ok(())
    }

    async fn broadcast_tracklist(&self, tracklist: Tracklist) -> AppResult<()> {
        self.database.set_tracklist(&tracklist).await?;
        self.tracklist_tx.send(tracklist)?;
        Ok(())
    }

    fn seek(&mut self, duration: Duration) -> AppResult<()> {
        match self.sink.seek(duration) {
            Ok(()) => {
                self.position.send(self.sink.position())?;
                self.set_target_status(Status::Playing);
            }
            Err(e) => {
                tracing::warn!("Seek to {:?} failed: {e:?}", duration);
            }
        }
        Ok(())
    }

    fn jump_forward(&mut self) -> AppResult<()> {
        let duration = self
            .tracklist_rx
            .borrow()
            .current_track()
            .map(|x| Duration::from_secs(x.duration_seconds as u64));

        if let Some(duration) = duration {
            let ten_seconds = Duration::from_secs(10);
            let next_position = self.sink.position() + ten_seconds;

            if next_position < duration {
                self.seek(next_position)?;
            } else {
                self.seek(duration)?;
            }
        }

        Ok(())
    }

    fn jump_backward(&mut self) -> AppResult<()> {
        let current_position = self.sink.position();

        if current_position.as_millis() < 10000 {
            self.seek(Duration::default())?;
        } else {
            let ten_seconds = Duration::from_secs(10);
            let seek_position = current_position - ten_seconds;

            self.seek(seek_position)?;
        }
        Ok(())
    }

    async fn skip_to_position(&mut self, new_position: i32, force: bool) -> AppResult<()> {
        let mut tracklist = self.tracklist_rx.borrow().clone();
        let current_position = tracklist.current_position();

        // Typical previous skip functionality where if,
        // the track is greater than 1 second into playing,
        // then it goes to the beginning. If triggered again
        // within a second after playing, it will skip to the previous track.
        if !force
            && new_position < current_position as i32
            && self.position.borrow().as_millis() > 1000
        {
            self.seek(Duration::default())?;
            return Ok(());
        }

        self.position.send(Default::default())?;

        if tracklist.skip_to_track(new_position).is_some() {
            self.new_queue(tracklist).await?;
        } else {
            tracklist.reset();
            self.sink.clear()?;
            self.next_track_is_queried = false;
            self.set_target_status(Status::Paused);
            self.position.send(Default::default())?;
            self.broadcast_tracklist(tracklist).await?;
        }

        Ok(())
    }

    async fn next(&mut self) -> AppResult<()> {
        let current_position = self.tracklist_rx.borrow().current_position();
        self.skip_to_position((current_position + 1) as i32, true)
            .await
    }

    async fn previous(&mut self) -> AppResult<()> {
        let current_position = self.tracklist_rx.borrow().current_position();
        self.skip_to_position(current_position as i32 - 1, false)
            .await
    }

    async fn new_queue(&mut self, tracklist: Tracklist) -> AppResult<()> {
        self.sink.clear()?;
        self.next_track_is_queried = false;
        self.next_track_in_sink_queue = false;

        if let Some(first_track) = tracklist.current_track() {
            tracing::info!("New queue starting with: {}", first_track.title);
            self.query_track(first_track, false).await?;
        }

        self.broadcast_tracklist(tracklist).await?;

        Ok(())
    }

    async fn new_track_queue(&mut self, items: Vec<NewQueueItem>, play: bool) -> AppResult<()> {
        self.sink.clear()?;
        self.next_track_is_queried = false;
        self.next_track_in_sink_queue = false;

        let mut queue_items = vec![];
        for (index, item) in items.iter().enumerate() {
            let track = self.client.track(item.track_id).await?;
            let queue_item = QueueItem {
                track,
                queue_id: item.queue_id,
                index,
            };
            queue_items.push(queue_item);
        }

        if let Some(item) = queue_items.first_mut() {
            item.track.status = TrackStatus::Playing;
        }

        let tracklist = Tracklist::new_with_id(TracklistType::Tracks, queue_items);

        if play && let Some(first_track) = tracklist.current_track() {
            tracing::info!("New queue starting with: {}", first_track.title);
            self.query_track(first_track, false).await?;
        }

        self.broadcast_tracklist(tracklist).await?;

        Ok(())
    }

    async fn clear_queue(&mut self) -> AppResult<()> {
        self.pause();
        self.sink.clear()?;
        self.next_track_is_queried = false;
        self.next_track_in_sink_queue = false;

        let tracklist = Tracklist::default();
        self.broadcast_tracklist(tracklist).await?;
        Ok(())
    }

    async fn update_queue(&mut self, tracklist: Tracklist) -> AppResult<()> {
        self.next_track_is_queried = false;
        self.sink.clear_queue()?;
        self.broadcast_tracklist(tracklist).await?;
        Ok(())
    }

    async fn play_track(&mut self, track_id: u32) -> AppResult<()> {
        let mut track: Track = self.client.track(track_id).await?;
        track.status = TrackStatus::Playing;

        let tracklist = Tracklist::new(TracklistType::Tracks, tracks_to_queue_items(vec![track]));

        self.new_queue(tracklist).await
    }

    async fn play_album(&mut self, album_id: &str, index: usize) -> AppResult<()> {
        let album: Album = self.client.album(album_id).await?;

        let unstreamable_tracks_to_index = album
            .tracks
            .iter()
            .take(index)
            .filter(|t| !t.available)
            .count() as i32;

        let mut tracklist = Tracklist::new(
            TracklistType::Album(tracklist::AlbumTracklist {
                title: album.title,
                id: album.id,
                image: Some(album.image),
            }),
            tracks_to_queue_items(album.tracks.into_iter().filter(|t| t.available).collect()),
        );

        tracklist.skip_to_track(index as i32 - unstreamable_tracks_to_index);
        self.new_queue(tracklist).await
    }

    async fn play_top_tracks(&mut self, artist_id: u32, index: usize) -> AppResult<()> {
        let artist = self.client.artist_page(artist_id).await?;
        let tracks = artist.top_tracks;
        let unstreamable_tracks_to_index =
            tracks.iter().take(index).filter(|t| !t.available).count() as i32;

        let mut tracklist = Tracklist::new(
            TracklistType::TopTracks(tracklist::TopTracklist {
                artist_name: artist.name,
                id: artist_id,
                image: artist.image,
            }),
            tracks_to_queue_items(tracks.into_iter().filter(|t| t.available).collect()),
        );

        tracklist.skip_to_track(index as i32 - unstreamable_tracks_to_index);
        self.new_queue(tracklist).await
    }

    async fn play_tracks(&mut self, ids: Vec<u32>, shuffle: bool) -> AppResult<()> {
        let mut tracks: Vec<_> = self
            .client
            .tracks(ids)
            .await?
            .into_iter()
            .filter(|t| t.available)
            .collect();

        if shuffle {
            tracks.shuffle(&mut rand::rng());
        }

        let mut tracklist = Tracklist::new(TracklistType::Tracks, tracks_to_queue_items(tracks));

        tracklist.skip_to_track(0);
        self.new_queue(tracklist).await
    }

    async fn play_playlist(
        &mut self,
        playlist_id: u32,
        index: usize,
        shuffle: bool,
    ) -> AppResult<()> {
        let playlist = self.client.playlist(playlist_id).await?;

        let unstreamable_tracks_to_index = match shuffle {
            true => 0,
            false => playlist
                .tracks
                .iter()
                .take(index)
                .filter(|t| !t.available)
                .count() as i32,
        };

        let mut queue: Vec<QueueItem> = tracks_to_queue_items(
            playlist
                .tracks
                .into_iter()
                .filter(|t| t.available)
                .collect(),
        );

        if shuffle {
            queue.shuffle(&mut rand::rng());
        }

        let mut tracklist = Tracklist::new(
            TracklistType::Playlist(tracklist::PlaylistTracklist {
                title: playlist.title,
                id: playlist.id,
                image: playlist.image,
            }),
            queue,
        );

        tracklist.skip_to_track(index as i32 - unstreamable_tracks_to_index);

        self.new_queue(tracklist).await
    }

    async fn remove_index_from_queue(&mut self, index: usize) -> AppResult<()> {
        let mut tracklist = self.tracklist_rx.borrow().clone();

        tracklist.remove_track(index);
        self.update_queue(tracklist).await?;
        let notification = Notification::Info("Queue updated".into());
        self.broadcast.send(notification);
        Ok(())
    }

    async fn add_tracks_to_queue(&mut self, ids: Vec<u32>) -> AppResult<()> {
        let tracklist = self.tracklist_rx.borrow().clone();
        let last_track_id = tracklist
            .queue()
            .last()
            .map(|x| x.track.id as usize)
            .unwrap_or(0);

        self.insert_tracks_to_queue(ids, last_track_id).await
    }

    async fn insert_tracks_to_queue(&mut self, ids: Vec<u32>, after: usize) -> AppResult<()> {
        let mut tracklist = self.tracklist_rx.borrow().clone();
        tracklist.set_list_type(TracklistType::Tracks);

        let tracks = self.client.tracks(ids).await?;
        let track_titles: Vec<_> = tracks.iter().map(|x| x.title.clone()).collect();
        let track_titles = track_titles.join(", ");

        let notification = Notification::Info(format!("{} added to queue", track_titles));

        for (index, track) in tracks.into_iter().enumerate() {
            tracklist.insert_track(after + index, track);
        }

        self.update_queue(tracklist).await?;
        self.broadcast.send(notification);
        Ok(())
    }

    async fn play_tracks_next(&mut self, ids: Vec<u32>) -> AppResult<()> {
        let mut tracklist = self.tracklist_rx.borrow().clone();
        tracklist.set_list_type(TracklistType::Tracks);

        let mut tracks = self.client.tracks(ids).await?;
        let track_titles: Vec<_> = tracks.iter().map(|x| x.title.clone()).collect();
        let track_titles = track_titles.join(", ");

        let notification = Notification::Info(format!("{} playing next", track_titles));

        let current_index = tracklist.current_position();

        tracks.reverse();
        for track in tracks {
            tracklist.insert_track(current_index + 1, track);
        }

        self.update_queue(tracklist).await?;
        self.broadcast.send(notification);
        Ok(())
    }

    async fn reorder_queue(&mut self, new_order: Vec<usize>) -> AppResult<()> {
        let mut tracklist = self.tracklist_rx.borrow().clone();

        tracklist.reorder_queue(new_order);

        self.update_queue(tracklist).await?;
        let notification = Notification::Info("Queue updated".into());
        self.broadcast.send(notification);
        Ok(())
    }

    async fn tick(&mut self) -> AppResult<()> {
        if *self.target_status.borrow() != Status::Playing {
            return Ok(());
        }

        let position = self.sink.position();
        self.position.send(position)?;

        let duration = self
            .tracklist_rx
            .borrow()
            .current_track()
            .map(|x| x.duration_seconds);

        if let Some(duration) = duration {
            let position = position.as_secs();

            let track_about_to_finish = (duration as i16 - position as i16) < 60;

            if track_about_to_finish && !self.next_track_is_queried {
                tracing::info!("Track about to finish");

                let tracklist = self.tracklist_rx.borrow().clone();

                if let Some(next_track) = tracklist.next_track() {
                    tracing::info!("Query next track: {} from tick", &next_track.title);
                    self.query_track(next_track, true).await?;
                }
            }
        }

        Ok(())
    }

    async fn handle_message(&mut self, notification: ControlCommand) -> AppResult<()> {
        match notification {
            ControlCommand::Album { id, index } => {
                self.play_album(&id, index).await?;
            }
            ControlCommand::Playlist { id, index, shuffle } => {
                self.play_playlist(id, index, shuffle).await?;
            }
            ControlCommand::ArtistTopTracks { artist_id, index } => {
                self.play_top_tracks(artist_id, index).await?;
            }
            ControlCommand::Track { id } => {
                self.play_track(id).await?;
            }
            ControlCommand::Tracks { ids, shuffle } => {
                self.play_tracks(ids, shuffle).await?;
            }
            ControlCommand::Next => {
                self.next().await?;
            }
            ControlCommand::Previous => {
                self.previous().await?;
            }
            ControlCommand::PlayPause => {
                self.play_pause().await?;
            }
            ControlCommand::Play => {
                self.play().await?;
            }
            ControlCommand::Pause => {
                self.pause();
            }
            ControlCommand::SkipToPosition {
                new_position,
                force,
            } => {
                self.skip_to_position(new_position as i32, force).await?;
            }
            ControlCommand::JumpForward => {
                self.jump_forward()?;
            }
            ControlCommand::JumpBackward => {
                self.jump_backward()?;
            }
            ControlCommand::Seek { time } => {
                self.seek(time)?;
            }
            ControlCommand::SetVolume { volume } => {
                self.set_volume(volume).await?;
            }
            ControlCommand::AddTracksToQueue { ids } => self.add_tracks_to_queue(ids).await?,
            ControlCommand::InsertTracksToQueue { ids, after } => {
                self.insert_tracks_to_queue(ids, after).await?
            }
            ControlCommand::RemoveIndexFromQueue { index } => {
                self.remove_index_from_queue(index).await?
            }
            ControlCommand::PlayTracksNext { ids } => self.play_tracks_next(ids).await?,
            ControlCommand::ReorderQueue { new_order } => self.reorder_queue(new_order).await?,
            ControlCommand::NewQueue { items, play } => self.new_track_queue(items, play).await?,
            ControlCommand::ClearQueue => self.clear_queue().await?,
        }
        Ok(())
    }

    async fn track_finished(&mut self) -> AppResult<()> {
        let mut tracklist = self.tracklist_rx.borrow().clone();

        let current_position = tracklist.current_position();
        let new_position = current_position + 1;

        let next_track = tracklist.skip_to_track(new_position as i32);

        match next_track {
            Some(next_track) => {
                if !self.next_track_in_sink_queue {
                    tracing::info!(
                        "Track finished and next track is not in queue. Resetting queue, and querying track."
                    );
                    self.sink.clear()?;
                    if let Some(delay) = self.sample_rate_change_delay {
                        tracing::info!("Waiting for sample rate change delay");
                        sleep(delay).await;
                    }
                    self.query_track(next_track, false).await?;
                }
            }
            None => {
                tracklist.reset();
                self.set_target_status(Status::Paused);
                self.sink.pause();
                self.sink.clear()?;
                self.position.send(Default::default())?;
            }
        }
        self.next_track_is_queried = false;
        self.broadcast_tracklist(tracklist).await?;
        Ok(())
    }

    pub async fn player_loop(&mut self, mut exit_receiver: ExitReceiver) -> AppResult<()> {
        let mut interval = tokio::time::interval(Duration::from_millis(INTERVAL_MS));

        loop {
            select! {
                _ = interval.tick() => {
                    if let Err(err) = self.tick().await {
                        self.broadcast.send_error(err.to_string());
                    };
                }

                Some(notification) = self.controls_rx.recv() => {
                    if let Err(err) = self.handle_message(notification).await {
                        self.broadcast.send_error(err.to_string());
                    };
                }

                Ok(_) = self.track_finished.changed() => {
                    if let Err(err) = self.track_finished().await {
                        self.broadcast.send_error(err.to_string());
                    };
                }

                Ok(exit) = exit_receiver.recv() => {
                    if exit {
                        break Ok(());
                    }
                }
            }
        }
    }
}

fn tracks_to_queue_items(tracks: Vec<Track>) -> Vec<QueueItem> {
    tracks
        .into_iter()
        .enumerate()
        .map(|(i, track)| QueueItem {
            track,
            queue_id: i as u64,
            index: i,
        })
        .collect()
}
