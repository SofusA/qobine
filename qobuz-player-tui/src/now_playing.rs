use crate::ui::{HIGHLIGHT_TEXT_STYLE, block, format_mseconds, format_seconds};
use qobuz_player_controls::{Status, models::Track};
use ratatui::{prelude::*, widgets::*};
use ratatui_image::{StatefulImage, protocol::StatefulProtocol};

#[derive(Default)]
pub struct NowPlayingState {
    pub image: Option<(StatefulProtocol, f32)>,
    pub entity_title: Option<String>,
    pub playing_track: Option<Track>,
    pub tracklist_length: usize,
    pub tracklist_position: usize,
    pub status: Status,
    pub duration_ms: u32,
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut NowPlayingState,
    full_screen: bool,
    disable_tui_album_cover: bool,
) {
    let track = match &state.playing_track {
        Some(t) => t,
        None => return,
    };

    let title = get_status(state.status).to_string();
    let block = block(Some(&title));

    let length = state
        .image
        .as_ref()
        .map(|image| image.1 * (area.height * 2 - 1) as f32)
        .map(|x| x as u16)
        .unwrap_or(0);

    let chunks = match disable_tui_album_cover {
        true => std::rc::Rc::new([block.inner(area)]),
        false => Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(length), Constraint::Min(1)])
            .split(block.inner(area)),
    };

    if !full_screen {
        frame.render_widget(block, area);
    }

    if let Some(image) = &mut state.image
        && !disable_tui_album_cover
    {
        let stateful_image = StatefulImage::default();
        frame.render_stateful_widget(stateful_image, chunks[0], &mut image.0);
    }

    let info_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(*chunks.last().unwrap());

    let mut lines = vec![];

    if let Some(entity) = &state.entity_title {
        lines.push(Line::from(entity.clone()).style(Style::new().bold()));
    }

    if let Some(artist) = &track.artist_name {
        lines.push(Line::from(artist.clone()));
    }

    lines.push(Line::from(track.title.clone()));

    lines.push(Line::from(format!(
        "{} of {}",
        state.tracklist_position + 1,
        state.tracklist_length
    )));

    let duration = if state.duration_ms < track.duration_seconds * 1000 {
        state.duration_ms
    } else {
        track.duration_seconds * 1000
    };

    let ratio = duration as f64 / (track.duration_seconds * 1000) as f64;

    let label = format!(
        "{} / {}",
        format_mseconds(state.duration_ms),
        format_seconds(track.duration_seconds),
    );

    let gauge = Gauge::default()
        .ratio(ratio)
        .gauge_style(HIGHLIGHT_TEXT_STYLE)
        .label(label);

    frame.render_widget(gauge, info_chunks[1]);
    frame.render_widget(Text::from(lines), info_chunks[0]);
}

fn get_status(state: Status) -> String {
    match state {
        Status::Playing => "Playing ⏵".to_string(),
        Status::Paused => "Paused ⏸ ".to_string(),
        Status::Buffering => "Buffering".to_string(),
        Status::Stopped => "Stopped ⏹️".to_string(),
    }
}
