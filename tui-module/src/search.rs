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
pub enum SearchFocus {
    #[default]
    Sidebar,
    Content,
    Editing,
}

#[derive(Default)]
pub struct SearchState {
    filter: Input,
    albums: AlbumList,
    artists: ArtistList,
    playlists: PlaylistList,
    tracks: TrackList,
    sub_tab: SubTab,
    focus: SearchFocus,
}

impl SearchState {
    pub fn render(&mut self, frame: &mut Frame, area: Rect, favorite_ids: &FavoriteIds) {
        let tab_content_area_split = Layout::default()
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area);

        render_input(
            &self.filter,
            self.focus == SearchFocus::Editing,
            tab_content_area_split[0],
            frame,
            "Search",
        );

        let block = block(None);
        frame.render_widget(block, tab_content_area_split[1]);

        let tab_content_area = tab_content_area_split[1].inner(Margin::new(1, 1));

        let (sidebar, sidebar_width) =
            sidebar(SubTab::labels(), self.focus == SearchFocus::Sidebar);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
            .split(tab_content_area);

        let mut sidebar_state = ListState::default();
        sidebar_state.select(Some(self.sub_tab.selected().into()));

        frame.render_stateful_widget(sidebar, chunks[0], &mut sidebar_state);

        let content_focused = self.focus == SearchFocus::Content;

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

    pub fn focus_editing(&mut self) {
        self.focus = SearchFocus::Editing;
    }

    pub async fn handle_events(
        &mut self,
        event: Event,
        client: &Client,
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
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
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
                    KeyCode::Left | KeyCode::Char('h') => {
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

    async fn update_search(&mut self, client: &Client) -> AppResult<()> {
        if !self.filter.value().trim().is_empty() {
            let search_results = client.search(self.filter.value().to_string()).await?;

            self.albums.set_all_items(
                search_results
                    .albums
                    .into_iter()
                    .map(|x| x.into())
                    .collect(),
            );

            self.artists.set_all_items(search_results.artists);

            self.playlists.set_all_items(
                search_results
                    .playlists
                    .into_iter()
                    .map(|x| x.into())
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
