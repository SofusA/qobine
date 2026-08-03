use controls_module::{
    controls::Controls,
    models::{AlbumSimple, PlaylistSimple},
};
use futures::future::try_join_all;
use player_module::{
    AppResult,
    client::{GenrePlaylistSlug, StreamClient},
    error::PlayerError,
};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::{Block, Borders, ListState, Paragraph},
};

use crate::{
    app::FavoriteIds,
    image_cache::ImageManager,
    ui::{SELECTED_STYLE, sidebar},
    widgets::grid::Grid,
};
use crate::{
    app::{NotificationList, Output},
    ui::block,
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
    albums: Vec<(String, Grid<AlbumSimple>)>,
    playlists: Vec<(String, Grid<PlaylistSimple>)>,
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
    pub async fn new(client: &StreamClient) -> AppResult<Self> {
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
            focus: GenresFocus::default(),
        })
    }

    async fn load_genre(&mut self, client: &StreamClient) -> AppResult<()> {
        let Some(genre_id) = self.genres.get(self.selected_genre).map(|genre| genre.id) else {
            return Ok(());
        };

        let discover = client.discover_page(Some(genre_id)).await?;

        let playlists = try_join_all(discover.playlists_tags.into_iter().map(|tag| {
            let slug = tag.slug.clone();

            async {
                let playlists = client
                    .genre_playlists(GenrePlaylistSlug {
                        genre_id: Some(genre_id),
                        playlist_slug: Some(slug),
                    })
                    .await?;

                Ok::<_, PlayerError>((tag.name, Grid::new(playlists)))
            }
        }))
        .await?;

        let albums = vec![
            ("New releases".into(), Grid::new(discover.new_releases)),
            ("Qobuzissime".into(), Grid::new(discover.qobuzissims)),
            (
                "Essential Discography".into(),
                Grid::new(discover.ideal_discography),
            ),
            (
                "Album of the week".into(),
                Grid::new(discover.album_of_the_week),
            ),
            ("Press Accolades".into(), Grid::new(discover.press_awards)),
            ("Most streamed".into(), Grid::new(discover.most_streamed)),
        ];

        if let Some(genre) = self.genres.get_mut(self.selected_genre) {
            genre.albums = albums;
            genre.playlists = playlists;
        }

        Ok(())
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

        let content_area = area.inner(Margin::new(1, 1));

        match self.mode {
            GenresMode::GenreList => self.render_genre_list(frame, content_area),
            GenresMode::GenreDetail => {
                self.render_genre_detail(frame, content_area, favorites, image_cache);
            }
        }
    }

    fn render_genre_list(&self, frame: &mut Frame, area: Rect) {
        let [title_area, content_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .areas(area);

        let title = Paragraph::new("Select a Genre")
            .style(SELECTED_STYLE)
            .alignment(Alignment::Center);

        frame.render_widget(title, title_area);

        let items_per_row = 2;
        let rows_needed = self.genres.len().div_ceil(items_per_row);

        let rows = Layout::vertical(vec![Constraint::Length(3); rows_needed]).split(content_area);

        for (row_idx, (row_area, genres)) in rows
            .iter()
            .zip(self.genres.chunks(items_per_row))
            .enumerate()
        {
            let [left_area, right_area] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(*row_area);

            for (col_idx, (genre, column_area)) in
                genres.iter().zip([left_area, right_area]).enumerate()
            {
                let genre_idx = row_idx * items_per_row + col_idx;
                let is_selected = genre_idx == self.selected_genre;

                let style = Style::default()
                    .fg(if is_selected {
                        Color::Cyan
                    } else {
                        Color::White
                    })
                    .add_modifier(if is_selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    });

                let border_style = Style::default().fg(if is_selected {
                    Color::Cyan
                } else {
                    Color::DarkGray
                });

                let widget = Paragraph::new(genre.name.as_str())
                    .style(style)
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(border_style),
                    );

                frame.render_widget(widget, column_area);
            }
        }
    }

    fn render_genre_detail(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        favorites: &FavoriteIds,
        image_cache: &mut ImageManager,
    ) {
        let [title_area, detail_area] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);

        let Some(genre) = self.genres.get(self.selected_genre) else {
            return;
        };

        let title_widget = Paragraph::new(format!("← Back | {}", genre.name))
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Left);

        frame.render_widget(title_widget, title_area);

        let labels = self
            .visible_album_indices()
            .into_iter()
            .filter_map(|index| genre.albums.get(index))
            .map(|(label, _)| label.as_str())
            .chain(genre.playlists.iter().map(|(label, _)| label.as_str()))
            .collect::<Vec<_>>();

        let (sidebar, sidebar_width) = sidebar(labels, self.focus == GenresFocus::Sidebar);

        let [sidebar_area, content_area] =
            Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(1)])
                .areas(detail_area);

        let mut sidebar_state = ListState::default();
        sidebar_state.select(Some(self.selected_sub_tab));

        frame.render_stateful_widget(sidebar, sidebar_area, &mut sidebar_state);

        let content_focused = self.focus == GenresFocus::Content;

        match self.selected_mut() {
            Some(Selected::Album(list)) => list.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                favorites.albums(),
                image_cache,
            ),
            Some(Selected::Playlist(list)) => list.render(
                content_area,
                frame.buffer_mut(),
                content_focused,
                favorites.playlists(),
                image_cache,
            ),
            None => {}
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
        client: &StreamClient,
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
        client: &StreamClient,
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
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        self.focus = GenresFocus::Content;

                        Ok(Output::Consumed)
                    }
                    _ => Ok(Output::NotConsumed),
                },
                GenresFocus::Content => match code {
                    KeyCode::Esc => {
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
        client: &StreamClient,
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
        self.genres
            .get(self.selected_genre)
            .into_iter()
            .flat_map(|genre| genre.albums.iter())
            .enumerate()
            .filter(|(_, (_, albums))| !albums.all_items().is_empty())
            .map(|(index, _)| index)
            .collect()
    }

    fn current_subtab(&self) -> Option<SubTab> {
        let genre = self.genres.get(self.selected_genre)?;
        let album_indices = self.visible_album_indices();

        if let Some(&album_index) = album_indices.get(self.selected_sub_tab) {
            return Some(SubTab::Album(album_index));
        }

        let playlist_index = self.selected_sub_tab.checked_sub(album_indices.len())?;

        genre
            .playlists
            .get(playlist_index)
            .map(|_| SubTab::Playlist(playlist_index))
    }

    fn selected_mut(&mut self) -> Option<Selected<'_>> {
        let subtab = self.current_subtab()?;
        let genre = self.genres.get_mut(self.selected_genre)?;

        match subtab {
            SubTab::Album(index) => genre
                .albums
                .get_mut(index)
                .map(|(_, grid)| Selected::Album(grid)),

            SubTab::Playlist(index) => genre
                .playlists
                .get_mut(index)
                .map(|(_, grid)| Selected::Playlist(grid)),
        }
    }

    fn total_tabs(&self) -> usize {
        let playlist_count = self
            .genres
            .get(self.selected_genre)
            .map_or(0, |genre| genre.playlists.len());

        self.visible_album_indices()
            .len()
            .saturating_add(playlist_count)
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
    Album(&'a mut Grid<AlbumSimple>),
    Playlist(&'a mut Grid<PlaylistSimple>),
}

enum SubTab {
    Album(usize),
    Playlist(usize),
}
