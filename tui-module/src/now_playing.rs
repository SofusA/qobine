use crate::ui::{HIGHLIGHT_TEXT_STYLE, block, format_mseconds, format_seconds};
use controls_module::{Status, models::Track};
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
    disable_tui_album_cover: bool,
) {
    let track = match &state.playing_track {
        Some(t) => t,
        None => return,
    };

    let block = block(Some(get_status(state.status)));

    let length = state
        .image
        .as_ref()
        .map(|image| image.1 * (area.height * 2 - 1) as f32)
        .map(|x| x as u16)
        .unwrap_or(0);

    let chunks = if disable_tui_album_cover {
        vec![block.inner(area)]
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(length), Constraint::Min(1)])
            .split(block.inner(area))
            .to_vec()
    };

    frame.render_widget(block, area);

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

    lines.push(Line::from(track.title.as_str()).bold());

    if let Some(artist) = &track.artist_name {
        lines.push(Line::from(artist.as_str()));
    }

    if let Some(entity) = &state.entity_title {
        lines.push(Line::from(entity.as_str()));
    }

    lines.push(Line::from(format!(
        "{} of {}",
        state.tracklist_position + 1,
        state.tracklist_length
    )));

    render_progress(frame, info_chunks[1], state.duration_ms, track);
    frame.render_widget(Text::from(lines), info_chunks[0]);
}

pub(crate) fn render_progress(frame: &mut Frame, area: Rect, duration_ms: u32, track: &Track) {
    let total_ms = track.duration_seconds.saturating_mul(1000);
    let duration = duration_ms.min(total_ms);

    let ratio = duration as f64 / (track.duration_seconds * 1000) as f64;

    let progress_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(1),
            Constraint::Length(7),
        ])
        .split(area);

    let current_time = Paragraph::new(format_mseconds(duration_ms)).alignment(Alignment::Left);

    let total_time =
        Paragraph::new(format_seconds(track.duration_seconds)).alignment(Alignment::Right);

    let gauge_width = progress_chunks[1].width as usize;

    let gauge_str = smooth_gauge(ratio, gauge_width);

    let gauge = Paragraph::new(gauge_str).style(HIGHLIGHT_TEXT_STYLE);

    frame.render_widget(current_time, progress_chunks[0]);
    frame.render_widget(gauge, progress_chunks[1]);
    frame.render_widget(total_time, progress_chunks[2]);
}

pub fn get_status(state: Status) -> &'static str {
    match state {
        Status::Playing => "Playing ⏵",
        Status::Paused => "Paused ⏸",
        Status::Buffering => "Buffering",
    }
}

fn smooth_gauge(ratio: f64, width: usize) -> String {
    let blocks = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];

    let total = ratio * width as f64;
    let full = total.floor() as usize;
    let frac = ((total - full as f64) * 8.0).round() as usize;

    let mut s = String::new();

    for _ in 0..full {
        s.push('█');
    }

    if full < width {
        s.push_str(blocks[frac]);
    }

    let remaining = width.saturating_sub(full + 1);

    for _ in 0..remaining {
        s.push(' ');
    }

    s
}
