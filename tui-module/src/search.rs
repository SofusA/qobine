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
    app::{FavoriteIds, NotificationList, Output},
    image_cache::ImageManager,
    sub_tab::SubTab,
    ui::{block, render_input, sidebar},
    widgets::{
        grid::Grid,
        track_list::{TrackList, TrackListEvent},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchFocus {
    #[default]
    Sidebar,
    Content,
    Editing,
}

#[derive(Default)]
pub struct SearchState {
    filter: Input,
    albums: Grid<AlbumSimple>,
    artists: Grid<Artist>,
    playlists: Grid<PlaylistSimple>,
    tracks: TrackList,
    sub_tab: SubTab,
    focus: SearchFocus,
}

impl SearchState {
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        favorites: &FavoriteIds,
        image_cache: &mut ImageManager,
    ) {
        let [input_area, content_area] = Layout::default()
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .areas(area);

        render_input(
            &self.filter,
            self.focus == SearchFocus::Editing,
            input_area,
            frame,
            "Search",
        );

        let block = block(None);
        frame.render_widget(block, content_area);

        let tab_content_area = content_area.inner(Margin::new(1, 1));

        let (sidebar, sidebar_width) = sidebar(
            SubTab::labels().to_vec(),
            self.focus == SearchFocus::Sidebar,
        );

        let [sidebar_area, content_area] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
            .areas(tab_content_area);

        let mut sidebar_state = ListState::default();
        sidebar_state.select(Some(self.sub_tab.selected()));

        frame.render_stateful_widget(sidebar, sidebar_area, &mut sidebar_state);

        let content_focused = self.focus == SearchFocus::Content;

        match self.sub_tab {
            SubTab::Albums => self.albums.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                favorites.albums(),
                image_cache,
            ),
            SubTab::Artists => self.artists.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                favorites.artists(),
                image_cache,
            ),
            SubTab::Playlists => self.playlists.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                favorites.playlists(),
                image_cache,
            ),
            SubTab::Tracks => self.tracks.render(
                content_area,
                frame.buffer_mut(),
                true,
                content_focused,
                favorites.tracks(),
            ),
        }
    }

    pub const fn focus_editing(&mut self) {
        self.focus = SearchFocus::Editing;
    }

    pub async fn handle_events(
        &mut self,
        event: Event,
        client: &StreamClient,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => match self.focus {
                SearchFocus::Editing => match key_event.code {
                    KeyCode::Esc | KeyCode::Enter => {
                        self.focus = SearchFocus::Sidebar;
                        self.update_search(client).await?;
                        Ok(Output::Consumed)
                    }
                    _ => {
                        self.filter.handle_event(&event);
                        Ok(Output::Consumed)
                    }
                },
                SearchFocus::Sidebar => match key_event.code {
                    KeyCode::Char('e') => {
                        self.focus_editing();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.cycle_subtab_backwards();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.cycle_subtab();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        self.focus = SearchFocus::Content;
                        Ok(Output::Consumed)
                    }
                    _ => Ok(Output::NotConsumed),
                },
                SearchFocus::Content => match key_event.code {
                    KeyCode::Char('e') => {
                        self.focus_editing();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Esc => {
                        self.focus = SearchFocus::Sidebar;
                        Ok(Output::Consumed)
                    }
                    _ => {
                        self.handle_content_events(key_event.code, client, controls, notifications)
                            .await
                    }
                },
            },
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

    async fn update_search(&mut self, client: &StreamClient) -> AppResult<()> {
        if !self.filter.value().trim().is_empty() {
            let search_results = client.search(self.filter.value().to_string()).await?;

            self.albums.set_all_items(
                search_results
                    .albums
                    .into_iter()
                    .map(std::convert::Into::into)
                    .collect(),
            );

            self.artists.set_all_items(search_results.artists);

            self.playlists.set_all_items(
                search_results
                    .playlists
                    .into_iter()
                    .map(std::convert::Into::into)
                    .collect(),
            );

            self.tracks.set_all_items(search_results.tracks);
        }

        Ok(())
    }

    fn cycle_subtab_backwards(&mut self) {
        self.sub_tab = self.sub_tab.previous();
    }

    fn cycle_subtab(&mut self) {
        self.sub_tab = self.sub_tab.next();
    }
}
