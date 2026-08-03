use controls_module::models::{Artist, Track};
use player_module::{AppResult, client::StreamClient};
use ratatui::{
    crossterm::event::KeyCode,
    prelude::*,
    widgets::{ListState, Paragraph},
};
use ratatui_image::StatefulImage;

use super::{AlbumOverlay, ArtistOverlay, Overlay};
use crate::{
    app::Output,
    image_cache::{AppImage, ImageManager},
    ui::{ALBUM_COVER_GAP, ALBUM_COVER_HEIGHT, ALBUM_COVER_WIDTH, block, format_seconds, sidebar},
};

pub struct TrackInfoOverlay {
    track: Track,
    selected_sub_tab: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackTabKind {
    Info,
    GoToAlbum,
    GoToArtist,
}

impl TrackInfoOverlay {
    pub const fn new(track: Track) -> Self {
        Self {
            track,
            selected_sub_tab: 0,
        }
    }

    pub fn title(&self) -> &str {
        &self.track.title
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, image_cache: &mut ImageManager) {
        let header_height = ALBUM_COVER_HEIGHT + 1;
        let outer_block = block(Some("Track Info"));

        frame.render_widget(&outer_block, area);

        let inner = outer_block.inner(area);

        let [header_area, body_area] =
            Layout::vertical([Constraint::Length(header_height), Constraint::Min(1)]).areas(inner);

        self.render_header(frame, header_area, image_cache);
        self.render_body(frame, body_area);
    }

    pub async fn handle_event(
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

            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l')
                if self.selected_tab_kind() == TrackTabKind::GoToAlbum =>
            {
                self.open_album(client).await
            }

            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l')
                if self.selected_tab_kind() == TrackTabKind::GoToArtist =>
            {
                self.open_artist(client).await
            }

            KeyCode::Esc => Ok(Output::PopOverlay),

            _ => Ok(Output::Consumed),
        }
    }

    fn render_header(&mut self, frame: &mut Frame, area: Rect, image_cache: &mut ImageManager) {
        let image = self
            .track
            .image
            .as_ref()
            .and_then(|url| image_cache.get_mut(url));

        let can_render_cover = image.is_some()
            && area.width >= ALBUM_COVER_WIDTH.saturating_add(ALBUM_COVER_GAP)
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

        if can_render_cover && let Some(AppImage { protocol, .. }) = image {
            frame.render_stateful_widget(StatefulImage::default(), image_area, protocol);
        }

        let title = Line::from(Span::styled(self.track.title.clone(), Style::new().bold()));

        let artist_name = self
            .track
            .artist_name
            .clone()
            .unwrap_or_else(|| "Unknown artist".to_owned());

        let album_title = self
            .track
            .album_title
            .clone()
            .unwrap_or_else(|| "Unknown album".to_owned());

        let metadata = self.metadata_line();

        let header_lines = vec![
            title,
            Line::from(artist_name),
            Line::from(album_title),
            metadata,
        ];

        frame.render_widget(Paragraph::new(header_lines), info_area);
    }

    fn render_body(&self, frame: &mut Frame, area: Rect) {
        let (sidebar_widget, sidebar_width) = sidebar(tabs(), true);

        let [sidebar_area, content_area] =
            Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(1)]).areas(area);

        let mut sidebar_state = ListState::default();
        sidebar_state.select(Some(self.selected_sub_tab));

        frame.render_stateful_widget(sidebar_widget, sidebar_area, &mut sidebar_state);

        match self.selected_tab_kind() {
            TrackTabKind::Info => {
                self.render_track_information(frame, content_area);
            }

            TrackTabKind::GoToAlbum => {
                let message = if self.track.album_id.is_some() {
                    "Press Enter to open album"
                } else {
                    "Album information is unavailable"
                };

                frame.render_widget(
                    Paragraph::new(message).style(Style::new().dim()),
                    content_area,
                );
            }

            TrackTabKind::GoToArtist => {
                let message = if self.track.artist_id.is_some() {
                    "Press Enter to open artist"
                } else {
                    "Artist information is unavailable"
                };

                frame.render_widget(
                    Paragraph::new(message).style(Style::new().dim()),
                    content_area,
                );
            }
        }
    }

    fn render_track_information(&self, frame: &mut Frame, area: Rect) {
        let mut lines = Vec::new();

        if let Some(release_date) = &self.track.release_date {
            lines.push(Line::from(format!("Released: {release_date}",)));
        }

        if let Some(performers) = self
            .track
            .performers
            .as_ref()
            .filter(|performers| !performers.is_empty())
        {
            if !lines.is_empty() {
                lines.push(Line::default());
            }

            lines.push(Line::styled("Credits", Style::new().bold()));

            lines.extend(
                performers
                    .split(" - ")
                    .map(str::trim)
                    .filter(|credit| !credit.is_empty())
                    .map(|credit| Line::from(credit.to_owned())),
            );
        }

        if let Some(copyright) = self
            .track
            .copyright
            .as_ref()
            .filter(|copyright| !copyright.is_empty())
        {
            if !lines.is_empty() {
                lines.push(Line::default());
            }

            lines.push(Line::styled("Copyright", Style::new().bold()));

            lines.push(Line::from(copyright.clone()));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn metadata_line(&self) -> Line<'static> {
        let mut metadata = vec![Span::styled(
            format_seconds(self.track.duration_seconds),
            Style::new().dim(),
        )];

        if self.track.hires_available {
            metadata.push(Span::styled(" · ", Style::new().dim()));

            metadata.push(Span::styled("󰐵", Style::new().dim()));

            if let (Some(bit_depth), Some(sampling_rate)) =
                (self.track.bit_depth, self.track.sampling_rate)
            {
                metadata.push(Span::styled(
                    format!(" {bit_depth} bit - {sampling_rate}kHz",),
                    Style::new().dim(),
                ));
            }
        }

        if self.track.explicit {
            metadata.push(Span::styled(" · ", Style::new().dim()));

            metadata.push(Span::styled("󰬌", Style::new().dim()));
        }

        Line::from(metadata)
    }

    async fn open_album(&self, client: &StreamClient) -> AppResult<Output> {
        let Some(album_id) = self.track.album_id.as_ref() else {
            return Ok(Output::Consumed);
        };

        let album = client.album(album_id).await?;
        let popup = AlbumOverlay::new(album, client).await;

        Ok(Output::Overlay(Overlay::Album(popup)))
    }

    async fn open_artist(&self, client: &StreamClient) -> AppResult<Output> {
        let Some(artist_id) = self.track.artist_id else {
            return Ok(Output::Consumed);
        };

        let artist = Artist {
            id: artist_id,
            name: self.track.artist_name.clone().unwrap_or_default(),
            image: None,
        };

        let popup = ArtistOverlay::new(&artist, client).await?;

        Ok(Output::Overlay(Overlay::Artist(popup)))
    }

    const fn selected_tab_kind(&self) -> TrackTabKind {
        match self.selected_sub_tab {
            0 => TrackTabKind::Info,
            1 => TrackTabKind::GoToAlbum,
            _ => TrackTabKind::GoToArtist,
        }
    }

    fn cycle_subtab(&mut self) {
        self.selected_sub_tab = (self.selected_sub_tab + 1) % tabs().len();
    }

    fn cycle_subtab_backwards(&mut self) {
        let count = tabs().len();

        self.selected_sub_tab = (self.selected_sub_tab + count - 1) % count;
    }
}

fn tabs() -> Vec<&'static str> {
    vec!["Info", "Go to Album", "Go to Artist"]
}
