use controls_module::{
    controls::Controls,
    models::{Album, AlbumSimple, Artist},
};
use num_traits::ToPrimitive;
use player_module::{AppResult, client::StreamClient};
use ratatui::{
    crossterm::event::KeyCode,
    prelude::*,
    widgets::{ListState, Paragraph, ScrollbarState},
};
use ratatui_image::StatefulImage;

use super::{
    ArtistOverlay, Overlay, OverlayFocus, about_scroll_delta, header_blurb, render_about,
    scroll_about,
};
use crate::{
    app::{FavoriteIds, NotificationList, Output},
    image_cache::{AppImage, ImageManager},
    ui::{
        ALBUM_COVER_GAP, ALBUM_COVER_HEIGHT, ALBUM_COVER_WIDTH, block, format_seconds,
        mark_as_favorite, sidebar,
    },
    widgets::{
        grid::Grid,
        track_list::{TrackList, TrackListEvent},
    },
};

pub struct AlbumOverlay {
    focus: OverlayFocus,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlbumTabKind {
    Tracks,
    Similar,
    About,
    GoToArtist,
}

enum SelectedTabMut<'a> {
    Tracks(&'a mut TrackList),
    Similar(&'a mut Grid<AlbumSimple>),
}

impl AlbumOverlay {
    pub async fn new(album: Album, client: &StreamClient) -> Self {
        let similar = client.suggested_albums(&album.id).await.unwrap_or_default();

        Self {
            focus: OverlayFocus::default(),
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
            id: album.id,
            awards: album.awards,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        favorites: &FavoriteIds,
        image_cache: &mut ImageManager,
    ) {
        let header_height = ALBUM_COVER_HEIGHT + 1;
        let outer_block = block(Some(&self.title));

        frame.render_widget(&outer_block, area);

        let inner = outer_block.inner(area);
        let [header_area, body_area] =
            Layout::vertical([Constraint::Length(header_height), Constraint::Min(1)]).areas(inner);

        self.render_header(frame, header_area, favorites, image_cache);
        self.render_body(frame, body_area, favorites, image_cache);
    }

    pub async fn handle_event(
        &mut self,
        code: KeyCode,
        client: &StreamClient,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match self.focus {
            OverlayFocus::Sidebar => self.handle_sidebar_event(code, client).await,

            OverlayFocus::Content => {
                self.handle_content_event(code, client, controls, notifications)
                    .await
            }
        }
    }

    fn render_header(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        favorites: &FavoriteIds,
        image_cache: &mut ImageManager,
    ) {
        let can_render_cover = image_cache.get_mut(&self.image_url).is_some()
            && area.width >= ALBUM_COVER_WIDTH.saturating_add(2)
            && area.height >= ALBUM_COVER_HEIGHT;

        let image_width = if can_render_cover {
            ALBUM_COVER_WIDTH
        } else {
            0
        };

        let gap = if can_render_cover { ALBUM_COVER_GAP } else { 0 };

        let [image_area, _, info_area] = Layout::horizontal([
            Constraint::Length(image_width),
            Constraint::Length(gap),
            Constraint::Min(1),
        ])
        .areas(area);

        if can_render_cover
            && let Some(AppImage { protocol, .. }) = image_cache.get_mut(&self.image_url)
        {
            frame.render_stateful_widget(StatefulImage::default(), image_area, protocol);
        }

        let [title, artist, metadata] = self.album_detail_lines(
            favorites.albums().contains(&self.id),
            favorites.artists().contains(&self.artist.id),
        );

        let has_description = self
            .description
            .as_ref()
            .is_some_and(|description| !description.is_empty());

        if has_description {
            let [title_area, artist_area, description_area, metadata_area, _] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .areas(info_area);

            frame.render_widget(Paragraph::new(title), title_area);
            frame.render_widget(Paragraph::new(artist), artist_area);
            frame.render_widget(Paragraph::new(metadata), metadata_area);

            if let Some(description) = &self.description {
                let blurb = header_blurb(
                    description,
                    description_area.width.to_usize().unwrap_or_default(),
                );

                frame.render_widget(
                    Paragraph::new(blurb).style(Style::new().dim()),
                    description_area,
                );
            }
        } else {
            let [title_area, artist_area, metadata_area, _] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .areas(info_area);

            frame.render_widget(Paragraph::new(title), title_area);
            frame.render_widget(Paragraph::new(artist), artist_area);
            frame.render_widget(Paragraph::new(metadata), metadata_area);
        }
    }

    fn render_body(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        favorites: &FavoriteIds,
        image_cache: &mut ImageManager,
    ) {
        let (sidebar_widget, sidebar_width) =
            sidebar(self.tabs(), self.focus == OverlayFocus::Sidebar);

        let [sidebar_area, content_area] =
            Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(1)]).areas(area);

        let mut sidebar_state = ListState::default();
        sidebar_state.select(Some(self.selected_sub_tab));

        frame.render_stateful_widget(sidebar_widget, sidebar_area, &mut sidebar_state);

        match self.selected_tab_kind() {
            Some(AlbumTabKind::GoToArtist) => {
                let hint = format!("Press Enter to open {}", self.artist.name);

                frame.render_widget(Paragraph::new(hint).style(Style::new().dim()), content_area);
            }

            Some(AlbumTabKind::About) => {
                render_about(
                    frame,
                    content_area,
                    self.description.as_deref().unwrap_or_default(),
                    &self.awards,
                    &mut self.about_scroll,
                );
            }

            Some(AlbumTabKind::Tracks | AlbumTabKind::Similar) => {
                if let Some(selected) = self.current_tab_mut() {
                    match selected {
                        SelectedTabMut::Tracks(track_list) => {
                            track_list.render(
                                content_area,
                                frame.buffer_mut(),
                                true,
                                true,
                                favorites.tracks(),
                            );
                        }

                        SelectedTabMut::Similar(album_grid) => {
                            album_grid.render(
                                content_area,
                                frame.buffer_mut(),
                                true,
                                favorites.albums(),
                                image_cache,
                            );
                        }
                    }
                }
            }

            None => {}
        }
    }

    async fn handle_sidebar_event(
        &mut self,
        code: KeyCode,
        client: &StreamClient,
    ) -> AppResult<Output> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.cycle_subtab_backwards();
                Ok(Output::Consumed)
            }

            KeyCode::Down | KeyCode::Char('j') => {
                self.cycle_subtab();
                Ok(Output::Consumed)
            }

            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if self.selected_tab_kind() == Some(AlbumTabKind::GoToArtist) {
                    return self.open_artist(client).await;
                }

                self.focus = OverlayFocus::Content;
                Ok(Output::Consumed)
            }

            KeyCode::Esc => Ok(Output::PopOverlay),

            _ => Ok(Output::Consumed),
        }
    }

    async fn handle_content_event(
        &mut self,
        code: KeyCode,
        client: &StreamClient,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match code {
            KeyCode::Esc => {
                self.focus = OverlayFocus::Sidebar;
                return Ok(Output::Consumed);
            }

            KeyCode::Char('G') => {
                return self.open_artist(client).await;
            }

            _ => {}
        }

        match self.selected_tab_kind() {
            Some(AlbumTabKind::About) => {
                if let Some(delta) = about_scroll_delta(code) {
                    scroll_about(&mut self.about_scroll, delta);
                }

                Ok(Output::Consumed)
            }

            Some(AlbumTabKind::GoToArtist) => {
                if code == KeyCode::Enter {
                    self.open_artist(client).await
                } else {
                    Ok(Output::Consumed)
                }
            }

            Some(AlbumTabKind::Tracks | AlbumTabKind::Similar) => {
                let album_id = self.id.clone();

                match self.current_tab_mut() {
                    Some(SelectedTabMut::Tracks(track_list)) => {
                        track_list
                            .handle_events(
                                code,
                                client,
                                controls,
                                notifications,
                                TrackListEvent::Album(album_id),
                            )
                            .await
                    }

                    Some(SelectedTabMut::Similar(album_grid)) => {
                        album_grid
                            .handle_events(code, client, controls, notifications)
                            .await
                    }

                    None => Ok(Output::Consumed),
                }
            }

            None => Ok(Output::Consumed),
        }
    }

    async fn open_artist(&self, client: &StreamClient) -> AppResult<Output> {
        let popup = ArtistOverlay::new(&self.artist, client).await?;
        Ok(Output::Overlay(Overlay::Artist(popup)))
    }

    fn album_detail_lines(
        &self,
        is_favorite: bool,
        is_artist_favorite: bool,
    ) -> [Line<'static>; 3] {
        let title = Line::from(Span::styled(self.title.clone(), Style::new().bold()));

        let title = mark_as_favorite(title, is_favorite);

        let artist = Line::from(self.artist.name.clone());
        let artist = mark_as_favorite(artist, is_artist_favorite);

        let mut parts = Vec::new();

        if self.release_year > 0 {
            parts.push(self.release_year.to_string());
        }

        parts.push(format!("{} tracks", self.total_tracks));
        parts.push(format_seconds(self.duration_seconds));

        if !self.awards.is_empty() {
            let count = self.awards.len();

            parts.push(format!(
                "{count} award{}",
                if count == 1 { "" } else { "s" },
            ));
        }

        let mut metadata = vec![Span::styled(parts.join(" · "), Style::new().dim())];

        if self.hires_available {
            metadata.push(Span::styled(" · ", Style::new().dim()));
            metadata.push(Span::styled("󰐵", Style::new().dim()));

            if let (Some(bit_depth), Some(sampling_rate)) = (self.bit_depth, self.sampling_rate) {
                metadata.push(Span::styled(
                    format!(" {bit_depth} bit - {sampling_rate}kHz"),
                    Style::new().dim(),
                ));
            }
        }

        if self.explicit {
            metadata.push(Span::styled(" · ", Style::new().dim()));
            metadata.push(Span::styled("󰬌", Style::new().dim()));
        }

        [title, artist, Line::from(metadata)]
    }

    fn selected_tab_kind(&self) -> Option<AlbumTabKind> {
        self.visible_tab_kinds().get(self.selected_sub_tab).copied()
    }

    fn current_tab_mut(&mut self) -> Option<SelectedTabMut<'_>> {
        match self.selected_tab_kind()? {
            AlbumTabKind::Tracks => Some(SelectedTabMut::Tracks(&mut self.tracks)),

            AlbumTabKind::Similar => Some(SelectedTabMut::Similar(&mut self.similar)),

            AlbumTabKind::About | AlbumTabKind::GoToArtist => None,
        }
    }

    fn cycle_subtab(&mut self) {
        let count = self.visible_tab_kinds().len();

        if count == 0 {
            return;
        }

        self.selected_sub_tab = (self.selected_sub_tab + 1) % count;

        self.about_scroll = ScrollbarState::default();
    }

    fn cycle_subtab_backwards(&mut self) {
        let count = self.visible_tab_kinds().len();

        if count == 0 {
            return;
        }

        self.selected_sub_tab = (self.selected_sub_tab + count - 1) % count;

        self.about_scroll = ScrollbarState::default();
    }

    fn visible_tab_kinds(&self) -> Vec<AlbumTabKind> {
        let mut tabs = Vec::with_capacity(4);

        if !self.tracks.filter().is_empty() {
            tabs.push(AlbumTabKind::Tracks);
        }

        if !self.similar.filter().is_empty() {
            tabs.push(AlbumTabKind::Similar);
        }

        if self
            .description
            .as_ref()
            .is_some_and(|description| !description.is_empty())
        {
            tabs.push(AlbumTabKind::About);
        }

        tabs.push(AlbumTabKind::GoToArtist);
        tabs
    }

    fn tabs(&self) -> Vec<&'static str> {
        self.visible_tab_kinds()
            .into_iter()
            .map(|tab| match tab {
                AlbumTabKind::Tracks => "Tracks",
                AlbumTabKind::Similar => "Similar",
                AlbumTabKind::About => "About",
                AlbumTabKind::GoToArtist => "Go to Artist",
            })
            .collect()
    }
}
