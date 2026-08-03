use crate::{
    image_cache::{AppImage, ImageManager},
    ui::{
        ALBUM_COVER_GAP, ALBUM_COVER_HEIGHT, ALBUM_COVER_WIDTH, HIGHLIGHT_TEXT_STYLE,
        album_cover_area, block, format_mseconds, format_seconds,
    },
};
use controls_module::{Status, models::Track};
use num_traits::ToPrimitive;
use ratatui::{prelude::*, widgets::Paragraph};
use ratatui_image::StatefulImage;

#[derive(Default)]
pub struct NowPlayingState {
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
    state: &NowPlayingState,
    image_cache: &mut ImageManager,
) {
    let Some(track) = &state.playing_track else {
        return;
    };

    let block = block(Some(get_status(state.status)));
    let inner = block.inner(area);

    frame.render_widget(block, area);

    let image = track
        .image
        .as_ref()
        .and_then(|key| image_cache.get_mut(key));

    let can_render_cover = image.is_some()
        && inner.height >= ALBUM_COVER_HEIGHT
        && inner.width
            >= ALBUM_COVER_WIDTH
                .saturating_add(ALBUM_COVER_GAP)
                .saturating_add(1);

    let info_area = if can_render_cover {
        let [image_area, _gap, content_area] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(ALBUM_COVER_WIDTH),
                Constraint::Length(ALBUM_COVER_GAP),
                Constraint::Min(1),
            ])
            .areas(inner);

        if let Some(image_area) = album_cover_area(image_area)
            && let Some(AppImage { protocol, .. }) = image
        {
            frame.render_stateful_widget(StatefulImage::default(), image_area, protocol);
        }

        content_area
    } else {
        inner
    };

    let [content_area, progress_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .areas(info_area);

    let mut lines = vec![Line::from(track.title.as_str()).bold()];

    if let Some(artist) = &track.artist_name {
        lines.push(Line::from(artist.as_str()));
    }

    if let Some(entity) = &state.entity_title {
        lines.push(Line::from(entity.as_str()));
    }

    lines.push(Line::from(format!(
        "{} of {}",
        state.tracklist_position + 1,
        state.tracklist_length,
    )));

    render_progress(frame, progress_area, state.duration_ms, track);
    frame.render_widget(Text::from(lines), content_area);
}

pub fn render_progress(frame: &mut Frame, area: Rect, duration_ms: u32, track: &Track) {
    let total_ms = track.duration_seconds.saturating_mul(1000);
    let duration = duration_ms.min(total_ms);

    let ratio = f64::from(duration) / f64::from(track.duration_seconds * 1000);

    let [progress_area, gauge_area, duration_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(1),
            Constraint::Length(7),
        ])
        .areas(area);

    let current_time = Paragraph::new(format_mseconds(duration_ms)).alignment(Alignment::Left);

    let total_time =
        Paragraph::new(format_seconds(track.duration_seconds)).alignment(Alignment::Right);

    let gauge_str = smooth_gauge(ratio, gauge_area.width);

    let gauge = Paragraph::new(gauge_str).style(HIGHLIGHT_TEXT_STYLE);

    frame.render_widget(current_time, progress_area);
    frame.render_widget(gauge, gauge_area);
    frame.render_widget(total_time, duration_area);
}

pub const fn get_status(state: Status) -> &'static str {
    match state {
        Status::Playing => "Playing ⏵",
        Status::Paused => "Paused ⏸",
        Status::Buffering => "Buffering",
    }
}

fn smooth_gauge(ratio: f64, width: u16) -> String {
    const BLOCKS: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];

    let width = usize::from(width);
    let total = ratio.clamp(0.0, 1.0) * f64::from(width.to_u16().unwrap_or_default());

    let full = total.floor().to_usize().unwrap_or_default();
    let frac = ((total - full.to_f64().unwrap_or_default()) * 8.0)
        .round()
        .to_usize()
        .unwrap_or_default();

    let mut gauge = "█".repeat(full);

    if full < width {
        gauge.push_str(BLOCKS.get(frac).copied().unwrap_or_default());
    }

    gauge.push_str(&" ".repeat(width.saturating_sub(full.saturating_add(1))));

    gauge
}
