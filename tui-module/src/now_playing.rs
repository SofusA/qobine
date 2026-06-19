use crate::ui::{HIGHLIGHT_TEXT_STYLE, block, format_duration, format_mseconds};
use controls_module::{Status, models::Track};
use player_module::RightTimerMode;
use ratatui::{prelude::*, widgets::Gauge};
use ratatui_image::{StatefulImage, protocol::StatefulProtocol};
use std::time::Instant;

#[derive(Default)]
pub struct NowPlayingState {
    pub image: Option<(StatefulProtocol, f32)>,
    pub playing_track: Option<Track>,
    pub tracklist_length: usize,
    pub tracklist_position: usize,
    pub status: Status,
    pub duration_ms: u32,
    pub position_anchor: Option<Instant>,
    pub progress_cells: u16,
    pub progress_drawn_eighths: usize,
    pub queue_total_seconds: u32,
    pub queue_after_current_seconds: u32,
}

impl NowPlayingState {
    fn track_ms(&self) -> u32 {
        self.playing_track
            .as_ref()
            .map_or(0, |t| t.duration_seconds * 1000)
    }

    pub fn displayed_ms(&self) -> u32 {
        let interpolated = match (matches!(self.status, Status::Playing), self.position_anchor) {
            (true, Some(anchor)) => self
                .duration_ms
                .saturating_add(anchor.elapsed().as_millis() as u32),
            _ => self.duration_ms,
        };
        interpolated.min(self.track_ms())
    }

    pub fn progress_eighths(&self) -> usize {
        let track_ms = self.track_ms();
        if track_ms == 0 || self.progress_cells == 0 {
            return 0;
        }
        let ratio = self.displayed_ms() as f64 / track_ms as f64;
        (ratio.clamp(0.0, 1.0) * (self.progress_cells as usize * 8) as f64).round() as usize
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut NowPlayingState,
    full_screen: bool,
    disable_tui_album_cover: bool,
    right_timer_mode: RightTimerMode,
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

    let lines = vec![
        Line::from(track.title.clone()).style(Style::new().bold()),
        Line::from(track.artist_name.clone().unwrap_or_default()),
        Line::from(track.album_title.clone().unwrap_or_default()),
        Line::from(format!(
            "{} of {}",
            state.tracklist_position + 1,
            state.tracklist_length
        )),
    ];

    let displayed_ms = state.displayed_ms();
    let remaining_track = track.duration_seconds.saturating_sub(displayed_ms / 1000);
    let right_label = match right_timer_mode {
        RightTimerMode::TrackLength => format_duration(track.duration_seconds),
        RightTimerMode::TrackRemaining => format!("-{}", format_duration(remaining_track)),
        RightTimerMode::QueueRemaining => format!(
            "-{}",
            format_duration(remaining_track + state.queue_after_current_seconds)
        ),
        RightTimerMode::QueueTotal => format_duration(state.queue_total_seconds),
    };

    let cells = render_progress_bar(
        frame,
        info_chunks[1],
        displayed_ms,
        track.duration_seconds,
        &right_label,
    );
    state.progress_cells = cells;
    frame.render_widget(Text::from(lines), info_chunks[0]);
}

fn render_progress_bar(
    frame: &mut Frame,
    area: Rect,
    elapsed_ms: u32,
    duration_secs: u32,
    right_label: &str,
) -> u16 {
    let elapsed = format_mseconds(elapsed_ms);
    let total = right_label;

    let [left, gauge_area, right] = Layout::horizontal([
        Constraint::Length(elapsed.chars().count() as u16 + 1),
        Constraint::Min(1),
        Constraint::Length(total.chars().count() as u16 + 2),
    ])
    .areas(area);

    let ratio = if duration_secs == 0 {
        0.0
    } else {
        (elapsed_ms as f64 / (duration_secs * 1000) as f64).clamp(0.0, 1.0)
    };

    frame.render_widget(Line::from(elapsed).style(Style::new().dim()), left);
    frame.render_widget(
        Gauge::default()
            .ratio(ratio)
            .gauge_style(HIGHLIGHT_TEXT_STYLE)
            .label("")
            .use_unicode(true),
        gauge_area,
    );
    frame.render_widget(
        Line::from(format!("{total} "))
            .style(Style::new().dim())
            .right_aligned(),
        right,
    );

    gauge_area.width
}

fn get_status(state: Status) -> String {
    match state {
        Status::Playing => "Playing ⏵".to_string(),
        Status::Paused => "Paused ⏸ ".to_string(),
        Status::Buffering => "Buffering".to_string(),
    }
}
