use controls_module::controls::Controls;
use controls_module::models::{AlbumSimple, PlaylistSimple};
use futures::future::try_join_all;
use player_module::AppResult;
use player_module::client::{GenrePlaylistSlug, StreamClient};
use player_module::error::PlayerError;
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::ListState,
};

use crate::app::FavoriteIds;
use crate::image_cache::ImageManager;
use crate::ui::sidebar;
use crate::widgets::grid::Grid;
use crate::{
    app::{NotificationList, Output},
    ui::block,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DiscoverFocus {
    #[default]
    Sidebar,
    Content,
}

pub struct DiscoverState {
    featured_albums: Vec<(String, Grid<AlbumSimple>)>,
    featured_playlists: Vec<(String, Grid<PlaylistSimple>)>,
    selected_sub_tab: usize,
    focus: DiscoverFocus,
}

impl DiscoverState {
    pub async fn new(client: &StreamClient) -> AppResult<Self> {
        let discover = client.discover_page(None).await?;

        let featured_albums = vec![
            ("New releases".to_string(), Grid::new(discover.new_releases)),
            ("Qobuzissime".to_string(), Grid::new(discover.qobuzissims)),
            (
                "Essential Discography".to_string(),
                Grid::new(discover.ideal_discography),
            ),
            (
                "Album of the week".to_string(),
                Grid::new(discover.album_of_the_week),
            ),
            (
                "Press Accolades".to_string(),
                Grid::new(discover.press_awards),
            ),
            (
                "Most streamed".to_string(),
                Grid::new(discover.most_streamed),
            ),
        ];

        let featured_playlists =
            try_join_all(discover.playlists_tags.into_iter().map(|tag| async {
                let playlists = client
                    .genre_playlists(GenrePlaylistSlug {
                        genre_id: None,
                        playlist_slug: Some(tag.clone().slug),
                    })
                    .await?;

                Ok::<_, PlayerError>((tag.name, Grid::new(playlists)))
            }))
            .await?;

        Ok(Self {
            featured_albums,
            featured_playlists,
            selected_sub_tab: 0,
            focus: DiscoverFocus::default(),
        })
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        favorites: &FavoriteIds,
        image_cache: &mut ImageManager,
    ) {
        let block = block(None);
        frame.render_widget(block, area);

        let tab_content_area = area.inner(Margin::new(1, 1));

        let labels = self
            .featured_albums
            .iter()
            .map(|(label, _)| label.as_str())
            .chain(
                self.featured_playlists
                    .iter()
                    .map(|(label, _)| label.as_str()),
            )
            .collect::<Vec<_>>();

        let (sidebar, sidebar_width) = sidebar(labels, self.focus == DiscoverFocus::Sidebar);

        let [sidebar_area, content_area] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
            .areas(tab_content_area);

        let mut sidebar_state = ListState::default();
        sidebar_state.select(Some(self.selected_sub_tab));

        frame.render_stateful_widget(sidebar, sidebar_area, &mut sidebar_state);

        let content_focused = self.focus == DiscoverFocus::Content;

        if let Some((_, list)) = self.selected_album_mut() {
            list.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                favorites.albums(),
                image_cache,
            );
        } else if let Some((_, list)) = self.selected_playlist_mut() {
            list.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                favorites.playlists(),
                image_cache,
            );
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
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => match self.focus {
                DiscoverFocus::Sidebar => match key_event.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.cycle_subtab_backwards();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.cycle_subtab();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        self.focus = DiscoverFocus::Content;
                        Ok(Output::Consumed)
                    }
                    _ => Ok(Output::NotConsumed),
                },
                DiscoverFocus::Content => match key_event.code {
                    KeyCode::Esc => {
                        self.focus = DiscoverFocus::Sidebar;
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
        if let Some((_, list)) = self.selected_album_mut() {
            return list
                .handle_events(key_code, client, controls, notifications)
                .await;
        }

        if let Some((_, list)) = self.selected_playlist_mut() {
            return list
                .handle_events(key_code, client, controls, notifications)
                .await;
        }

        Ok(Output::NotConsumed)
    }

    fn selected_album_mut(&mut self) -> Option<&mut (String, Grid<AlbumSimple>)> {
        self.featured_albums.get_mut(self.selected_sub_tab)
    }

    fn selected_playlist_mut(&mut self) -> Option<&mut (String, Grid<PlaylistSimple>)> {
        let index = self
            .selected_sub_tab
            .checked_sub(self.featured_albums.len())?;

        self.featured_playlists.get_mut(index)
    }

    fn cycle_subtab_backwards(&mut self) {
        let count = self
            .featured_albums
            .len()
            .saturating_add(self.featured_playlists.len());

        if count == 0 {
            return;
        }

        self.selected_sub_tab = self
            .selected_sub_tab
            .checked_sub(1)
            .unwrap_or_else(|| count.saturating_sub(1))
            .checked_rem(count)
            .unwrap_or(0);
    }

    fn cycle_subtab(&mut self) {
        let count = self
            .featured_albums
            .len()
            .saturating_add(self.featured_playlists.len());

        if count == 0 {
            return;
        }

        self.selected_sub_tab = self
            .selected_sub_tab
            .checked_add(1)
            .and_then(|value| value.checked_rem(count))
            .unwrap_or(0);
    }
}
