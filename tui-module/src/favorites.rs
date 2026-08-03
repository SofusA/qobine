use std::collections::HashSet;

use controls_module::{
    controls::Controls,
    models::{AlbumSimple, Artist, PlaylistSimple},
};
use player_module::{AppResult, client::StreamClient};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::ListState,
};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::{
    app::{NotificationList, Output},
    image_cache::ImageManager,
    sub_tab::SubTab,
    ui::{block, render_input, sidebar},
    widgets::{
        grid::Grid,
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
    filter: Input,
    pub albums: Grid<AlbumSimple>,
    pub artists: Grid<Artist>,
    pub playlists: Grid<PlaylistSimple>,
    pub tracks: TrackList,
    editing: bool,
    sub_tab: SubTab,
    focus: FavoritesFocus,
}

impl FavoritesState {
    pub async fn new(client: &StreamClient) -> AppResult<Self> {
        let favorites = client.favorites().await?;

        Ok(Self {
            editing: bool::default(),
            filter: Input::default(),
            albums: Grid::new(favorites.albums),
            artists: Grid::new(favorites.artists),
            playlists: Grid::new(
                favorites
                    .playlists
                    .into_iter()
                    .map(std::convert::Into::into)
                    .collect(),
            ),
            tracks: TrackList::new(favorites.tracks),
            sub_tab: SubTab::default(),
            focus: FavoritesFocus::default(),
        })
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, image_cache: &mut ImageManager) {
        let [input_area, content_area] = Layout::default()
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .areas(area);

        render_input(&self.filter, self.editing, input_area, frame, "Filter");

        let block = block(None);
        frame.render_widget(block, content_area);

        let (sidebar, sidebar_width) = sidebar(
            SubTab::labels().to_vec(),
            self.focus == FavoritesFocus::Sidebar,
        );

        let content_area = content_area.inner(Margin::new(1, 1));
        let [sidebar_area, content_area] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
            .areas(content_area);

        let mut sidebar_state = ListState::default();
        sidebar_state.select(Some(self.sub_tab.selected()));

        frame.render_stateful_widget(sidebar, sidebar_area, &mut sidebar_state);

        let content_focused = self.focus == FavoritesFocus::Content;
        match self.sub_tab {
            SubTab::Albums => self.albums.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                &HashSet::default(),
                image_cache,
            ),
            SubTab::Artists => self.artists.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                &HashSet::default(),
                image_cache,
            ),
            SubTab::Playlists => self.playlists.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                &HashSet::default(),
                image_cache,
            ),
            SubTab::Tracks => self.tracks.render(
                content_area,
                frame.buffer_mut(),
                true,
                content_focused,
                &HashSet::default(),
            ),
        }
    }

    pub async fn handle_events(
        &mut self,
        event: Event,
        client: &StreamClient,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                if self.editing {
                    match key_event.code {
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
                    }
                } else {
                    match key_event.code {
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
                                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                                    self.focus = FavoritesFocus::Content;
                                    Ok(Output::Consumed)
                                }
                                _ => Ok(Output::NotConsumed),
                            },
                            FavoritesFocus::Content => match key_event.code {
                                KeyCode::Esc => {
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
                    }
                }
            }
            _ => Ok(Output::NotConsumed),
        }
    }

    async fn handle_content_events(
        &mut self,
        key_code: KeyCode,
        client: &StreamClient,
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
                    .handle_events(key_code, client, controls, notifications)
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

    const fn start_editing(&mut self) {
        self.editing = true;
    }

    const fn stop_editing(&mut self) {
        self.editing = false;
    }

    fn cycle_subtab_backwards(&mut self) {
        self.sub_tab = self.sub_tab.previous();
    }

    fn cycle_subtab(&mut self) {
        self.sub_tab = self.sub_tab.next();
    }
}
