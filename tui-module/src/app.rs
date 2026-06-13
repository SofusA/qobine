use crate::{
    discover::DiscoverState,
    favorites::FavoritesState,
    genres::GenresState,
    now_playing::NowPlayingState,
    popup::{Popup, TrackPopupState},
    preferences::PreferencesState,
    queue::QueueState,
    search::SearchState,
    ui::fetch_image,
};
use controls_module::{
    PositionReceiver, Status, StatusReceiver, TracklistReceiver,
    controls::Controls,
    models::{AlbumSimple, Artist, Track},
    tracklist::{Tracklist, TracklistType},
};
use core::fmt;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use disconnect_module::DisconnectClientConfig;
use futures::StreamExt;
use player_module::{
    AppResult,
    client::Client,
    database::Database,
    notification::{Notification, NotificationBroadcast},
};
use ratatui::{DefaultTerminal, widgets::*};
use ratatui_image::protocol::StatefulProtocol;
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

    pub fn tick(&mut self) -> bool {
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

pub struct App {
    pub client: Arc<Client>,
    pub controls: Controls,
    pub database: Arc<Database>,
    pub position: PositionReceiver,
    pub tracklist: TracklistReceiver,
    pub status: StatusReceiver,
    pub current_screen: Tab,
    pub exit: bool,
    pub should_draw: bool,
    pub app_state: AppState,
    pub now_playing: NowPlayingState,
    pub favorites: FavoritesState,
    pub search: SearchState,
    pub queue: QueueState,
    pub discover: DiscoverState,
    pub genres: GenresState,
    pub preferences: PreferencesState,
    pub broadcast: Arc<NotificationBroadcast>,
    pub notifications: NotificationList,
    pub full_screen: bool,
    pub disable_tui_album_cover: bool,
    pub current_image_url: Option<String>,
    pub favorite_ids: FavoriteIds,
    pub connect_available_devices: watch::Receiver<Vec<String>>,
    pub connect_active_device: watch::Receiver<String>,
    pub set_connect_active_device: mpsc::UnboundedSender<String>,
    pub disconnect_client_config_sender: watch::Sender<Option<DisconnectClientConfig>>,
}

#[derive(Default)]
pub struct FavoriteIds {
    pub tracks: HashSet<u32>,
    pub albums: HashSet<String>,
    pub artists: HashSet<u32>,
}

#[derive(Default)]
pub enum AppState {
    #[default]
    Normal,
    Popup(Vec<Popup>),
    Help,
    ConnectPopup(usize),
}

#[allow(clippy::large_enum_variant)]
pub enum Output {
    Consumed,
    NotConsumed,
    UpdateFavorites,
    FavoriteAdded(FavoriteAdd),
    FavoriteRemoved(FavoriteRemove),
    Popup(Popup),
    PopPopupUpdateFavorites,
    AddTrackToPlaylistPopup(Track),
    AddTrackToPlaylistAndPopPopup((u32, u32)), // TODO: Add a type
}

pub enum FavoriteAdd {
    Track(Track),
    Album(AlbumSimple),
    Artist(Artist),
}

pub enum FavoriteRemove {
    Track(u32),
    Album(String),
    Artist(u32),
}

#[derive(Default, PartialEq)]
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
            Tab::Favorites => write!(f, "Favorites"),
            Tab::Search => write!(f, "Search"),
            Tab::Queue => write!(f, "Queue"),
            Tab::Discover => write!(f, "Discover"),
            Tab::Genres => write!(f, "Genres"),
            Tab::Preferences => write!(f, "Preferences"),
        }
    }
}

impl Tab {
    pub const VALUES: [Self; 6] = [
        Tab::Favorites,
        Tab::Search,
        Tab::Queue,
        Tab::Discover,
        Tab::Genres,
        Tab::Preferences,
    ];
}

#[derive(Default)]
pub struct FilteredListState<T> {
    filter: Vec<T>,
    all_items: Vec<T>,
    pub state: TableState,
}

impl<T> FilteredListState<T>
where
    T: Clone,
{
    pub fn new(list: Vec<T>) -> Self {
        Self {
            filter: list.clone(),
            all_items: list,
            state: Default::default(),
        }
    }

    pub fn filter(&self) -> &Vec<T> {
        &self.filter
    }

    pub fn all_items(&self) -> &Vec<T> {
        &self.all_items
    }

    pub fn set_all_items(&mut self, items: Vec<T>) {
        self.all_items = items.clone();
        self.filter = items;
    }

    pub fn set_filter(&mut self, items: Vec<T>) {
        self.filter = items;
    }

    pub fn remove_at_index(&mut self, index: usize) {
        if index >= self.all_items.len() {
            return;
        }

        self.all_items.remove(index);
        self.filter = self.all_items.clone();
    }

    pub fn move_index_to_new_index(&mut self, index: usize, new_index: usize) {
        if index >= self.all_items.len() || new_index >= self.all_items.len() {
            return;
        }

        let item = self.all_items.remove(index);
        self.all_items.insert(new_index, item);

        self.filter = self.all_items.clone();
    }
}

impl App {
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let mut tick_interval = time::interval(Duration::from_millis(100));
        let mut receiver = self.broadcast.subscribe();
        let mut event_stream = EventStream::new();
        let (image_tx, mut image_rx) =
            tokio::sync::mpsc::channel::<Option<(StatefulProtocol, f32)>>(1);

        if let Some(image_url) = self.current_image_url.as_ref()
            && !self.disable_tui_album_cover
        {
            let image = fetch_image(image_url).await;
            self.now_playing.image = image;
        };

        while !self.exit {
            tokio::select! {
                // Prioritize keyboard events by checking them first with biased
                biased;

                Some(event_result) = event_stream.next() => {
                    if let Ok(event) = event_result {
                        self.handle_event(event).await.expect("infallible");
                    }
                }

                Ok(_) = self.position.changed() => {
                    self.now_playing.duration_ms = self.position.borrow_and_update().as_millis() as u32;
                    self.should_draw = true;
                },

                Ok(_) = self.tracklist.changed() => {
                    let tracklist = self.tracklist.borrow_and_update().clone();
                    self.queue.set_items(tracklist
                        .queue()
                        .into_iter()
                        .map(|x| x.track.clone())
                        .collect());
                    let status = self.now_playing.status;
                    let (mut new_state, image_url) = get_current_state_without_image(&tracklist, status);

                    if image_url == self.current_image_url {
                        new_state.image = self.now_playing.image.take();
                    } else if !self.disable_tui_album_cover {
                        if let Some(url) = image_url.clone() {
                            let tx = image_tx.clone();
                            tokio::spawn(async move {
                                let result = fetch_image(&url).await;
                                let _ = tx.send(result).await;
                            });
                        }
                        self.current_image_url = image_url;
                    }

                    self.now_playing = new_state;
                    self.should_draw = true;
                },

                Some(image) = image_rx.recv() => {
                    self.now_playing.image = image;
                    self.should_draw = true;
                }

                Ok(_) = self.status.changed() => {
                    let status = self.status.borrow_and_update();
                    self.now_playing.status = *status;
                    self.should_draw = true;
                }

                _ = tick_interval.tick() => {
                    // Tick is only used for notification cleanup
                }

                notification = receiver.recv() => {
                    if let Ok(notification) = notification {
                        self.notifications.push(notification);
                        self.should_draw = true;
                    }
                }
            }

            if self.notifications.tick() {
                self.should_draw = true;
            };

            if self.should_draw {
                terminal.draw(|frame| self.render(frame))?;
                self.should_draw = false;
            }
        }

        Ok(())
    }

    pub(crate) async fn update_favorites(&mut self) {
        let favorites = self.client.favorites().await;
        let Ok(favorites) = favorites else {
            return;
        };

        self.favorite_ids.tracks = favorites.tracks.iter().map(|t| t.id).collect();
        self.favorite_ids.albums = favorites.albums.iter().map(|a| a.id.clone()).collect();
        self.favorite_ids.artists = favorites.artists.iter().map(|a| a.id).collect();

        self.favorites.albums.set_all_items(favorites.albums);
        self.favorites.artists.set_all_items(favorites.artists);
        self.favorites
            .playlists
            .set_all_items(favorites.playlists.into_iter().map(|x| x.into()).collect());
        self.favorites.tracks.set_all_items(favorites.tracks);
        self.favorites.filter.reset();
    }

    fn apply_favorite_added(&mut self, added: FavoriteAdd) {
        match added {
            FavoriteAdd::Track(track) => {
                self.favorite_ids.tracks.insert(track.id);
                let mut items = self.favorites.tracks.all_items().clone();
                items.insert(0, track);
                self.favorites.tracks.set_all_items(items);
            }
            FavoriteAdd::Album(album) => {
                self.favorite_ids.albums.insert(album.id.clone());
                let mut items = self.favorites.albums.all_items().clone();
                items.push(album);
                items.sort_by(|a, b| {
                    a.artist
                        .name
                        .to_lowercase()
                        .cmp(&b.artist.name.to_lowercase())
                });
                self.favorites.albums.set_all_items(items);
            }
            FavoriteAdd::Artist(artist) => {
                self.favorite_ids.artists.insert(artist.id);
                let mut items = self.favorites.artists.all_items().clone();
                items.push(artist);
                items.sort_by_key(|a| a.name.to_lowercase());
                self.favorites.artists.set_all_items(items);
            }
        }
        self.favorites.filter.reset();
    }

    fn apply_favorite_removed(&mut self, removed: FavoriteRemove) {
        match removed {
            FavoriteRemove::Track(id) => {
                self.favorite_ids.tracks.remove(&id);
                let items: Vec<_> = self
                    .favorites
                    .tracks
                    .all_items()
                    .iter()
                    .filter(|t| t.id != id)
                    .cloned()
                    .collect();
                self.favorites.tracks.set_all_items(items);
            }
            FavoriteRemove::Album(id) => {
                self.favorite_ids.albums.remove(&id);
                let items: Vec<_> = self
                    .favorites
                    .albums
                    .all_items()
                    .iter()
                    .filter(|a| a.id != id)
                    .cloned()
                    .collect();
                self.favorites.albums.set_all_items(items);
            }
            FavoriteRemove::Artist(id) => {
                self.favorite_ids.artists.remove(&id);
                let items: Vec<_> = self
                    .favorites
                    .artists
                    .all_items()
                    .iter()
                    .filter(|a| a.id != id)
                    .cloned()
                    .collect();
                self.favorites.artists.set_all_items(items);
            }
        }
        self.favorites.filter.reset();
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
            Output::FavoriteAdded(added) => {
                self.apply_favorite_added(added);
                self.should_draw = true;
            }
            Output::FavoriteRemoved(removed) => {
                self.apply_favorite_removed(removed);
                self.should_draw = true;
            }
            Output::NotConsumed => match key_code {
                KeyCode::Char('?') => {
                    self.app_state = AppState::Help;
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
                        self.app_state = AppState::ConnectPopup(0);
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
                        let image = fetch_image(&album.image).await;
                        let popup = Popup::AlbumInfo(album, true, image);
                        let mut popups = match std::mem::take(&mut self.app_state) {
                            AppState::Popup(popups) => popups,
                            _ => Vec::new(),
                        };

                        popups.push(popup);
                        self.app_state = AppState::Popup(popups);
                        self.should_draw = true;
                    }
                }
                KeyCode::Char('q') => {
                    self.should_draw = true;
                    self.exit()
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
                    self.full_screen = !self.full_screen;
                    self.should_draw = true;
                }
                _ => {}
            },
            Output::Popup(popup) => {
                let mut popups = match std::mem::take(&mut self.app_state) {
                    AppState::Popup(popups) => popups,
                    _ => Vec::new(),
                };

                popups.push(popup);

                self.app_state = AppState::Popup(popups);
                self.should_draw = true;
            }
            Output::PopPopupUpdateFavorites => {
                if let AppState::Popup(popups) = &mut self.app_state {
                    popups.pop();
                    if popups.is_empty() {
                        self.app_state = AppState::Normal;
                    }
                    self.update_favorites().await;
                    self.should_draw = true;
                }
            }
            Output::AddTrackToPlaylistPopup(track) => {
                let playlists_res = self.client.favorites().await.map(|favs| {
                    favs.playlists
                        .into_iter()
                        .filter(|p| p.is_owned)
                        .map(|x| x.into())
                        .collect::<Vec<_>>()
                });

                if let Ok(playlists) = playlists_res {
                    let mut popups = match std::mem::take(&mut self.app_state) {
                        AppState::Popup(v) => v,
                        other => {
                            self.app_state = other;
                            Vec::new()
                        }
                    };

                    popups.push(Popup::Track(TrackPopupState::new(track, playlists)));

                    self.app_state = AppState::Popup(popups);
                    self.should_draw = true;
                }
            }
            Output::AddTrackToPlaylistAndPopPopup((track_id, playlist_id)) => {
                match self
                    .client
                    .playlist_add_track(playlist_id, &[track_id])
                    .await
                {
                    Ok(_) => {
                        if let AppState::Popup(popups) = &mut self.app_state {
                            popups.pop();
                            if popups.is_empty() {
                                self.app_state = AppState::Normal;
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
                };
                self.should_draw = true;
            }
        }
    }

    async fn handle_event(&mut self, event: Event) -> io::Result<()> {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                match &mut self.app_state {
                    AppState::Help => {
                        self.app_state = AppState::Normal;
                        self.should_draw = true;
                        return Ok(());
                    }
                    AppState::ConnectPopup(selected_device) => {
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
                                };

                                self.app_state = AppState::Normal;
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
                                self.app_state = AppState::Normal;
                            }
                        }
                        self.should_draw = true;
                        return Ok(());
                    }
                    AppState::Popup(popups) => {
                        if key_event.code == KeyCode::Esc {
                            _ = popups.pop();
                            if popups.is_empty() {
                                self.app_state = AppState::Normal;
                            }
                            self.should_draw = true;
                            return Ok(());
                        }

                        let outcome_opt = {
                            if let AppState::Popup(popups) = &mut self.app_state {
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

                        self.handle_output(key_event.code, outcome_opt).await;

                        self.should_draw = true;
                        return Ok(());
                    }
                    _ => {}
                };

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
        };
        Ok(())
    }

    fn navigate_to_favorites(&mut self) {
        self.current_screen = Tab::Favorites;
    }

    fn navigate_to_search(&mut self) {
        self.search.focus_editing();
        self.current_screen = Tab::Search;
    }

    fn navigate_to_queue(&mut self) {
        self.current_screen = Tab::Queue;
    }

    fn navigate_to_discover(&mut self) {
        self.current_screen = Tab::Discover;
    }

    fn navigate_to_genres(&mut self) {
        self.current_screen = Tab::Genres;
    }

    fn navigate_to_preferences(&mut self) {
        self.current_screen = Tab::Preferences;
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

pub fn get_current_state_without_image(
    tracklist: &Tracklist,
    status: Status,
) -> (NowPlayingState, Option<String>) {
    let track = tracklist.current_track().cloned();
    let track_image = track.as_ref().and_then(|track| track.image.as_ref());
    let tracklist_type = tracklist.list_type();

    let (title, image) = match tracklist_type {
        TracklistType::Album(tracklist) => (
            Some(tracklist.title.clone()),
            tracklist.image.as_ref().or(track_image).cloned(),
        ),
        TracklistType::Playlist(tracklist) => (Some(tracklist.title.clone()), track_image.cloned()),
        TracklistType::TopTracks(tracklist) => {
            (Some(tracklist.artist_name.clone()), track_image.cloned())
        }
        TracklistType::Tracks => (
            track.as_ref().map(|x| x.title.clone()),
            track_image.cloned(),
        ),
    };

    let state = NowPlayingState {
        image: None,
        entity_title: title,
        playing_track: track,
        tracklist_length: tracklist.total(),
        status,
        tracklist_position: tracklist.current_position(),
        duration_ms: 0,
    };

    (state, image)
}
