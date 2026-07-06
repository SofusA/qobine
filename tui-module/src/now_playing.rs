use crate::ui::{
    HIGHLIGHT_STYLE, HIGHLIGHT_TEXT_STYLE, block, center, format_mseconds, format_seconds,
};
use controls_module::{Status, models::Track};
use ratatui::{layout::Flex, prelude::*, widgets::*};
use ratatui_image::{FilterType, Resize, StatefulImage, protocol::StatefulProtocol};
use tui_big_text::{BigText, PixelSize};

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

const BIG_TEXT_CHAR_WIDTH: u16 = 4;
const TITLE_CHAR_WIDTH: u16 = 8;
const IMAGE_INFO_GAP: u16 = 6;

pub fn render_full_screen(
    frame: &mut Frame,
    area: Rect,
    state: &mut NowPlayingState,
    disable_tui_album_cover: bool,
) {
    let track = match &state.playing_track {
        Some(t) => t,
        None => return,
    };

    let image_size = if disable_tui_album_cover {
        None
    } else {
        state.image.as_ref().map(|image| {
            image.0.size_for(
                Resize::Scale(Some(FilterType::Triangle)),
                Size::new(area.width * 2 / 5, area.height * 9 / 10),
            )
        })
    };

    let info_area = match image_size {
        Some(size) => {
            let info_width = size
                .width
                .max(50)
                .min(area.width.saturating_sub(size.width + IMAGE_INFO_GAP));

            let chunks = Layout::horizontal([
                Constraint::Length(size.width),
                Constraint::Length(info_width),
            ])
            .spacing(IMAGE_INFO_GAP)
            .flex(Flex::Center)
            .split(area);

            let image_area = center(
                chunks[0],
                Constraint::Length(size.width),
                Constraint::Length(size.height),
            );

            if let Some(image) = &mut state.image {
                frame.render_stateful_widget(
                    StatefulImage::new().resize(Resize::Scale(Some(FilterType::Triangle))),
                    image_area,
                    &mut image.0,
                );
            }

            Rect {
                x: chunks[1].x,
                y: image_area.y + 1,
                width: chunks[1].width,
                height: image_area.height.saturating_sub(2),
            }
        }
        None => center(area, Constraint::Percentage(60), Constraint::Percentage(80)),
    };

    let entity_lines = state
        .entity_title
        .as_ref()
        .map(|entity| wrap_big_text(entity, info_area.width / BIG_TEXT_CHAR_WIDTH))
        .unwrap_or_default();
    let entity_height = 3 * entity_lines.len().max(1) as u16;

    let title_lines = wrap_big_text(
        &track.title,
        info_area.width.saturating_sub(10) / TITLE_CHAR_WIDTH,
    );
    let title_height = 4 * title_lines.len() as u16;

    let top_spacer = (info_area.height / 2)
        .saturating_sub(entity_height + 3)
        .saturating_sub(title_height / 2);

    let rows = Layout::vertical([
        Constraint::Length(entity_height),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(top_spacer),
        Constraint::Length(title_height),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(info_area);

    if !entity_lines.is_empty() {
        frame.render_widget(
            BigText::builder()
                .pixel_size(PixelSize::Sextant)
                .lines(entity_lines.into_iter().map(Line::from).collect::<Vec<_>>())
                .centered()
                .build(),
            rows[0],
        );
    }

    if let Some(artist) = &track.artist_name {
        frame.render_widget(
            Paragraph::new(format!("by {artist}")).alignment(Alignment::Center),
            rows[1],
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", state.tracklist_position + 1),
                HIGHLIGHT_STYLE,
            ),
            Span::raw(" of "),
            Span::styled(format!(" {} ", state.tracklist_length), HIGHLIGHT_STYLE),
        ]))
        .alignment(Alignment::Center),
        rows[3],
    );

    let title_width = title_lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or_default() as u16
        * TITLE_CHAR_WIDTH;

    let title_area = center(
        rows[5],
        Constraint::Length(title_width),
        Constraint::Percentage(100),
    );

    let icon_area = Rect {
        x: title_area.x.saturating_sub(10),
        y: title_area.y + (title_height - 4) / 2,
        width: 8,
        height: 4,
    }
    .intersection(rows[5]);

    frame.render_widget(
        BigText::builder()
            .pixel_size(PixelSize::Quadrant)
            .lines(vec![Line::from(status_icon(state.status))])
            .right_aligned()
            .build(),
        icon_area,
    );

    frame.render_widget(
        BigText::builder()
            .pixel_size(PixelSize::HalfHeight)
            .style(HIGHLIGHT_TEXT_STYLE)
            .lines(title_lines.into_iter().map(Line::from).collect::<Vec<_>>())
            .centered()
            .build(),
        title_area,
    );

    render_progress(frame, rows[7], state.duration_ms, track);
}

fn wrap_big_text(text: &str, max_chars: u16) -> Vec<String> {
    if text.chars().count() <= max_chars as usize {
        return vec![text.to_string()];
    }

    let mut lines = vec![];
    let mut current = String::new();

    for word in text.split_whitespace() {
        if !current.is_empty()
            && current.chars().count() + 1 + word.chars().count() > max_chars as usize
        {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
        .into_iter()
        .map(|line| fit_big_text(&line, max_chars))
        .collect()
}

fn fit_big_text(text: &str, max_chars: u16) -> String {
    let max_chars = max_chars as usize;

    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{truncated}...")
}

fn render_progress(frame: &mut Frame, area: Rect, duration_ms: u32, track: &Track) {
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

fn status_icon(status: Status) -> &'static str {
    match status {
        Status::Playing => ">",
        Status::Paused => "||",
        Status::Buffering => "~",
    }
}

fn get_status(state: Status) -> &'static str {
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
