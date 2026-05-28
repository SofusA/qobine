use futures::future::try_join_all;
use qobuz_player_controls::client::{Client, GenrePlaylistSlug};
use qobuz_player_controls::error::Error;
use qobuz_player_controls::{AppResult, controls::Controls};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
};

use crate::{
    app::{NotificationList, Output},
    ui::{block, tab_bar},
    widgets::{album_list::AlbumList, playlist_list::PlaylistList},
};

pub struct DiscoverState {
    featured_albums: Vec<(String, AlbumList)>,
    featured_playlists: Vec<(String, PlaylistList)>,
    selected_sub_tab: usize,
}

impl DiscoverState {
    pub async fn new(client: &Client) -> AppResult<Self> {
        let discover = client.discover_page(None).await?;
        let featured_albums = vec![
            (
                "New releases".to_string(),
                AlbumList::new(discover.new_releases),
            ),
            (
                "Qobuzissime".to_string(),
                AlbumList::new(discover.qobuzissims),
            ),
            (
                "Essential Discography".to_string(),
                AlbumList::new(discover.ideal_discography),
            ),
            (
                "Album of the week".to_string(),
                AlbumList::new(discover.album_of_the_week),
            ),
            (
                "Press Accolades".to_string(),
                AlbumList::new(discover.press_awards),
            ),
            (
                "Most streamed".to_string(),
                AlbumList::new(discover.most_streamed),
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

                Ok::<_, Error>((tag.name, PlaylistList::new(playlists)))
            }))
            .await?;

        Ok(Self {
            featured_albums,
            featured_playlists,
            selected_sub_tab: 0,
        })
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let block = block(None);
        frame.render_widget(block, area);

        let tab_content_area = area.inner(Margin::new(1, 1));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(1)])
            .split(tab_content_area);

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

        let tabs = tab_bar(labels, self.selected_sub_tab);
        frame.render_widget(tabs, chunks[0]);

        if let Some((_, list)) = self.selected_album_mut() {
            list.render(chunks[1], frame.buffer_mut());
        } else if let Some((_, list)) = self.selected_playlist_mut() {
            list.render(chunks[1], frame.buffer_mut());
        }
    }

    pub async fn handle_events(
        &mut self,
        event: Event,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                match key_event.code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        self.cycle_subtab_backwards();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        self.cycle_subtab();
                        Ok(Output::Consumed)
                    }
                    _ => {
                        if let Some((_, list)) = self.selected_album_mut() {
                            return list
                                .handle_events(key_event.code, client, controls, notifications)
                                .await;
                        }

                        if let Some((_, list)) = self.selected_playlist_mut() {
                            return list
                                .handle_events(key_event.code, client, controls, notifications)
                                .await;
                        }

                        Ok(Output::NotConsumed)
                    }
                }
            }
            _ => Ok(Output::NotConsumed),
        }
    }

    fn selected_album_mut(&mut self) -> Option<&mut (String, AlbumList)> {
        self.featured_albums.get_mut(self.selected_sub_tab)
    }

    fn selected_playlist_mut(&mut self) -> Option<&mut (String, PlaylistList)> {
        let index = self
            .selected_sub_tab
            .checked_sub(self.featured_albums.len())?;

        self.featured_playlists.get_mut(index)
    }

    fn cycle_subtab_backwards(&mut self) {
        let count = self.featured_albums.len() + self.featured_playlists.len();

        if count == 0 {
            return;
        }

        self.selected_sub_tab = (self.selected_sub_tab + count - 1) % count;
    }

    fn cycle_subtab(&mut self) {
        let count = self.featured_albums.len() + self.featured_playlists.len();

        if count == 0 {
            return;
        }

        self.selected_sub_tab = (self.selected_sub_tab + 1) % count;
    }
}
