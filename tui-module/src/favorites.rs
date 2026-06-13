use controls_module::controls::Controls;
use player_module::{AppResult, client::Client};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::ListState,
};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::{
    app::{FavoriteIds, NotificationList, Output},
    sub_tab::SubTab,
    ui::{block, render_input, sidebar},
    widgets::{
        album_list::AlbumList,
        artist_list::ArtistList,
        playlist_list::PlaylistList,
        track_list::{TrackList, TrackListEvent},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FavoritesFocus {
    #[default]
    Sidebar,
    Content,
}

pub struct FavoritesState {
    pub filter: Input,
    pub albums: AlbumList,
    pub artists: ArtistList,
    pub playlists: PlaylistList,
    pub tracks: TrackList,
    editing: bool,
    sub_tab: SubTab,
    focus: FavoritesFocus,
}

impl FavoritesState {
    pub async fn new(client: &Client) -> AppResult<Self> {
        let favorites = client.favorites().await?;

        Ok(Self {
            editing: Default::default(),
            filter: Default::default(),
            albums: AlbumList::new(favorites.albums),
            artists: ArtistList::new(favorites.artists),
            playlists: PlaylistList::new(
                favorites.playlists.into_iter().map(|x| x.into()).collect(),
            ),
            tracks: TrackList::new(favorites.tracks),
            sub_tab: Default::default(),
            focus: Default::default(),
        })
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, favorite_ids: &FavoriteIds) {
        let tab_content_area_split = Layout::default()
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area);

        render_input(
            &self.filter,
            self.editing,
            tab_content_area_split[0],
            frame,
            "Filter",
        );

        let block = block(None);
        frame.render_widget(block, tab_content_area_split[1]);

        let tab_content_area = tab_content_area_split[1].inner(Margin::new(1, 1));

        let (sidebar, sidebar_width) =
            sidebar(SubTab::labels(), self.focus == FavoritesFocus::Sidebar);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
            .split(tab_content_area);

        let mut sidebar_state = ListState::default();
        sidebar_state.select(Some(self.sub_tab.selected().into()));

        frame.render_stateful_widget(sidebar, chunks[0], &mut sidebar_state);

        let content_focused = self.focus == FavoritesFocus::Content;
        match self.sub_tab {
            SubTab::Albums => self.albums.render(
                chunks[1],
                frame.buffer_mut(),
                content_focused,
                &favorite_ids.albums,
            ),
            SubTab::Artists => self.artists.render(
                chunks[1],
                frame.buffer_mut(),
                content_focused,
                &favorite_ids.artists,
            ),
            SubTab::Playlists => {
                self.playlists
                    .render(chunks[1], frame.buffer_mut(), content_focused)
            }
            SubTab::Tracks => self.tracks.render(
                chunks[1],
                frame.buffer_mut(),
                true,
                content_focused,
                &favorite_ids.tracks,
            ),
        };
    }

    pub async fn handle_events(
        &mut self,
        event: Event,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => match self.editing {
                false => match key_event.code {
                    KeyCode::Char('e') => {
                        self.start_editing();
                        Ok(Output::Consumed)
                    }
                    _ => match self.focus {
                        FavoritesFocus::Sidebar => match key_event.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                self.cycle_subtab_backwards();
                                Ok(Output::Consumed)
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                self.cycle_subtab();
                                Ok(Output::Consumed)
                            }
                            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                                self.focus = FavoritesFocus::Content;
                                Ok(Output::Consumed)
                            }
                            _ => Ok(Output::NotConsumed),
                        },
                        FavoritesFocus::Content => match key_event.code {
                            KeyCode::Left | KeyCode::Char('h') => {
                                self.focus = FavoritesFocus::Sidebar;
                                Ok(Output::Consumed)
                            }
                            _ => {
                                self.handle_content_events(
                                    key_event.code,
                                    client,
                                    controls,
                                    notifications,
                                )
                                .await
                            }
                        },
                    },
                },
                true => match key_event.code {
                    KeyCode::Esc | KeyCode::Enter => {
                        self.stop_editing();
                        Ok(Output::Consumed)
                    }
                    _ => {
                        self.filter.handle_event(&event);

                        let match_in = |s: &str| {
                            s.to_lowercase()
                                .contains(&self.filter.value().to_lowercase())
                        };

                        self.albums.set_filter(
                            self.albums
                                .all_items()
                                .iter()
                                .filter(|album| {
                                    match_in(&album.title) || match_in(&album.artist.name)
                                })
                                .cloned()
                                .collect(),
                        );

                        self.artists.set_filter(
                            self.artists
                                .all_items()
                                .iter()
                                .filter(|artist| match_in(&artist.name))
                                .cloned()
                                .collect(),
                        );

                        self.playlists.set_filter(
                            self.playlists
                                .all_items()
                                .iter()
                                .filter(|playlist| match_in(&playlist.title))
                                .cloned()
                                .collect(),
                        );

                        self.tracks.set_filter(
                            self.tracks
                                .all_items()
                                .iter()
                                .filter(|track| {
                                    match_in(&track.title)
                                        || track
                                            .artist_name
                                            .as_ref()
                                            .is_some_and(|artist| match_in(artist))
                                        || track
                                            .album_title
                                            .as_ref()
                                            .is_some_and(|album| match_in(album))
                                })
                                .cloned()
                                .collect(),
                        );

                        Ok(Output::Consumed)
                    }
                },
            },
            _ => Ok(Output::NotConsumed),
        }
    }

    async fn handle_content_events(
        &mut self,
        key_code: KeyCode,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match self.sub_tab {
            SubTab::Albums => {
                self.albums
                    .handle_events(key_code, client, controls, notifications)
                    .await
            }
            SubTab::Artists => {
                self.artists
                    .handle_events(key_code, client, notifications)
                    .await
            }
            SubTab::Playlists => {
                self.playlists
                    .handle_events(key_code, client, controls, notifications)
                    .await
            }
            SubTab::Tracks => {
                self.tracks
                    .handle_events(
                        key_code,
                        client,
                        controls,
                        notifications,
                        TrackListEvent::Track,
                    )
                    .await
            }
        }
    }

    fn start_editing(&mut self) {
        self.editing = true;
    }

    fn stop_editing(&mut self) {
        self.editing = false;
    }

    fn cycle_subtab_backwards(&mut self) {
        self.sub_tab = self.sub_tab.previous();
    }

    fn cycle_subtab(&mut self) {
        self.sub_tab = self.sub_tab.next();
    }
}
