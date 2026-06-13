use controls_module::controls::Controls;
use futures::future::try_join_all;
use player_module::{
    AppResult,
    client::{Client, GenrePlaylistSlug},
    error::Error,
};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::{Block, Borders, ListState, Paragraph},
};

use crate::ui::{SELECTED_STYLE, sidebar};
use crate::{
    app::{FavoriteIds, NotificationList, Output},
    ui::block,
    widgets::{album_list::AlbumList, playlist_list::PlaylistList},
};

pub struct GenresState {
    genres: Vec<GenreItem>,
    selected_genre: usize,
    selected_sub_tab: usize,
    mode: GenresMode,
    focus: GenresFocus,
}

struct GenreItem {
    id: u32,
    name: String,
    albums: Vec<(String, AlbumList)>,
    playlists: Vec<(String, PlaylistList)>,
}

#[derive(PartialEq)]
enum GenresMode {
    GenreList,
    GenreDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum GenresFocus {
    #[default]
    Sidebar,
    Content,
}

impl GenresState {
    pub async fn new(client: &Client) -> AppResult<Self> {
        let genres_list = client.genres().await?;

        let genres = genres_list
            .into_iter()
            .map(|g| GenreItem {
                id: g.id,
                name: g.name,
                albums: vec![],
                playlists: vec![],
            })
            .collect();

        Ok(Self {
            genres,
            selected_genre: 0,
            selected_sub_tab: 0,
            mode: GenresMode::GenreList,
            focus: Default::default(),
        })
    }

    async fn load_genre(&mut self, client: &Client) -> AppResult<()> {
        let genre_id = self.genres[self.selected_genre].id;

        let discover = client.discover_page(Some(genre_id)).await?;

        let playlists = try_join_all(discover.playlists_tags.into_iter().map(|tag| async {
            let playlists = client
                .genre_playlists(GenrePlaylistSlug {
                    genre_id: Some(genre_id),
                    playlist_slug: Some(tag.clone().slug),
                })
                .await?;

            Ok::<_, Error>((tag.name, PlaylistList::new(playlists)))
        }))
        .await?;

        let albums = vec![
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

        self.genres[self.selected_genre].albums = albums;
        self.genres[self.selected_genre].playlists = playlists;

        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, favorite_ids: &FavoriteIds) {
        let block = block(None);
        frame.render_widget(block, area);

        let content_area = area.inner(Margin::new(1, 1));

        match self.mode {
            GenresMode::GenreList => self.render_genre_list(frame, content_area),
            GenresMode::GenreDetail => self.render_genre_detail(frame, content_area, favorite_ids),
        }
    }

    fn render_genre_list(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area);

        let title = Paragraph::new("Select a Genre")
            .style(SELECTED_STYLE)
            .alignment(Alignment::Center);

        frame.render_widget(title, chunks[0]);

        let items_per_row = 2;
        let rows_needed = self.genres.len().div_ceil(items_per_row);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3); rows_needed])
            .split(chunks[1]);

        for (row_idx, row_area) in rows.iter().enumerate() {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(*row_area);

            for col_idx in 0..items_per_row {
                let genre_idx = row_idx * items_per_row + col_idx;

                if let Some(genre) = self.genres.get(genre_idx) {
                    let is_selected = genre_idx == self.selected_genre;

                    let style = if is_selected {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let border_style = if is_selected {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    let widget = Paragraph::new(genre.name.as_str())
                        .style(style)
                        .alignment(Alignment::Center)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .border_style(border_style),
                        );

                    frame.render_widget(widget, cols[col_idx]);
                }
            }
        }
    }

    fn render_genre_detail(&mut self, frame: &mut Frame, area: Rect, favorite_ids: &FavoriteIds) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(1)])
            .split(area);

        let title = format!("← Back | {}", self.genres[self.selected_genre].name);

        let title_widget = Paragraph::new(title)
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Left);

        frame.render_widget(title_widget, chunks[0]);

        let labels = self
            .visible_album_indices()
            .into_iter()
            .map(|i| self.genres[self.selected_genre].albums[i].0.as_str())
            .chain(
                self.genres[self.selected_genre]
                    .playlists
                    .iter()
                    .map(|(label, _)| label.as_str()),
            )
            .collect::<Vec<_>>();

        let (sidebar, sidebar_width) = sidebar(labels, self.focus == GenresFocus::Sidebar);

        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
            .split(chunks[1]);

        let mut sidebar_state = ListState::default();
        sidebar_state.select(Some(self.selected_sub_tab));

        frame.render_stateful_widget(sidebar, content_chunks[0], &mut sidebar_state);

        let content_focused = self.focus == GenresFocus::Content;
        match self.selected_mut() {
            Some(Selected::Album(list)) => {
                list.render(
                    content_chunks[1],
                    frame.buffer_mut(),
                    content_focused,
                    &favorite_ids.albums,
                );
            }
            Some(Selected::Playlist(list)) => {
                list.render(content_chunks[1], frame.buffer_mut(), content_focused);
            }
            None => {}
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
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => match self.mode {
                GenresMode::GenreList => {
                    self.handle_genre_list_events(key_event.code, client).await
                }
                GenresMode::GenreDetail => {
                    self.handle_genre_detail_events(key_event.code, client, controls, notifications)
                        .await
                }
            },
            _ => Ok(Output::NotConsumed),
        }
    }

    async fn handle_genre_list_events(
        &mut self,
        code: KeyCode,
        client: &Client,
    ) -> AppResult<Output> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_genre >= 2 {
                    self.selected_genre -= 2;
                }

                Ok(Output::Consumed)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_genre + 2 < self.genres.len() {
                    self.selected_genre += 2;
                }

                Ok(Output::Consumed)
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.selected_genre > 0 {
                    self.selected_genre -= 1;
                }

                Ok(Output::Consumed)
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.selected_genre + 1 < self.genres.len() {
                    self.selected_genre += 1;
                }

                Ok(Output::Consumed)
            }
            KeyCode::Enter => {
                self.load_genre(client).await?;
                self.mode = GenresMode::GenreDetail;
                self.selected_sub_tab = 0;
                self.focus = GenresFocus::Sidebar;

                Ok(Output::Consumed)
            }
            _ => Ok(Output::NotConsumed),
        }
    }

    async fn handle_genre_detail_events(
        &mut self,
        code: KeyCode,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = GenresMode::GenreList;
                self.focus = GenresFocus::Sidebar;

                Ok(Output::Consumed)
            }
            _ => match self.focus {
                GenresFocus::Sidebar => match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.cycle_subtab_backwards();

                        Ok(Output::Consumed)
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.cycle_subtab();

                        Ok(Output::Consumed)
                    }
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                        self.focus = GenresFocus::Content;

                        Ok(Output::Consumed)
                    }
                    _ => Ok(Output::NotConsumed),
                },
                GenresFocus::Content => match code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        self.focus = GenresFocus::Sidebar;

                        Ok(Output::Consumed)
                    }
                    _ => {
                        self.handle_selected_content_events(code, client, controls, notifications)
                            .await
                    }
                },
            },
        }
    }

    async fn handle_selected_content_events(
        &mut self,
        code: KeyCode,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match self.selected_mut() {
            Some(Selected::Album(list)) => {
                list.handle_events(code, client, controls, notifications)
                    .await
            }
            Some(Selected::Playlist(list)) => {
                list.handle_events(code, client, controls, notifications)
                    .await
            }
            None => Ok(Output::NotConsumed),
        }
    }

    fn visible_album_indices(&self) -> Vec<usize> {
        self.genres[self.selected_genre]
            .albums
            .iter()
            .enumerate()
            .filter(|(_, (_, albums))| !albums.all_items().is_empty())
            .map(|(i, _)| i)
            .collect()
    }

    fn current_subtab(&self) -> Option<SubTab> {
        let album_indices = self.visible_album_indices();

        if self.selected_sub_tab < album_indices.len() {
            return Some(SubTab::Album(album_indices[self.selected_sub_tab]));
        }

        let playlist_index = self.selected_sub_tab.checked_sub(album_indices.len())?;

        if playlist_index < self.genres[self.selected_genre].playlists.len() {
            Some(SubTab::Playlist(playlist_index))
        } else {
            None
        }
    }

    fn selected_mut(&mut self) -> Option<Selected<'_>> {
        match self.current_subtab()? {
            SubTab::Album(index) => Some(Selected::Album(
                &mut self.genres[self.selected_genre].albums[index].1,
            )),
            SubTab::Playlist(index) => Some(Selected::Playlist(
                &mut self.genres[self.selected_genre].playlists[index].1,
            )),
        }
    }

    fn total_tabs(&self) -> usize {
        self.visible_album_indices().len() + self.genres[self.selected_genre].playlists.len()
    }

    fn cycle_subtab(&mut self) {
        let total = self.total_tabs();

        if total == 0 {
            return;
        }

        self.selected_sub_tab = (self.selected_sub_tab + 1) % total;
    }

    fn cycle_subtab_backwards(&mut self) {
        let total = self.total_tabs();

        if total == 0 {
            return;
        }

        self.selected_sub_tab = (self.selected_sub_tab + total - 1) % total;
    }
}

enum Selected<'a> {
    Album(&'a mut AlbumList),
    Playlist(&'a mut PlaylistList),
}

enum SubTab {
    Album(usize),
    Playlist(usize),
}
