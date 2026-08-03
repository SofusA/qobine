use crate::{
    detail_pages::{AddTrackOverlay, AlbumOverlay, ArtistOverlay, Overlay, TrackInfoOverlay},
    discover::DiscoverState,
    favorites::FavoritesState,
    genres::GenresState,
    image_cache::{ImageLoaded, ImageManager},
    now_playing::NowPlayingState,
    preferences::PreferencesState,
    queue::QueueState,
    search::SearchState,
};
use controls_module::{
    PositionReceiver, Status, StatusReceiver, TracklistReceiver,
    controls::Controls,
    models::{Artist, Track},
    tracklist::{Tracklist, TracklistType},
};
use core::fmt;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use disconnect_module::DisconnectClientConfig;
use futures::StreamExt;
use num_traits::ToPrimitive;
use player_module::{
    AppResult,
    client::StreamClient,
    database::Database,
    notification::{Notification, NotificationBroadcast},
};
use ratatui::{DefaultTerminal, widgets::Clear};
use std::{collections::HashSet, io, sync::Arc, time::Instant};
use tokio::{
    sync::{mpsc, watch},
    time::{self, Duration},
};

#[derive(Default)]
pub struct NotificationList {
    notifications: Vec<(Notification, Instant)>,
}

impl NotificationList {
    pub fn push(&mut self, notification: Notification) {
        self.notifications.push((notification, Instant::now()));
    }

    fn tick(&mut self) -> bool {
        let notifications_before_clean = self.notifications.len();
        self.notifications
            .retain(|notification| notification.1.elapsed() < Duration::from_secs(5));
        let notifications_after_clean = self.notifications.len();

        notifications_before_clean != notifications_after_clean
    }

    pub fn notifications(&self) -> Vec<&Notification> {
        self.notifications.iter().map(|x| &x.0).collect()
    }
}

pub struct FavoriteIds {
    albums: HashSet<String>,
    artists: HashSet<u32>,
    playlists: HashSet<u32>,
    tracks: HashSet<u32>,
}

impl FavoriteIds {
    pub const fn albums(&self) -> &HashSet<String> {
        &self.albums
    }

    pub const fn artists(&self) -> &HashSet<u32> {
        &self.artists
    }

    pub const fn playlists(&self) -> &HashSet<u32> {
        &self.playlists
    }

    pub const fn tracks(&self) -> &HashSet<u32> {
        &self.tracks
    }
}

#[allow(clippy::large_enum_variant)]
pub enum Output {
    Consumed,
    NotConsumed,
    UpdateFavorites,
    Overlay(Overlay),
    PopOverlay,
    PopOverlayUpdateFavorites,
    AddTrackToPlaylistOverlay(Track),
    AddTrackToPlaylistAndPopOverlay((u32, u32)),
}

#[derive(Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Favorites,
    Search,
    Queue,
    Discover,
    Genres,
    Preferences,
}

impl fmt::Display for Tab {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Favorites => write!(f, "Favorites"),
            Self::Search => write!(f, "Search"),
            Self::Queue => write!(f, "Queue"),
            Self::Discover => write!(f, "Discover"),
            Self::Genres => write!(f, "Genres"),
            Self::Preferences => write!(f, "Preferences"),
        }
    }
}

impl Tab {
    pub const VALUES: [Self; 6] = [
        Self::Favorites,
        Self::Search,
        Self::Queue,
        Self::Discover,
        Self::Genres,
        Self::Preferences,
    ];
}

pub struct App {
    pub client: Arc<StreamClient>,
    pub image_cache: ImageManager,
    pub image_rx: mpsc::UnboundedReceiver<ImageLoaded>,
    pub controls: Controls,
    pub database: Arc<Database>,
    pub position: PositionReceiver,
    pub tracklist: TracklistReceiver,
    pub status: StatusReceiver,
    pub current_screen: Tab,
    pub exit: bool,
    pub should_draw: bool,
    pub should_clear: bool,
    pub state: AppState,
    pub now_playing: NowPlayingState,
    pub favorites: FavoritesState,
    pub favorite_ids: FavoriteIds,
    pub search: SearchState,
    pub queue: QueueState,
    pub discover: DiscoverState,
    pub genres: GenresState,
    pub preferences: PreferencesState,
    pub broadcast: Arc<NotificationBroadcast>,
    pub notifications: NotificationList,
    pub connect_available_devices: watch::Receiver<Vec<String>>,
    pub connect_active_device: watch::Receiver<String>,
    pub set_connect_active_device: mpsc::UnboundedSender<String>,
    pub disconnect_client_config_sender: watch::Sender<Option<DisconnectClientConfig>>,
}

#[derive(Default)]
pub enum AppState {
    #[default]
    Normal,
    Overlay(Vec<Overlay>),
    Help,
    ConnectOverlay(usize),
    Focus,
}

impl App {
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let mut notification_tick_interval = time::interval(Duration::from_millis(2000));
        let mut receiver = self.broadcast.subscribe();
        let mut event_stream = EventStream::new();

        let tracklist = self.tracklist.borrow().clone();
        self.now_playing = create_now_playing_state(&tracklist, self.now_playing.status);

        while !self.exit {
            tokio::select! {
                // Prioritize keyboard events by checking them first with biased
                biased;

                Some(event_result) = event_stream.next() => {
                    if let Ok(event) = event_result {
                        _ = self.handle_event(event).await;
                    }
                }

                Ok(()) = self.position.changed() => {
                    self.now_playing.duration_ms = self.position.borrow_and_update().as_millis().to_u32().unwrap_or_default();
                    self.should_draw = true;
                },

                Ok(()) = self.tracklist.changed() => {
                    let tracklist = self.tracklist.borrow_and_update().clone();

                    self.queue.set_items(
                        tracklist
                            .queue()
                            .into_iter()
                            .map(|item| item.track.clone())
                            .collect(),
                    );

                    let status = self.now_playing.status;
                    let new_state = create_now_playing_state(&tracklist, status);

                    self.now_playing = new_state;
                    self.should_draw = true;
                },

                Some(message) = self.image_rx.recv() => {
                    self.image_cache.insert(message);
                    self.should_draw = true;
                }

                Ok(()) = self.status.changed() => {
                    let status = self.status.borrow_and_update();
                    self.now_playing.status = *status;
                    self.should_draw = true;
                }

                _ = notification_tick_interval.tick() => {
                    if self.notifications.tick() {
                        self.should_draw = true;
                        self.should_clear = true;
                    }
                }

                notification = receiver.recv() => {
                    if let Ok(notification) = notification {
                        self.notifications.push(notification);
                        self.should_draw = true;
                    }
                }
            }

            if self.should_clear {
                terminal.draw(|frame| frame.render_widget(Clear, frame.area()))?;
                self.should_clear = false;
            }

            if self.should_draw {
                terminal.draw(|frame| self.render(frame))?;
                self.should_draw = false;
            }
        }

        Ok(())
    }

    pub(crate) async fn update_favorites(&mut self) {
        let favorites = FavoritesState::new(&self.client).await;
        if let Ok(favorites) = favorites {
            self.favorite_ids = build_favorite_ids(&favorites);
            self.favorites = favorites;
        }
    }

    fn handle_focus_event(&mut self, key_code: KeyCode) -> Output {
        match key_code {
            KeyCode::Esc | KeyCode::Char('F') => {
                self.state = AppState::Normal;
                Output::Consumed
            }
            KeyCode::Char(' ') => {
                self.controls.play_pause();
                Output::Consumed
            }
            _ => Output::NotConsumed,
        }
    }

    fn push_popup(&mut self, popup: Overlay) {
        let mut popups = match std::mem::take(&mut self.state) {
            AppState::Overlay(popups) => popups,
            _ => Vec::new(),
        };

        popups.push(popup);
        self.state = AppState::Overlay(popups);
        self.should_draw = true;
    }

    async fn handle_output(&mut self, key_code: KeyCode, output: AppResult<Output>) {
        let output = match output {
            Ok(res) => res,
            Err(err) => {
                self.notifications
                    .push(Notification::Error(err.to_string()));
                return;
            }
        };

        match output {
            Output::Consumed => {
                self.should_draw = true;
            }
            Output::UpdateFavorites => {
                self.update_favorites().await;
                self.should_draw = true;
            }
            Output::NotConsumed => match key_code {
                KeyCode::Char('?') => {
                    self.state = AppState::Help;
                    self.should_draw = true;
                }
                KeyCode::Char('X') => {
                    self.controls.clear_queue();
                    self.should_draw = true;
                }
                KeyCode::Char('c') => {
                    let enable_connect = self
                        .database
                        .get_configuration()
                        .await
                        .map(|x| x.enable_disconnect)
                        .unwrap_or(false);

                    if enable_connect {
                        self.state = AppState::ConnectOverlay(0);
                        self.should_draw = true;
                    }
                }
                KeyCode::Char('I') => {
                    if let Some(album_id) = self
                        .now_playing
                        .playing_track
                        .as_ref()
                        .and_then(|t| t.album_id.clone())
                        && let Ok(album) = self.client.album(&album_id).await
                    {
                        let popup = Overlay::Album(AlbumOverlay::new(album, &self.client).await);
                        self.push_popup(popup);
                    }
                }
                KeyCode::Char('G') => {
                    if let Some(track) = self.now_playing.playing_track.as_ref()
                        && let Some(artist_id) = track.artist_id
                    {
                        let artist = Artist {
                            id: artist_id,
                            name: track.artist_name.clone().unwrap_or_default(),
                            image: None,
                        };

                        if let Ok(state) = ArtistOverlay::new(&artist, &self.client).await {
                            self.push_popup(Overlay::Artist(state));
                        }
                    }
                }
                KeyCode::Char('i') => {
                    if let Some(id) = self.now_playing.playing_track.as_ref().map(|t| t.id)
                        && let Ok(track) = self.client.track(id).await
                    {
                        let state = TrackInfoOverlay::new(track);
                        self.push_popup(Overlay::TrackInfo(state));
                    }
                }
                KeyCode::Char('q') => {
                    self.should_draw = true;
                    self.exit();
                }
                KeyCode::Char('1') => {
                    self.navigate_to_favorites();
                    self.should_draw = true;
                }
                KeyCode::Char('2') => {
                    self.navigate_to_search();
                    self.should_draw = true;
                }
                KeyCode::Char('3') => {
                    self.navigate_to_queue();
                    self.should_draw = true;
                }
                KeyCode::Char('4') => {
                    self.navigate_to_discover();
                    self.should_draw = true;
                }
                KeyCode::Char('5') => {
                    self.navigate_to_genres();
                    self.should_draw = true;
                }
                KeyCode::Char('6') => {
                    self.navigate_to_preferences();
                    self.should_draw = true;
                }
                KeyCode::Char(' ') => {
                    self.controls.play_pause();
                    self.should_draw = true;
                }
                KeyCode::Char('n') => {
                    self.controls.next();
                    self.should_draw = true;
                }
                KeyCode::Char('p') => {
                    self.controls.previous();
                    self.should_draw = true;
                }
                KeyCode::Char('f') => {
                    self.controls.jump_forward();
                    self.should_draw = true;
                }
                KeyCode::Char('b') => {
                    self.controls.jump_backward();
                    self.should_draw = true;
                }
                KeyCode::Char('F') => {
                    self.state = AppState::Focus;
                    self.should_draw = true;
                }
                _ => {}
            },
            Output::Overlay(popup) => {
                self.push_popup(popup);
            }
            Output::PopOverlay => {
                if let AppState::Overlay(popups) = &mut self.state {
                    popups.pop();
                    if popups.is_empty() {
                        self.state = AppState::Normal;
                    }
                    self.should_draw = true;
                }
            }
            Output::PopOverlayUpdateFavorites => {
                if let AppState::Overlay(popups) = &mut self.state {
                    popups.pop();
                    if popups.is_empty() {
                        self.state = AppState::Normal;
                    }
                    self.update_favorites().await;
                    self.should_draw = true;
                }
            }
            Output::AddTrackToPlaylistOverlay(track) => {
                let playlists = self
                    .favorites
                    .playlists
                    .all_items()
                    .iter()
                    .filter(|p| p.is_owned)
                    .cloned()
                    .collect();

                let mut popups = match std::mem::take(&mut self.state) {
                    AppState::Overlay(v) => v,
                    other => {
                        self.state = other;
                        Vec::new()
                    }
                };

                popups.push(Overlay::AddTrackToPlaylist(AddTrackOverlay::new(
                    track, playlists,
                )));

                self.state = AppState::Overlay(popups);
                self.should_draw = true;
            }
            Output::AddTrackToPlaylistAndPopOverlay((track_id, playlist_id)) => {
                match self
                    .client
                    .playlist_add_track(playlist_id, &[track_id])
                    .await
                {
                    Ok(_) => {
                        if let AppState::Overlay(popups) = &mut self.state {
                            popups.pop();
                            if popups.is_empty() {
                                self.state = AppState::Normal;
                            }
                            self.update_favorites().await;
                        }
                        self.notifications
                            .push(Notification::Info("Added to playlist".into())); // Add track and playlist name
                    }
                    Err(err) => {
                        self.notifications
                            .push(Notification::Error(err.to_string()));
                    }
                }
                self.should_draw = true;
            }
        }
    }

    async fn handle_event(&mut self, event: Event) -> io::Result<()> {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                match &mut self.state {
                    AppState::Help => {
                        self.state = AppState::Normal;
                        self.should_draw = true;
                        self.should_clear = true;
                        return Ok(());
                    }
                    AppState::Focus => {
                        let output = self.handle_focus_event(key_event.code);
                        self.handle_output(key_event.code, Ok(output)).await;
                        self.should_draw = true;
                        return Ok(());
                    }
                    AppState::ConnectOverlay(selected_device) => {
                        match key_event.code {
                            KeyCode::Enter => {
                                let available_devices = self.connect_available_devices.borrow();
                                let selected_device_string =
                                    available_devices.get(*selected_device);

                                if let Some(selected_device_string) = selected_device_string
                                    && let Err(err) = self
                                        .set_connect_active_device
                                        .send(selected_device_string.clone())
                                {
                                    self.broadcast
                                        .send_error(format!("Unable to select device: {err}"));
                                }

                                self.state = AppState::Normal;
                            }
                            KeyCode::Left | KeyCode::Up => {
                                if 0 < *selected_device {
                                    *selected_device = selected_device.saturating_sub(1);
                                }
                            }
                            KeyCode::Right | KeyCode::Down => {
                                let available_devices =
                                    self.connect_available_devices.borrow().len();

                                if *selected_device < available_devices - 1 {
                                    *selected_device = selected_device.saturating_add(1);
                                }
                            }
                            _ => {
                                self.state = AppState::Normal;
                            }
                        }
                        self.should_draw = true;
                        return Ok(());
                    }
                    AppState::Overlay(_) => {
                        let outcome = {
                            if let AppState::Overlay(popups) = &mut self.state {
                                if let Some(popup) = popups.last_mut() {
                                    popup
                                        .handle_event(
                                            event,
                                            &self.client,
                                            &self.controls,
                                            &mut self.notifications,
                                        )
                                        .await
                                } else {
                                    Ok(Output::NotConsumed)
                                }
                            } else {
                                Ok(Output::NotConsumed)
                            }
                        };

                        self.handle_output(key_event.code, outcome).await;
                        return Ok(());
                    }

                    AppState::Normal => {}
                }

                let screen_output = match self.current_screen {
                    Tab::Favorites => {
                        self.favorites
                            .handle_events(
                                event,
                                &self.client,
                                &self.controls,
                                &mut self.notifications,
                            )
                            .await
                    }
                    Tab::Search => {
                        self.search
                            .handle_events(
                                event,
                                &self.client,
                                &self.controls,
                                &mut self.notifications,
                            )
                            .await
                    }
                    Tab::Queue => {
                        self.queue
                            .handle_events(
                                event,
                                &self.client,
                                &self.controls,
                                &mut self.notifications,
                            )
                            .await
                    }
                    Tab::Discover => {
                        self.discover
                            .handle_events(
                                event,
                                &self.client,
                                &self.controls,
                                &mut self.notifications,
                            )
                            .await
                    }
                    Tab::Genres => {
                        self.genres
                            .handle_events(
                                event,
                                &self.client,
                                &self.controls,
                                &mut self.notifications,
                            )
                            .await
                    }
                    Tab::Preferences => Ok(self
                        .preferences
                        .handle_events(
                            event,
                            &self.controls,
                            &self.database,
                            &self.disconnect_client_config_sender,
                        )
                        .await),
                };

                self.handle_output(key_event.code, screen_output).await;
            }

            Event::Resize(_, _) => self.should_draw = true,
            _ => {}
        }
        Ok(())
    }

    const fn navigate_to_favorites(&mut self) {
        self.current_screen = Tab::Favorites;
    }

    const fn navigate_to_search(&mut self) {
        self.search.focus_editing();
        self.current_screen = Tab::Search;
    }

    const fn navigate_to_queue(&mut self) {
        self.current_screen = Tab::Queue;
    }

    const fn navigate_to_discover(&mut self) {
        self.current_screen = Tab::Discover;
    }

    const fn navigate_to_genres(&mut self) {
        self.current_screen = Tab::Genres;
    }

    const fn navigate_to_preferences(&mut self) {
        self.current_screen = Tab::Preferences;
    }

    const fn exit(&mut self) {
        self.exit = true;
    }
}

pub fn create_now_playing_state(tracklist: &Tracklist, status: Status) -> NowPlayingState {
    let track = tracklist.current_track().cloned();
    let tracklist_type = tracklist.list_type();

    let title = match tracklist_type {
        TracklistType::Album(tracklist) => Some(tracklist.title.clone()),
        TracklistType::Playlist(tracklist) => Some(tracklist.title.clone()),
        TracklistType::TopTracks(tracklist) => Some(tracklist.artist_name.clone()),
        TracklistType::Tracks => track.as_ref().and_then(|x| x.album_title.clone()),
    };

    NowPlayingState {
        entity_title: title,
        playing_track: track,
        tracklist_length: tracklist.total(),
        status,
        tracklist_position: tracklist.current_position(),
        duration_ms: 0,
    }
}

pub fn build_favorite_ids(favorite_state: &FavoritesState) -> FavoriteIds {
    let albums = favorite_state
        .albums
        .all_items()
        .iter()
        .map(|x| x.id.clone())
        .collect();

    let artists = favorite_state
        .artists
        .all_items()
        .iter()
        .map(|x| x.id)
        .collect();

    let playlists = favorite_state
        .playlists
        .all_items()
        .iter()
        .map(|x| x.id)
        .collect();

    let tracks = favorite_state
        .tracks
        .all_items()
        .iter()
        .map(|x| x.id)
        .collect();

    FavoriteIds {
        albums,
        artists,
        playlists,
        tracks,
    }
}
