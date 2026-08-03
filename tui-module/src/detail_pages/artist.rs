use controls_module::{
    controls::Controls,
    models::{AlbumSimple, Artist},
};
use num_traits::ToPrimitive;
use player_module::{AppResult, client::StreamClient};
use ratatui::{
    crossterm::event::KeyCode,
    prelude::*,
    widgets::{ListState, Paragraph, ScrollbarState},
};
use ratatui_image::StatefulImage;

use super::{OverlayFocus, about_scroll_delta, header_blurb, render_about, scroll_about};
use crate::{
    app::{FavoriteIds, NotificationList, Output},
    image_cache::{AppImage, ImageManager},
    ui::{block, mark_as_favorite, sidebar},
    widgets::{
        grid::Grid,
        track_list::{TrackList, TrackListEvent},
    },
};

pub struct ArtistOverlay {
    focus: OverlayFocus,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtistTabKind {
    Albums,
    TopTracks,
    Singles,
    Live,
    Compilations,
    Similar,
    About,
}

enum SelectedTabMut<'a> {
    Albums(&'a mut Grid<AlbumSimple>),
    TopTracks(&'a mut TrackList),
    SimilarArtists(&'a mut Grid<Artist>),
}

impl ArtistOverlay {
    pub async fn new(artist: &Artist, client: &StreamClient) -> AppResult<Self> {
        let artist_page = client.artist_page(artist.id).await?;

        Ok(Self {
            focus: OverlayFocus::default(),
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
        })
    }

    pub fn title(&self) -> &str {
        &self.artist_name
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        favorites: &FavoriteIds,
        image_cache: &mut ImageManager,
    ) {
        let header_height = 6;
        let outer_block = block(Some(&self.artist_name));

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
            OverlayFocus::Sidebar => Ok(self.handle_sidebar_event(code)),

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
        let image = self
            .image_url
            .as_ref()
            .and_then(|url| image_cache.get_mut(url));

        let image_width = image
            .as_ref()
            .and_then(|image| (image.ratio * f32::from(area.height * 2)).to_u16())
            .unwrap_or_default();

        let gap = if image_width > 0 { 2 } else { 0 };

        let [image_area, _, info_area] = Layout::horizontal([
            Constraint::Length(image_width),
            Constraint::Length(gap),
            Constraint::Min(1),
        ])
        .areas(area);

        if let Some(AppImage { protocol, .. }) = image {
            frame.render_stateful_widget(StatefulImage::default(), image_area, protocol);
        }

        let name = Line::from(Span::styled(self.artist_name.clone(), Style::new().bold()));

        let name = mark_as_favorite(name, favorites.artists().contains(&self.id));

        let [name_area, description_area, stats_area, _] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(info_area);

        frame.render_widget(Paragraph::new(name), name_area);

        if let Some(description) = self
            .description
            .as_ref()
            .filter(|description| !description.is_empty())
        {
            let blurb = header_blurb(
                description,
                description_area.width.to_usize().unwrap_or_default(),
            );

            frame.render_widget(
                Paragraph::new(blurb).style(Style::new().dim()),
                description_area,
            );
        }

        frame.render_widget(
            Paragraph::new(self.stats_line()).style(Style::new().dim()),
            stats_area,
        );
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
            Some(ArtistTabKind::About) => {
                let description = self.description.clone().unwrap_or_default();

                render_about(
                    frame,
                    content_area,
                    &description,
                    &[],
                    &mut self.about_scroll,
                );
            }

            Some(
                ArtistTabKind::Albums
                | ArtistTabKind::TopTracks
                | ArtistTabKind::Singles
                | ArtistTabKind::Live
                | ArtistTabKind::Compilations
                | ArtistTabKind::Similar,
            ) => {
                if let Some(selected) = self.current_tab_mut() {
                    match selected {
                        SelectedTabMut::Albums(album_grid) => {
                            album_grid.render(
                                content_area,
                                frame.buffer_mut(),
                                true,
                                favorites.albums(),
                                image_cache,
                            );
                        }

                        SelectedTabMut::TopTracks(track_list) => {
                            track_list.render(
                                content_area,
                                frame.buffer_mut(),
                                true,
                                true,
                                favorites.tracks(),
                            );
                        }

                        SelectedTabMut::SimilarArtists(artist_grid) => {
                            artist_grid.render(
                                content_area,
                                frame.buffer_mut(),
                                true,
                                favorites.artists(),
                                image_cache,
                            );
                        }
                    }
                }
            }

            None => {}
        }
    }

    fn handle_sidebar_event(&mut self, code: KeyCode) -> Output {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.cycle_subtab_backwards();
                Output::Consumed
            }

            KeyCode::Down | KeyCode::Char('j') => {
                self.cycle_subtab();
                Output::Consumed
            }

            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.focus = OverlayFocus::Content;
                Output::Consumed
            }

            KeyCode::Esc => Output::PopOverlay,

            _ => Output::Consumed,
        }
    }

    async fn handle_content_event(
        &mut self,
        code: KeyCode,
        client: &StreamClient,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        if matches!(code, KeyCode::Esc) {
            self.focus = OverlayFocus::Sidebar;
            return Ok(Output::Consumed);
        }

        if self.selected_tab_kind() == Some(ArtistTabKind::About) {
            if let Some(delta) = about_scroll_delta(code) {
                scroll_about(&mut self.about_scroll, delta);
            }

            return Ok(Output::Consumed);
        }

        let artist_id = self.id;

        match self.current_tab_mut() {
            Some(SelectedTabMut::Albums(album_grid)) => {
                album_grid
                    .handle_events(code, client, controls, notifications)
                    .await
            }

            Some(SelectedTabMut::TopTracks(track_list)) => {
                track_list
                    .handle_events(
                        code,
                        client,
                        controls,
                        notifications,
                        TrackListEvent::Artist(artist_id),
                    )
                    .await
            }

            Some(SelectedTabMut::SimilarArtists(artist_grid)) => {
                artist_grid
                    .handle_events(code, client, controls, notifications)
                    .await
            }

            None => Ok(Output::Consumed),
        }
    }

    fn stats_line(&self) -> String {
        [
            (self.albums.filter().len(), "albums"),
            (self.singles.filter().len(), "singles"),
            (self.live.filter().len(), "live"),
            (self.compilations.filter().len(), "compilations"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(" · ")
    }

    fn selected_tab_kind(&self) -> Option<ArtistTabKind> {
        self.visible_tab_kinds().get(self.selected_sub_tab).copied()
    }

    fn current_tab_mut(&mut self) -> Option<SelectedTabMut<'_>> {
        match self.selected_tab_kind()? {
            ArtistTabKind::Albums => Some(SelectedTabMut::Albums(&mut self.albums)),

            ArtistTabKind::TopTracks => Some(SelectedTabMut::TopTracks(&mut self.top_tracks)),

            ArtistTabKind::Singles => Some(SelectedTabMut::Albums(&mut self.singles)),

            ArtistTabKind::Live => Some(SelectedTabMut::Albums(&mut self.live)),

            ArtistTabKind::Compilations => Some(SelectedTabMut::Albums(&mut self.compilations)),

            ArtistTabKind::Similar => Some(SelectedTabMut::SimilarArtists(&mut self.similar)),

            ArtistTabKind::About => None,
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

    fn visible_tab_kinds(&self) -> Vec<ArtistTabKind> {
        let mut tabs = Vec::with_capacity(7);

        if !self.albums.filter().is_empty() {
            tabs.push(ArtistTabKind::Albums);
        }

        if !self.top_tracks.filter().is_empty() {
            tabs.push(ArtistTabKind::TopTracks);
        }

        if !self.singles.filter().is_empty() {
            tabs.push(ArtistTabKind::Singles);
        }

        if !self.live.filter().is_empty() {
            tabs.push(ArtistTabKind::Live);
        }

        if !self.compilations.filter().is_empty() {
            tabs.push(ArtistTabKind::Compilations);
        }

        if !self.similar.all_items().is_empty() {
            tabs.push(ArtistTabKind::Similar);
        }

        if self
            .description
            .as_ref()
            .is_some_and(|description| !description.is_empty())
        {
            tabs.push(ArtistTabKind::About);
        }

        tabs
    }

    fn tabs(&self) -> Vec<&'static str> {
        self.visible_tab_kinds()
            .into_iter()
            .map(|tab| match tab {
                ArtistTabKind::Albums => "Albums",
                ArtistTabKind::TopTracks => "Top Tracks",
                ArtistTabKind::Singles => "Singles",
                ArtistTabKind::Live => "Live",
                ArtistTabKind::Compilations => "Compilations",
                ArtistTabKind::Similar => "Similar",
                ArtistTabKind::About => "About",
            })
            .collect()
    }
}
