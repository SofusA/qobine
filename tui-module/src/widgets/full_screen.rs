use crate::{
    now_playing::{NowPlayingState, render_progress},
    ui::{HIGHLIGHT_STYLE, HIGHLIGHT_TEXT_STYLE, center},
};
use controls_module::Status;
use ratatui::{layout::Flex, prelude::*, widgets::*};
use ratatui_image::{FilterType, Resize, StatefulImage};
use tui_big_text::{BigText, PixelSize};

const BIG_TEXT_CHAR_WIDTH: u16 = 4;
const TITLE_CHAR_WIDTH: u16 = 8;
const IMAGE_INFO_GAP: u16 = 6;

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
        info_area.width.saturating_sub(18) / TITLE_CHAR_WIDTH,
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
        x: title_area.x.saturating_sub(18),
        y: title_area.y + (title_height - 4) / 2,
        width: 16,
        height: 4,
    }
    .intersection(rows[5]);

    frame.render_widget(
        BigText::builder()
            .pixel_size(PixelSize::HalfHeight)
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

fn status_icon(status: Status) -> &'static str {
    match status {
        Status::Playing => ">",
        Status::Paused => "||",
        Status::Buffering => "~",
    }
}
