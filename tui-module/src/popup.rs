use controls_module::{
    controls::Controls,
    models::{Album, AlbumSimple, Artist, Playlist, PlaylistSimple, Track},
};
use player_module::{AppResult, client::Client};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::*,
};
use ratatui_image::StatefulImage;
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::{
    app::{FavoriteIds, NotificationList, Output},
    image_cache::{AppImage, ImageManager},
    ui::{
        ALBUM_COVER_GAP, ALBUM_COVER_HEIGHT, ALBUM_COVER_WIDTH, block, format_seconds,
        mark_as_favorite, render_input, sidebar, tab_bar,
    },
    widgets::{
        grid::Grid,
        playlist_list::PlaylistList,
        track_list::{TrackList, TrackListEvent},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PopupFocus {
    #[default]
    Sidebar,
    Content,
}

pub struct ArtistPopupState {
    focus: PopupFocus,
    artist_name: String,
    albums: Grid<AlbumSimple>,
    singles: Grid<AlbumSimple>,
    live: Grid<AlbumSimple>,
    compilations: Grid<AlbumSimple>,
    similar: Grid<Artist>,
    description: Option<String>,
    image_url: Option<String>,
    selected_sub_tab: usize,
    about_scroll: ScrollbarState,
    top_tracks: TrackList,
    id: u32,
}

enum SelectedArtistPopupSubtabMut<'a> {
    Albums(&'a mut Grid<AlbumSimple>),
    TopTracks(&'a mut TrackList),
    Similar(&'a mut Grid<Artist>),
}

struct Tab<'a> {
    name: &'a str,
    is_empty: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum TabKind {
    Albums,
    TopTracks,
    Singles,
    Live,
    Compilations,
    Similar,
    About,
}

impl ArtistPopupState {
    pub async fn new(artist: &Artist, client: &Client) -> AppResult<Self> {
        let id = artist.id;
        let artist_page = client.artist_page(id).await?;

        let state = Self {
            artist_name: artist.name.clone(),
            albums: Grid::new(artist_page.albums),
            singles: Grid::new(artist_page.singles),
            live: Grid::new(artist_page.live),
            compilations: Grid::new(artist_page.compilations),
            similar: Grid::new(artist_page.similar_artists),
            description: artist_page.description,
            image_url: artist_page.image,
            selected_sub_tab: 0,
            about_scroll: ScrollbarState::default(),
            top_tracks: TrackList::new(artist_page.top_tracks),
            id: artist.id,
            focus: Default::default(),
        };

        Ok(state)
    }

    fn focus_sidebar(&mut self) {
        self.focus = PopupFocus::Sidebar;
    }

    fn focus_content(&mut self) {
        self.focus = PopupFocus::Content;
    }

    fn stats_line(&self) -> String {
        let counts = [
            (self.albums.filter().len(), "albums"),
            (self.singles.filter().len(), "singles"),
            (self.live.filter().len(), "live"),
            (self.compilations.filter().len(), "compilations"),
        ];

        counts
            .iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, label)| format!("{count} {label}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn selected_tab_kind(&self) -> Option<TabKind> {
        self.visible_tab_kinds()
            .into_iter()
            .nth(self.selected_sub_tab)
    }

    fn cycle_subtab_backwards(&mut self) {
        let count = self.tabs().len();
        self.selected_sub_tab = (self.selected_sub_tab + count - 1) % count;
        self.about_scroll = ScrollbarState::default();
    }

    fn cycle_subtab(&mut self) {
        let count = self.tabs().len();
        self.selected_sub_tab = (self.selected_sub_tab + count + 1) % count;
        self.about_scroll = ScrollbarState::default();
    }

    fn scroll_about(&mut self, delta: i16) {
        let position = self
            .about_scroll
            .get_position()
            .saturating_add_signed(delta as isize);
        self.about_scroll = self.about_scroll.position(position);
    }

    fn visible_tab_kinds(&self) -> Vec<TabKind> {
        let mut tabs = vec![];

        if !self.albums.filter().is_empty() {
            tabs.push(TabKind::Albums);
        }
        if !self.top_tracks.filter().is_empty() {
            tabs.push(TabKind::TopTracks);
        }
        if !self.singles.filter().is_empty() {
            tabs.push(TabKind::Singles);
        }
        if !self.live.filter().is_empty() {
            tabs.push(TabKind::Live);
        }
        if !self.compilations.filter().is_empty() {
            tabs.push(TabKind::Compilations);
        }
        if !self.similar.all_items().is_empty() {
            tabs.push(TabKind::Similar);
        }
        if self.description.as_ref().is_some_and(|d| !d.is_empty()) {
            tabs.push(TabKind::About);
        }

        tabs
    }

    fn current_state_mut(&'_ mut self) -> Option<SelectedArtistPopupSubtabMut<'_>> {
        match self.selected_tab_kind()? {
            TabKind::Albums => Some(SelectedArtistPopupSubtabMut::Albums(&mut self.albums)),
            TabKind::TopTracks => Some(SelectedArtistPopupSubtabMut::TopTracks(
                &mut self.top_tracks,
            )),
            TabKind::Singles => Some(SelectedArtistPopupSubtabMut::Albums(&mut self.singles)),
            TabKind::Live => Some(SelectedArtistPopupSubtabMut::Albums(&mut self.live)),
            TabKind::Compilations => {
                Some(SelectedArtistPopupSubtabMut::Albums(&mut self.compilations))
            }
            TabKind::Similar => Some(SelectedArtistPopupSubtabMut::Similar(&mut self.similar)),
            TabKind::About => None,
        }
    }

    fn tabs(&self) -> Vec<&'static str> {
        vec![
            Tab {
                name: "Albums",
                is_empty: self.albums.filter().is_empty(),
            },
            Tab {
                name: "Top Tracks",
                is_empty: self.top_tracks.filter().is_empty(),
            },
            Tab {
                name: "Singles",
                is_empty: self.singles.filter().is_empty(),
            },
            Tab {
                name: "Live",
                is_empty: self.live.filter().is_empty(),
            },
            Tab {
                name: "Compilations",
                is_empty: self.compilations.filter().is_empty(),
            },
            Tab {
                name: "Similar",
                is_empty: self.similar.all_items().is_empty(),
            },
            Tab {
                name: "About",
                is_empty: self.description.as_ref().is_none_or(|d| d.is_empty()),
            },
        ]
        .into_iter()
        .filter(|t| !t.is_empty)
        .map(|x| x.name)
        .collect()
    }
}

pub struct AlbumPopupState {
    focus: PopupFocus,
    title: String,
    artist: Artist,
    tracks: TrackList,
    similar: Grid<AlbumSimple>,
    description: Option<String>,
    image_url: String,
    release_year: u32,
    total_tracks: u32,
    duration_seconds: u32,
    hires_available: bool,
    explicit: bool,
    bit_depth: Option<u32>,
    sampling_rate: Option<f32>,
    selected_sub_tab: usize,
    about_scroll: ScrollbarState,
    id: String,
    awards: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum AlbumTabKind {
    Tracks,
    Similar,
    About,
    GoToArtist,
}

enum SelectedAlbumPopupSubtabMut<'a> {
    Tracks(&'a mut TrackList),
    Similar(&'a mut Grid<AlbumSimple>),
}

impl AlbumPopupState {
    pub async fn new(album: Album, client: &Client) -> Self {
        let similar = client.suggested_albums(&album.id).await.unwrap_or_default();

        Self {
            focus: Default::default(),
            title: album.title,
            artist: album.artist,
            tracks: TrackList::new(album.tracks),
            similar: Grid::new(similar),
            description: album.description,
            image_url: album.image,
            release_year: album.release_year,
            total_tracks: album.total_tracks,
            duration_seconds: album.duration_seconds,
            hires_available: album.hires_available,
            explicit: album.explicit,
            bit_depth: album.bit_depth,
            sampling_rate: album.sampling_rate,
            selected_sub_tab: 0,
            about_scroll: ScrollbarState::default(),
            awards: album.awards,
            id: album.id,
        }
    }

    fn focus_sidebar(&mut self) {
        self.focus = PopupFocus::Sidebar;
    }

    fn focus_content(&mut self) {
        self.focus = PopupFocus::Content;
    }

    fn album_detail_lines(
        &self,
        is_favorite: bool,
        is_artist_favorite: bool,
    ) -> [Line<'static>; 3] {
        let title = Line::from(Span::styled(self.title.clone(), Style::new().bold()));
        let title = mark_as_favorite(title, is_favorite);

        let artist = Line::from(Span::from(self.artist.name.clone()));
        let artist = mark_as_favorite(artist, is_artist_favorite);

        let mut parts = Vec::new();
        if self.release_year > 0 {
            parts.push(self.release_year.to_string());
        }
        parts.push(format!("{} tracks", self.total_tracks));
        parts.push(format_seconds(self.duration_seconds));

        let awards = self.awards.len();

        if awards > 0 {
            parts.push(format!(
                "{awards} award{}",
                if awards == 1 { "" } else { "s" }
            ));
        }

        let mut info = vec![Span::styled(parts.join(" · "), Style::new().dim())];

        if self.hires_available {
            info.push(Span::styled(" · ", Style::new().dim()));
            info.push(Span::styled("\u{f0435}", Style::new().dim()));
            if let (Some(bit_depth), Some(sampling_rate)) = (self.bit_depth, self.sampling_rate) {
                info.push(Span::styled(
                    format!(" {bit_depth} bit - {sampling_rate}kHz"),
                    Style::new().dim(),
                ));
            }
        }

        if self.explicit {
            info.push(Span::styled(" · ", Style::new().dim()));
            info.push(Span::styled("\u{f0b0c}", Style::new().dim()));
        }

        [title, artist, Line::from(info)]
    }

    fn selected_tab_kind(&self) -> Option<AlbumTabKind> {
        self.visible_tab_kinds()
            .into_iter()
            .nth(self.selected_sub_tab)
    }

    fn cycle_subtab_backwards(&mut self) {
        let count = self.tabs().len();
        self.selected_sub_tab = (self.selected_sub_tab + count - 1) % count;
        self.about_scroll = ScrollbarState::default();
    }

    fn cycle_subtab(&mut self) {
        let count = self.tabs().len();
        self.selected_sub_tab = (self.selected_sub_tab + count + 1) % count;
        self.about_scroll = ScrollbarState::default();
    }

    fn scroll_about(&mut self, delta: i16) {
        let position = self
            .about_scroll
            .get_position()
            .saturating_add_signed(delta as isize);
        self.about_scroll = self.about_scroll.position(position);
    }

    fn visible_tab_kinds(&self) -> Vec<AlbumTabKind> {
        let mut tabs = vec![];

        if !self.tracks.filter().is_empty() {
            tabs.push(AlbumTabKind::Tracks);
        }
        if !self.similar.filter().is_empty() {
            tabs.push(AlbumTabKind::Similar);
        }
        if self.description.as_ref().is_some_and(|d| !d.is_empty()) {
            tabs.push(AlbumTabKind::About);
        }
        tabs.push(AlbumTabKind::GoToArtist);

        tabs
    }

    fn current_state_mut(&'_ mut self) -> Option<SelectedAlbumPopupSubtabMut<'_>> {
        match self.selected_tab_kind()? {
            AlbumTabKind::Tracks => Some(SelectedAlbumPopupSubtabMut::Tracks(&mut self.tracks)),
            AlbumTabKind::Similar => Some(SelectedAlbumPopupSubtabMut::Similar(&mut self.similar)),
            AlbumTabKind::About | AlbumTabKind::GoToArtist => None,
        }
    }

    fn tabs(&self) -> Vec<&'static str> {
        vec![
            Tab {
                name: "Tracks",
                is_empty: self.tracks.filter().is_empty(),
            },
            Tab {
                name: "Similar",
                is_empty: self.similar.filter().is_empty(),
            },
            Tab {
                name: "About",
                is_empty: self.description.as_ref().is_none_or(|d| d.is_empty()),
            },
            Tab {
                name: "Go to Artist",
                is_empty: false,
            },
        ]
        .into_iter()
        .filter(|t| !t.is_empty)
        .map(|x| x.name)
        .collect()
    }
}

pub struct PlaylistPopupState {
    shuffle: bool,
    tracks: TrackList,
    title: String,
    id: u32,
    is_owned: bool,
    image_url: Option<String>,
    owner: String,
    duration_seconds: u32,
}

impl PlaylistPopupState {
    pub fn new(playlist: Playlist) -> Self {
        Self {
            tracks: TrackList::new(playlist.tracks),
            title: playlist.title,
            id: playlist.id,
            is_owned: playlist.is_owned,
            image_url: playlist.image,
            owner: playlist.owner.name,
            duration_seconds: playlist.duration_seconds,
            shuffle: false,
        }
    }
}

pub struct DeletePlaylistPopupState {
    title: String,
    id: u32,
    confirm: bool,
}

impl DeletePlaylistPopupState {
    pub fn new(playlist: PlaylistSimple) -> Self {
        Self {
            title: playlist.title,
            id: playlist.id,
            confirm: false,
        }
    }
}

pub struct TrackPopupState {
    playlists: PlaylistList,
    track: Track,
}

impl TrackPopupState {
    pub fn new(track: Track, owned_playlists: Vec<PlaylistSimple>) -> Self {
        Self {
            playlists: PlaylistList::new(owned_playlists),
            track,
        }
    }

    fn select_next(&mut self) {
        self.playlists.select_next();
    }

    fn select_previous(&mut self) {
        self.playlists.select_previous();
    }
}

pub struct NewPlaylistPopupState {
    name: Input,
}

impl NewPlaylistPopupState {
    pub fn new() -> Self {
        Self {
            name: Default::default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TrackTabKind {
    Info,
    GoToAlbum,
    GoToArtist,
}

pub struct TrackInfoPopupState {
    track: Track,
    selected_sub_tab: usize,
}

impl TrackInfoPopupState {
    pub fn new(track: Track) -> Self {
        Self {
            track,
            selected_sub_tab: 0,
        }
    }

    fn tabs(&self) -> Vec<&'static str> {
        vec!["Info", "Go to Album", "Go to Artist"]
    }

    fn selected_tab_kind(&self) -> TrackTabKind {
        match self.selected_sub_tab {
            0 => TrackTabKind::Info,
            1 => TrackTabKind::GoToAlbum,
            _ => TrackTabKind::GoToArtist,
        }
    }

    fn cycle_subtab(&mut self) {
        self.selected_sub_tab = (self.selected_sub_tab + 1) % self.tabs().len();
    }

    fn cycle_subtab_backwards(&mut self) {
        self.selected_sub_tab = (self.selected_sub_tab + self.tabs().len() - 1) % self.tabs().len();
    }
}

#[allow(clippy::large_enum_variant)]
pub enum Popup {
    Artist(ArtistPopupState),
    Album(AlbumPopupState),
    Playlist(PlaylistPopupState),
    AddTrackToPlaylist(TrackPopupState),
    NewPlaylist(NewPlaylistPopupState),
    DeletePlaylist(DeletePlaylistPopupState),
    TrackInfo(TrackInfoPopupState),
}

impl Popup {
    pub fn render(
        &mut self,
        frame: &mut Frame,
        favorites: &FavoriteIds,
        image_cache: &mut ImageManager,
    ) {
        let screen = frame.area();

        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // "<- back"
                Constraint::Min(1),    // Popup content
            ])
            .split(screen);

        let back_area = areas[0];
        let popup_area = areas[1];

        frame.render_widget(Clear, screen);
        frame.render_widget(Paragraph::new("<- back"), back_area);

        match self {
            Popup::Album(album) => {
                let area = popup_area;
                let header_height = ALBUM_COVER_HEIGHT + 1;

                let outer_block = block(Some(&album.title));

                frame.render_widget(&outer_block, area);

                let inner = outer_block.inner(area);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(header_height), Constraint::Min(1)])
                    .split(inner);

                let image = image_cache.get_mut(&album.image_url);

                let can_render_cover = image.is_some()
                    && chunks[0].width >= ALBUM_COVER_WIDTH.saturating_add(2)
                    && chunks[0].height >= ALBUM_COVER_HEIGHT;

                let image_width = if can_render_cover {
                    ALBUM_COVER_WIDTH
                } else {
                    0
                };

                let gap = if can_render_cover { ALBUM_COVER_GAP } else { 0 };

                let header = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(image_width),
                        Constraint::Length(gap),
                        Constraint::Min(1),
                    ])
                    .split(chunks[0]);

                if can_render_cover && let Some(AppImage { protocol, .. }) = image {
                    frame.render_stateful_widget(StatefulImage::default(), header[0], protocol);
                }

                let [title, artist, misc] = album.album_detail_lines(
                    favorites.albums().contains(&album.id),
                    favorites.artists().contains(&album.artist.id),
                );

                let has_description = album.description.as_ref().is_some_and(|d| !d.is_empty());

                let info_constraints = if has_description {
                    vec![
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Length(2),
                        Constraint::Length(1),
                        Constraint::Min(0),
                    ]
                } else {
                    vec![
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Min(0),
                    ]
                };

                let info_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(info_constraints)
                    .split(header[2]);

                frame.render_widget(Paragraph::new(title), info_chunks[0]);
                frame.render_widget(Paragraph::new(artist), info_chunks[1]);

                if has_description {
                    if let Some(description) = &album.description {
                        let blurb = header_blurb(description, info_chunks[2].width as usize);
                        frame.render_widget(
                            Paragraph::new(blurb).style(Style::new().dim()),
                            info_chunks[2],
                        );
                    }
                    frame.render_widget(Paragraph::new(misc), info_chunks[3]);
                } else {
                    frame.render_widget(Paragraph::new(misc), info_chunks[2]);
                }

                let (sidebar_widget, sidebar_width) =
                    sidebar(album.tabs(), album.focus == PopupFocus::Sidebar);

                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
                    .split(chunks[1]);

                let mut sidebar_state = ListState::default();
                sidebar_state.select(Some(album.selected_sub_tab));

                frame.render_stateful_widget(sidebar_widget, body[0], &mut sidebar_state);

                let content = body[1];

                if album.selected_tab_kind() == Some(AlbumTabKind::GoToArtist) {
                    let hint = format!("Press Enter to open {}", album.artist.name);
                    frame.render_widget(Paragraph::new(hint).style(Style::new().dim()), content);
                } else if album.selected_tab_kind() == Some(AlbumTabKind::About) {
                    let description = album.description.clone().unwrap_or_default();
                    render_about(
                        frame,
                        content,
                        &description,
                        album.awards.as_ref(),
                        &mut album.about_scroll,
                    );
                } else if let Some(state) = album.current_state_mut() {
                    match state {
                        SelectedAlbumPopupSubtabMut::Tracks(track_list) => track_list.render(
                            content,
                            frame.buffer_mut(),
                            true,
                            true,
                            favorites.tracks(),
                        ),
                        SelectedAlbumPopupSubtabMut::Similar(album_list) => album_list.render(
                            content,
                            frame.buffer_mut(),
                            true,
                            favorites.albums(),
                            image_cache,
                        ),
                    }
                }
            }

            Popup::Artist(artist) => {
                let area = popup_area;
                let header_height = 6;

                let outer_block = block(Some(&artist.artist_name));

                frame.render_widget(&outer_block, area);

                let inner = outer_block.inner(area);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(header_height), Constraint::Min(1)])
                    .split(inner);

                let image = artist
                    .image_url
                    .as_ref()
                    .and_then(|x| image_cache.get_mut(x));

                let image_width = image
                    .as_ref()
                    .map(|image| (image.ratio * (header_height * 2) as f32) as u16)
                    .unwrap_or(0);

                let gap = if image_width > 0 { 2 } else { 0 };

                let header = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(image_width),
                        Constraint::Length(gap),
                        Constraint::Min(1),
                    ])
                    .split(chunks[0]);

                if let Some(AppImage { protocol, ratio: _ }) = image {
                    frame.render_stateful_widget(StatefulImage::default(), header[0], protocol);
                }

                let name = Line::from(Span::styled(
                    artist.artist_name.clone(),
                    Style::new().bold(),
                ));
                let name = mark_as_favorite(name, favorites.artists().contains(&artist.id));

                let info_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),
                        Constraint::Length(2),
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Min(0),
                    ])
                    .split(header[2]);

                frame.render_widget(Paragraph::new(name), info_chunks[0]);

                if let Some(description) = &artist.description {
                    let blurb = header_blurb(description, info_chunks[1].width as usize);
                    frame.render_widget(
                        Paragraph::new(blurb).style(Style::new().dim()),
                        info_chunks[2],
                    );
                }

                frame.render_widget(
                    Paragraph::new(artist.stats_line()).style(Style::new().dim()),
                    info_chunks[3],
                );

                let (sidebar_widget, sidebar_width) =
                    sidebar(artist.tabs(), artist.focus == PopupFocus::Sidebar);

                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
                    .split(chunks[1]);

                let mut sidebar_state = ListState::default();
                sidebar_state.select(Some(artist.selected_sub_tab));

                frame.render_stateful_widget(sidebar_widget, body[0], &mut sidebar_state);

                let content = body[1];

                if artist.selected_tab_kind() == Some(TabKind::About) {
                    let description = artist.description.clone().unwrap_or_default();
                    render_about(frame, content, &description, &[], &mut artist.about_scroll);
                } else if let Some(state) = artist.current_state_mut() {
                    match state {
                        SelectedArtistPopupSubtabMut::Albums(album_list) => album_list.render(
                            content,
                            frame.buffer_mut(),
                            true,
                            favorites.albums(),
                            image_cache,
                        ),
                        SelectedArtistPopupSubtabMut::TopTracks(track_list) => track_list.render(
                            content,
                            frame.buffer_mut(),
                            true,
                            true,
                            favorites.tracks(),
                        ),
                        SelectedArtistPopupSubtabMut::Similar(artist_list) => artist_list.render(
                            content,
                            frame.buffer_mut(),
                            true,
                            favorites.artists(),
                            image_cache,
                        ),
                    }
                }
            }

            Popup::Playlist(playlist) => {
                let area = popup_area;

                let header_height = ALBUM_COVER_HEIGHT + 1;

                let outer_block = block(Some(&playlist.title));

                frame.render_widget(&outer_block, area);

                let inner = outer_block.inner(area);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(header_height),
                        Constraint::Min(1),
                        Constraint::Length(1),
                    ])
                    .split(inner);

                let image = playlist
                    .image_url
                    .as_ref()
                    .and_then(|url| image_cache.get_mut(url));

                let can_render_cover = image.is_some()
                    && chunks[0].width >= ALBUM_COVER_WIDTH.saturating_add(2)
                    && chunks[0].height >= ALBUM_COVER_HEIGHT;

                let image_width = image
                    .as_ref()
                    .filter(|_| can_render_cover)
                    .map(|image| (image.ratio * (ALBUM_COVER_HEIGHT * 2) as f32) as u16)
                    .unwrap_or(0);

                let gap = if can_render_cover { ALBUM_COVER_GAP } else { 0 };

                let header = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(image_width),
                        Constraint::Length(gap),
                        Constraint::Min(1),
                    ])
                    .split(chunks[0]);

                if can_render_cover && let Some(AppImage { protocol, ratio }) = image {
                    let width = (*ratio * (ALBUM_COVER_HEIGHT * 2) as f32) as u16;

                    let image_area = Rect::new(
                        header[0].x,
                        header[0].y + header[0].height.saturating_sub(ALBUM_COVER_HEIGHT) / 2,
                        width,
                        ALBUM_COVER_HEIGHT,
                    );

                    frame.render_stateful_widget(StatefulImage::default(), image_area, protocol);
                }

                let info = vec![
                    Line::from(Span::styled(playlist.title.clone(), Style::new().bold())),
                    Line::from(playlist.owner.clone()),
                    Line::from(""),
                    Line::from(Span::styled(
                        format!(
                            "{} tracks · {}",
                            playlist.tracks.filter().len(),
                            format_seconds(playlist.duration_seconds),
                        ),
                        Style::new().dim(),
                    )),
                ];

                frame.render_widget(Paragraph::new(info), header[2]);

                playlist.tracks.render(
                    chunks[1],
                    frame.buffer_mut(),
                    true,
                    true,
                    favorites.tracks(),
                );

                let buttons = tab_bar(["Play", "Shuffle"].into(), usize::from(playlist.shuffle));

                frame.render_widget(buttons, chunks[2]);
            }

            Popup::AddTrackToPlaylist(track_state) => {
                let area = popup_area;
                let title = format!("Add {} to playlist", track_state.track.title);
                let block = block(Some(&title));

                frame.render_widget(&block, area);
                track_state.playlists.render(
                    block.inner(area),
                    frame.buffer_mut(),
                    true,
                    favorites.playlists(),
                );
            }

            Popup::NewPlaylist(state) => {
                render_input(&state.name, false, popup_area, frame, "Create playlist");
            }

            Popup::DeletePlaylist(state) => {
                let title = format!("Delete {}?", state.title);

                let tabs = tab_bar(
                    ["Delete", "Cancel"].into(),
                    if state.confirm { 0 } else { 1 },
                )
                .block(block(Some(&title)));

                frame.render_widget(tabs, popup_area);
            }
            Popup::TrackInfo(state) => {
                let area = popup_area;
                let track = &state.track;
                let image = track
                    .image
                    .as_ref()
                    .and_then(|url| image_cache.get_mut(url));

                let header_height = ALBUM_COVER_HEIGHT + 1;

                let outer_block = block(Some("Track Info"));

                frame.render_widget(&outer_block, area);

                let inner = outer_block.inner(area);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(header_height), Constraint::Min(1)])
                    .split(inner);

                let can_render_cover = image.is_some()
                    && chunks[0].width >= ALBUM_COVER_WIDTH.saturating_add(2)
                    && chunks[0].height >= ALBUM_COVER_HEIGHT;

                let image_width = if can_render_cover {
                    ALBUM_COVER_WIDTH
                } else {
                    0
                };

                let gap = if can_render_cover { ALBUM_COVER_GAP } else { 0 };

                let header = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(image_width),
                        Constraint::Length(gap),
                        Constraint::Min(1),
                    ])
                    .split(chunks[0]);

                if can_render_cover && let Some(AppImage { protocol, .. }) = image {
                    frame.render_stateful_widget(StatefulImage::default(), header[0], protocol);
                }

                let title = Line::from(Span::styled(track.title.clone(), Style::new().bold()));

                let artist_name = track
                    .artist_name
                    .clone()
                    .unwrap_or_else(|| "Unknown artist".to_string());

                let album_title = track
                    .album_title
                    .clone()
                    .unwrap_or_else(|| "Unknown album".to_string());

                let mut meta = vec![Span::styled(
                    format_seconds(track.duration_seconds),
                    Style::new().dim(),
                )];

                if track.hires_available {
                    meta.push(Span::styled(" · ", Style::new().dim()));
                    meta.push(Span::styled("󰐵", Style::new().dim()));

                    if let (Some(bit_depth), Some(sampling_rate)) =
                        (track.bit_depth, track.sampling_rate)
                    {
                        meta.push(Span::styled(
                            format!(" {bit_depth} bit - {sampling_rate}kHz"),
                            Style::new().dim(),
                        ));
                    }
                }

                if track.explicit {
                    meta.push(Span::styled(" · ", Style::new().dim()));
                    meta.push(Span::styled("󰬌", Style::new().dim()));
                }

                let header_lines = vec![
                    title,
                    Line::from(artist_name),
                    Line::from(album_title),
                    Line::from(meta),
                ];

                frame.render_widget(Paragraph::new(header_lines), header[2]);

                let tabs = vec!["Info", "Go to Album", "Go to Artist"];

                let (sidebar_widget, sidebar_width) = sidebar(tabs, true);

                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
                    .split(chunks[1]);

                let mut sidebar_state = ListState::default();
                sidebar_state.select(Some(state.selected_sub_tab));

                frame.render_stateful_widget(sidebar_widget, body[0], &mut sidebar_state);

                let content = body[1];

                match state.selected_sub_tab {
                    0 => {
                        let mut lines = vec![];

                        if let Some(release_date) = &track.release_date {
                            lines.push(Line::from(format!("Released: {release_date}")));
                        }

                        if let Some(performers) = &track.performers {
                            if !lines.is_empty() {
                                lines.push(Line::from(""));
                            }

                            lines.push(Line::styled("Credits", Style::new().bold()));

                            for credit in performers.split(" - ") {
                                lines.push(Line::from(credit.trim().to_string()));
                            }
                        }

                        if let Some(copyright) = &track.copyright {
                            if !lines.is_empty() {
                                lines.push(Line::from(""));
                            }

                            lines.push(Line::styled("Copyright", Style::new().bold()));

                            lines.push(Line::from(copyright.clone()));
                        }

                        frame.render_widget(Paragraph::new(lines), content);
                    }

                    1 => {
                        frame.render_widget(
                            Paragraph::new("Press Enter to open album").style(Style::new().dim()),
                            content,
                        );
                    }

                    2 => {
                        frame.render_widget(
                            Paragraph::new("Press Enter to open artist").style(Style::new().dim()),
                            content,
                        );
                    }

                    _ => {}
                }
            }
        };
    }

    pub async fn handle_event(
        &mut self,
        event: Event,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => match self {
                Popup::TrackInfo(state) => match key_event.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        state.cycle_subtab_backwards();
                        Ok(Output::Consumed)
                    }

                    KeyCode::Down | KeyCode::Char('j') => {
                        state.cycle_subtab();
                        Ok(Output::Consumed)
                    }

                    KeyCode::Enter if state.selected_tab_kind() == TrackTabKind::GoToAlbum => {
                        open_track_album(&state.track, client).await
                    }

                    KeyCode::Enter if state.selected_tab_kind() == TrackTabKind::GoToArtist => {
                        open_track_artist(&state.track, client).await
                    }

                    _ => Ok(Output::NotConsumed),
                },
                Popup::Album(album_state) => match album_state.focus {
                    PopupFocus::Sidebar => match key_event.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            album_state.cycle_subtab_backwards();
                            Ok(Output::Consumed)
                        }

                        KeyCode::Down | KeyCode::Char('j') => {
                            album_state.cycle_subtab();
                            Ok(Output::Consumed)
                        }

                        KeyCode::Enter => {
                            if album_state.selected_tab_kind() == Some(AlbumTabKind::GoToArtist) {
                                let state =
                                    ArtistPopupState::new(&album_state.artist, client).await?;

                                return Ok(Output::Popup(Popup::Artist(state)));
                            }

                            album_state.focus_content();
                            Ok(Output::Consumed)
                        }

                        _ => Ok(Output::NotConsumed),
                    },

                    PopupFocus::Content => match key_event.code {
                        KeyCode::Esc => {
                            album_state.focus_sidebar();
                            Ok(Output::Consumed)
                        }

                        KeyCode::Char('G') => {
                            let state = ArtistPopupState::new(&album_state.artist, client).await?;

                            Ok(Output::Popup(Popup::Artist(state)))
                        }

                        _ => {
                            if album_state.selected_tab_kind() == Some(AlbumTabKind::About) {
                                if let Some(delta) = about_scroll_delta(key_event.code) {
                                    album_state.scroll_about(delta);
                                }

                                return Ok(Output::Consumed);
                            }

                            if album_state.selected_tab_kind() == Some(AlbumTabKind::GoToArtist) {
                                if key_event.code == KeyCode::Enter {
                                    let state =
                                        ArtistPopupState::new(&album_state.artist, client).await?;

                                    return Ok(Output::Popup(Popup::Artist(state)));
                                }

                                return Ok(Output::Consumed);
                            }

                            let album_id = album_state.id.clone();

                            match album_state.current_state_mut() {
                                Some(SelectedAlbumPopupSubtabMut::Tracks(track_list)) => {
                                    track_list
                                        .handle_events(
                                            key_event.code,
                                            client,
                                            controls,
                                            notifications,
                                            TrackListEvent::Album(album_id),
                                        )
                                        .await
                                }

                                Some(SelectedAlbumPopupSubtabMut::Similar(album_list)) => {
                                    album_list
                                        .handle_events(
                                            key_event.code,
                                            client,
                                            controls,
                                            notifications,
                                        )
                                        .await
                                }

                                None => Ok(Output::Consumed),
                            }
                        }
                    },
                },
                Popup::Artist(artist_popup_state) => match artist_popup_state.focus {
                    PopupFocus::Sidebar => match key_event.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            artist_popup_state.cycle_subtab_backwards();
                            Ok(Output::Consumed)
                        }

                        KeyCode::Down | KeyCode::Char('j') => {
                            artist_popup_state.cycle_subtab();
                            Ok(Output::Consumed)
                        }

                        KeyCode::Enter => {
                            artist_popup_state.focus_content();
                            Ok(Output::Consumed)
                        }

                        _ => Ok(Output::NotConsumed),
                    },

                    PopupFocus::Content => match key_event.code {
                        KeyCode::Esc => {
                            artist_popup_state.focus_sidebar();
                            Ok(Output::Consumed)
                        }

                        _ => {
                            if artist_popup_state.selected_tab_kind() == Some(TabKind::About) {
                                if let Some(delta) = about_scroll_delta(key_event.code) {
                                    artist_popup_state.scroll_about(delta);
                                }

                                return Ok(Output::Consumed);
                            }

                            let artist_id = artist_popup_state.id;

                            match artist_popup_state.current_state_mut() {
                                Some(state) => match state {
                                    SelectedArtistPopupSubtabMut::Albums(album_list) => {
                                        album_list
                                            .handle_events(
                                                key_event.code,
                                                client,
                                                controls,
                                                notifications,
                                            )
                                            .await
                                    }

                                    SelectedArtistPopupSubtabMut::TopTracks(track_list) => {
                                        track_list
                                            .handle_events(
                                                key_event.code,
                                                client,
                                                controls,
                                                notifications,
                                                TrackListEvent::Artist(artist_id),
                                            )
                                            .await
                                    }

                                    SelectedArtistPopupSubtabMut::Similar(artist_list) => {
                                        artist_list
                                            .handle_events(
                                                key_event.code,
                                                client,
                                                controls,
                                                notifications,
                                            )
                                            .await
                                    }
                                },

                                None => Ok(Output::Consumed),
                            }
                        }
                    },
                },

                Popup::Playlist(playlist_popup_state) => match key_event.code {
                    KeyCode::Left | KeyCode::Char('h') | KeyCode::Right | KeyCode::Char('l') => {
                        playlist_popup_state.shuffle = !playlist_popup_state.shuffle;
                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('D') => {
                        let index = playlist_popup_state.tracks.selected();

                        if let Some(index) = index {
                            let playlist_track_id = playlist_popup_state
                                .tracks
                                .get(index)
                                .and_then(|p| p.playlist_track_id);

                            if playlist_popup_state.is_owned
                                && let Some(playlist_track_id) = playlist_track_id
                            {
                                client
                                    .playlist_delete_track(
                                        playlist_popup_state.id,
                                        &[playlist_track_id],
                                    )
                                    .await?;
                                playlist_popup_state.tracks.remove_at_index(index);
                            }
                        }

                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('u') => {
                        let index = playlist_popup_state.tracks.selected();

                        if let Some(index) = index {
                            let playlist_track_id = playlist_popup_state
                                .tracks
                                .get(index)
                                .and_then(|p| p.playlist_track_id);

                            if playlist_popup_state.is_owned
                                && let Some(playlist_track_id) = playlist_track_id
                            {
                                let new_index = index - 1;
                                client
                                    .update_playlist_track_position(
                                        new_index,
                                        playlist_popup_state.id,
                                        playlist_track_id,
                                    )
                                    .await?;

                                playlist_popup_state
                                    .tracks
                                    .move_index_to_new_index(index, new_index);

                                playlist_popup_state.tracks.select_index(new_index);
                            }
                        }

                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('d') => {
                        let index = playlist_popup_state.tracks.selected();

                        if let Some(index) = index {
                            let playlist_track_id = playlist_popup_state
                                .tracks
                                .get(index)
                                .and_then(|p| p.playlist_track_id);

                            if playlist_popup_state.is_owned
                                && let Some(playlist_track_id) = playlist_track_id
                            {
                                let new_index = index + 1;
                                client
                                    .update_playlist_track_position(
                                        new_index,
                                        playlist_popup_state.id,
                                        playlist_track_id,
                                    )
                                    .await?;

                                playlist_popup_state
                                    .tracks
                                    .move_index_to_new_index(index, new_index);

                                playlist_popup_state.tracks.select_index(new_index);
                            }
                        }

                        Ok(Output::Consumed)
                    }
                    _ => {
                        playlist_popup_state
                            .tracks
                            .handle_events(
                                key_event.code,
                                client,
                                controls,
                                notifications,
                                TrackListEvent::Playlist(
                                    playlist_popup_state.id,
                                    playlist_popup_state.shuffle,
                                ),
                            )
                            .await
                    }
                },
                Popup::AddTrackToPlaylist(track_popup_state) => match key_event.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        track_popup_state.select_previous();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        track_popup_state.select_next();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Enter => {
                        let index = track_popup_state.playlists.selected();
                        let id = index
                            .and_then(|index| track_popup_state.playlists.get(index))
                            .map(|p| p.id);

                        if let Some(id) = id {
                            return Ok(Output::AddTrackToPlaylistAndPopPopup((
                                track_popup_state.track.id,
                                id,
                            )));
                        }

                        Ok(Output::Consumed)
                    }
                    _ => Ok(Output::NotConsumed),
                },
                Popup::NewPlaylist(state) => match key_event.code {
                    KeyCode::Enter => {
                        let input = state.name.value();
                        client
                            .create_playlist(input.to_string(), false, Default::default(), None)
                            .await?;
                        Ok(Output::PopPopupUpdateFavorites)
                    }
                    _ => {
                        state.name.handle_event(&event);
                        Ok(Output::Consumed)
                    }
                },
                Popup::DeletePlaylist(state) => match key_event.code {
                    KeyCode::Enter => {
                        if state.confirm {
                            client.delete_playlist(state.id).await?;
                            return Ok(Output::PopPopupUpdateFavorites);
                        }

                        Ok(Output::PopPopupUpdateFavorites)
                    }
                    KeyCode::Left | KeyCode::Right => {
                        state.confirm = !state.confirm;
                        Ok(Output::Consumed)
                    }
                    _ => Ok(Output::Consumed),
                },
            },
            _ => Ok(Output::Consumed),
        }
    }
}

async fn open_track_artist(track: &Track, client: &Client) -> AppResult<Output> {
    if let Some(artist_id) = track.artist_id {
        let artist = Artist {
            id: artist_id,
            name: track.artist_name.clone().unwrap_or_default(),
            image: None,
        };
        let state = ArtistPopupState::new(&artist, client).await?;
        Ok(Output::Popup(Popup::Artist(state)))
    } else {
        Ok(Output::Consumed)
    }
}

async fn open_track_album(track: &Track, client: &Client) -> AppResult<Output> {
    if let Some(album_id) = track.album_id.clone() {
        let album = client.album(&album_id).await?;
        Ok(Output::Popup(Popup::Album(
            AlbumPopupState::new(album, client).await,
        )))
    } else {
        Ok(Output::Consumed)
    }
}

fn about_scroll_delta(code: KeyCode) -> Option<i16> {
    match code {
        KeyCode::Up | KeyCode::Char('k') => Some(-1),
        KeyCode::Down | KeyCode::Char('j') => Some(1),
        KeyCode::PageUp => Some(-10),
        KeyCode::PageDown => Some(10),
        _ => None,
    }
}

fn wrap_text(text: &str, width: u16) -> Vec<Line<'static>> {
    let width = (width as usize).max(1);
    let mut lines = Vec::new();

    for paragraph in text.lines() {
        let mut current = String::new();
        let mut current_len = 0;

        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            let extra = if current_len == 0 {
                word_len
            } else {
                word_len + 1
            };

            if current_len > 0 && current_len + extra > width {
                lines.push(Line::from(std::mem::take(&mut current)));
                current_len = 0;
            }

            if current_len > 0 {
                current.push(' ');
                current_len += 1;
            }
            current.push_str(word);
            current_len += word_len;
        }

        lines.push(Line::from(current));
    }

    lines
}

fn render_about(
    frame: &mut Frame,
    area: Rect,
    description: &str,
    awards: &[String],
    scroll: &mut ScrollbarState,
) {
    let [text_area, bar_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(2)]).areas(area);

    let mut lines: Vec<Line> = Vec::new();

    if !awards.is_empty() {
        lines.push(Line::styled("Awards", Style::new().bold()));

        for award in awards {
            lines.push(Line::from(format!("• {}", award)));
        }

        lines.push(Line::from(""));
    }

    lines.extend(wrap_text(description, text_area.width));

    let total = lines.len() as u16;
    let viewport = text_area.height;
    let max_scroll = total.saturating_sub(viewport);

    let position = scroll.get_position().min(max_scroll as usize);

    *scroll = scroll
        .position(position)
        .content_length((max_scroll + 1) as usize)
        .viewport_content_length(viewport as usize);

    frame.render_widget(
        Paragraph::new(Text::from(lines)).scroll((position as u16, 0)),
        text_area,
    );

    if total > viewport {
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            bar_area,
            scroll,
        );
    }
}

fn header_blurb(description: &str, width: usize) -> Line<'static> {
    let normalized = description.split_whitespace().collect::<Vec<_>>().join(" ");

    let hint = " [see about]";
    let ellipsis = "…";

    let reserved = hint.chars().count() + ellipsis.chars().count();

    let effective = width.saturating_sub(reserved);

    if normalized.chars().count() <= width {
        return Line::from(normalized);
    }

    let truncated: String = normalized.chars().take(effective).collect();

    let head = format!("{}{}", truncated.trim_end(), ellipsis);

    Line::from(vec![
        Span::raw(head),
        Span::styled(hint, Style::new().italic()),
    ])
}
