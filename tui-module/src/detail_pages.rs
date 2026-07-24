use controls_module::controls::Controls;
use player_module::{AppResult, client::Client};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::*,
};

use crate::{
    app::{FavoriteIds, NotificationList, Output},
    image_cache::ImageManager,
};

mod add_track;
mod album;
mod artist;
mod delete_playlist;
mod new_playlist;
mod playlist;
mod track_info;

pub use add_track::AddTrackOverlay;
pub use album::AlbumOverlay;
pub use artist::ArtistOverlay;
pub use delete_playlist::DeletePlaylistOverlay;
pub use new_playlist::NewPlaylistOverlay;
pub use playlist::PlaylistOverlay;
pub use track_info::TrackInfoOverlay;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum OverlayFocus {
    #[default]
    Sidebar,
    Content,
}

#[allow(clippy::large_enum_variant)]
pub enum Overlay {
    Artist(ArtistOverlay),
    Album(AlbumOverlay),
    Playlist(PlaylistOverlay),
    AddTrackToPlaylist(AddTrackOverlay),
    NewPlaylist(NewPlaylistOverlay),
    DeletePlaylist(DeletePlaylistOverlay),
    TrackInfo(TrackInfoOverlay),
}

impl Overlay {
    pub fn title(&self) -> String {
        match self {
            Overlay::Artist(state) => state.title().to_string(),
            Overlay::Album(state) => state.title().to_string(),
            Overlay::Playlist(state) => state.title().to_string(),
            Overlay::AddTrackToPlaylist(state) => {
                format!("Add {} to playlist", state.track_title())
            }
            Overlay::NewPlaylist(_) => "New playlist".to_string(),
            Overlay::DeletePlaylist(_) => "Delete playlist".to_string(),
            Overlay::TrackInfo(state) => state.title().to_string(),
        }
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        favorites: &FavoriteIds,
        image_cache: &mut ImageManager,
        breadcrumb_titles: &[String],
    ) {
        let screen = frame.area();

        let [breadcrumb_area, popup_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(screen);

        frame.render_widget(Clear, screen);
        frame.render_widget(
            Paragraph::new(breadcrumb_line(breadcrumb_titles)),
            breadcrumb_area,
        );

        match self {
            Self::Artist(popup) => {
                popup.render(frame, popup_area, favorites, image_cache);
            }

            Self::Album(popup) => {
                popup.render(frame, popup_area, favorites, image_cache);
            }

            Self::Playlist(popup) => {
                popup.render(frame, popup_area, favorites, image_cache);
            }

            Self::AddTrackToPlaylist(popup) => {
                popup.render(frame, popup_area, favorites);
            }

            Self::NewPlaylist(popup) => {
                popup.render(frame, popup_area);
            }

            Self::DeletePlaylist(popup) => {
                popup.render(frame, popup_area);
            }

            Self::TrackInfo(popup) => {
                popup.render(frame, popup_area, image_cache);
            }
        }
    }

    pub async fn handle_event(
        &mut self,
        event: Event,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        let Event::Key(key_event) = event else {
            return Ok(Output::Consumed);
        };

        if key_event.kind != KeyEventKind::Press {
            return Ok(Output::Consumed);
        }

        match self {
            Self::Artist(popup) => {
                popup
                    .handle_event(key_event.code, client, controls, notifications)
                    .await
            }

            Self::Album(popup) => {
                popup
                    .handle_event(key_event.code, client, controls, notifications)
                    .await
            }

            Self::Playlist(popup) => {
                popup
                    .handle_event(key_event.code, client, controls, notifications)
                    .await
            }

            Self::AddTrackToPlaylist(popup) => popup.handle_event(key_event.code),

            Self::NewPlaylist(popup) => popup.handle_event(key_event.code, &event, client).await,

            Self::DeletePlaylist(popup) => popup.handle_event(key_event.code, client).await,

            Self::TrackInfo(popup) => popup.handle_event(key_event.code, client).await,
        }
    }
}

// -----------------------------------------------------------------------------
// Shared popup helpers
// -----------------------------------------------------------------------------

pub(super) fn about_scroll_delta(code: KeyCode) -> Option<i16> {
    match code {
        KeyCode::Up | KeyCode::Char('k') => Some(-1),
        KeyCode::Down | KeyCode::Char('j') => Some(1),
        KeyCode::PageUp => Some(-10),
        KeyCode::PageDown => Some(10),
        _ => None,
    }
}

pub(super) fn scroll_about(scroll: &mut ScrollbarState, delta: i16) {
    let position = scroll.get_position().saturating_add_signed(delta as isize);

    *scroll = scroll.position(position);
}

pub(super) fn render_about(
    frame: &mut Frame,
    area: Rect,
    description: &str,
    awards: &[String],
    scroll: &mut ScrollbarState,
) {
    let [text_area, scrollbar_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(2)]).areas(area);

    let mut lines = Vec::new();

    if !awards.is_empty() {
        lines.push(Line::styled("Awards", Style::new().bold()));

        lines.extend(awards.iter().map(|award| Line::from(format!("• {award}"))));

        lines.push(Line::default());
    }

    lines.extend(wrap_text(description, text_area.width));

    let total_lines = lines.len();
    let viewport_height = text_area.height as usize;
    let max_scroll = total_lines.saturating_sub(viewport_height);

    let position = scroll.get_position().min(max_scroll);

    *scroll = scroll
        .position(position)
        .content_length(total_lines)
        .viewport_content_length(viewport_height);

    frame.render_widget(
        Paragraph::new(Text::from(lines)).scroll((position as u16, 0)),
        text_area,
    );

    if total_lines > viewport_height {
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            scrollbar_area,
            scroll,
        );
    }
}

pub(super) fn header_blurb(description: &str, width: usize) -> Line<'static> {
    let normalized = description.split_whitespace().collect::<Vec<_>>().join(" ");

    if normalized.chars().count() <= width {
        return Line::from(normalized);
    }

    let hint = " [see about]";
    let ellipsis = "…";

    let reserved_width = hint.chars().count() + ellipsis.chars().count();

    let available_width = width.saturating_sub(reserved_width);

    let truncated: String = normalized.chars().take(available_width).collect();

    let head = format!("{}{}", truncated.trim_end(), ellipsis);

    Line::from(vec![
        Span::raw(head),
        Span::styled(hint, Style::new().italic()),
    ])
}

fn wrap_text(text: &str, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let mut lines = Vec::new();

    for paragraph in text.lines() {
        let mut current = String::new();
        let mut current_width = 0;

        for word in paragraph.split_whitespace() {
            let word_width = word.chars().count();
            let separator_width = usize::from(current_width > 0);
            let required_width = separator_width + word_width;

            if current_width > 0 && current_width + required_width > width {
                lines.push(Line::from(std::mem::take(&mut current)));
                current_width = 0;
            }

            if current_width > 0 {
                current.push(' ');
                current_width += 1;
            }

            current.push_str(word);
            current_width += word_width;
        }

        lines.push(Line::from(current));
    }

    lines
}

fn breadcrumb_line(titles: &[String]) -> Line<'_> {
    let mut spans = Vec::new();

    for (index, title) in titles.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                " › ",
                Style::default().add_modifier(Modifier::DIM),
            ));
        }

        let modifier = if index + 1 == titles.len() {
            Modifier::BOLD
        } else {
            Modifier::DIM
        };

        spans.push(Span::styled(
            title.as_str(),
            Style::default().add_modifier(modifier),
        ));
    }

    Line::from(spans)
}
