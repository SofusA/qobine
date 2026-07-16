use crate::{
    now_playing::{NowPlayingState, get_status, render_progress},
    ui::{HIGHLIGHT_TEXT_STYLE, center},
};
use ratatui::{layout::Flex, prelude::*, widgets::*};
use ratatui_image::{FilterType, Resize, StatefulImage};
use tui_big_text::{BigText, PixelSize};

const IMAGE_INFO_GAP: u16 = 6;
const CHAR_WIDTH: u16 = 4;
const CHAR_HEIGHT: u16 = 2;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut NowPlayingState,
    disable_tui_album_cover: bool,
) {
    let track = match &state.playing_track {
        Some(track) => track,
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
        .as_deref()
        .map(|entity| fit_big_text(entity, info_area.width, info_area.height.saturating_div(4)))
        .unwrap_or_default();

    let entity_height = entity_lines.len() as u16 * CHAR_HEIGHT;
    let title_budget = info_area.height.saturating_sub(entity_height + 7);
    let title_lines = fit_big_text(&track.title, info_area.width, title_budget);
    let title_height = title_lines.len() as u16 * CHAR_HEIGHT;

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
                .pixel_size(PixelSize::Octant)
                .lines(entity_lines.iter().map(Line::raw).collect::<Vec<_>>())
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
        Paragraph::new(Line::from(vec![Span::raw(format!(
            "{} of {}",
            state.tracklist_position + 1,
            state.tracklist_length
        ))]))
        .alignment(Alignment::Center),
        rows[3],
    );

    let title_width = title_lines
        .iter()
        .map(|line| line.chars().count() as u16)
        .max()
        .unwrap_or_default()
        .saturating_mul(CHAR_WIDTH)
        .min(rows[5].width);

    let title_area = center(
        rows[5],
        Constraint::Length(title_width),
        Constraint::Percentage(100),
    );

    if !title_lines.is_empty() {
        frame.render_widget(
            BigText::builder()
                .pixel_size(PixelSize::Octant)
                .style(HIGHLIGHT_TEXT_STYLE)
                .lines(title_lines.iter().map(Line::raw).collect::<Vec<_>>())
                .centered()
                .build(),
            title_area,
        );
    }

    let status_area = center(rows[6], Constraint::Percentage(100), Constraint::Length(1));

    frame.render_widget(
        Paragraph::new(get_status(state.status)).alignment(Alignment::Center),
        status_area,
    );

    render_progress(frame, rows[7], state.duration_ms, track);
}

fn fit_big_text(text: &str, max_width: u16, max_height: u16) -> Vec<String> {
    let max_chars = max_width / CHAR_WIDTH;
    let max_lines = max_height / CHAR_HEIGHT;

    if max_chars == 0 || max_lines == 0 {
        return Vec::new();
    }

    let mut lines = wrap_big_text(text, max_chars);

    if lines.len() > max_lines as usize {
        lines.truncate(max_lines as usize);

        if let Some(last) = lines.last_mut() {
            *last = truncate_with_dots(last, max_chars);
        }
    }

    lines
}

fn wrap_big_text(text: &str, max_chars: u16) -> Vec<String> {
    if max_chars == 0 {
        return Vec::new();
    }

    let max_chars = max_chars as usize;
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let word_length = word.chars().count();

        if word_length > max_chars {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }

            lines.push(truncate_with_dots(word, max_chars as u16));
            continue;
        }

        let required_length =
            current.chars().count() + usize::from(!current.is_empty()) + word_length;

        if required_length > max_chars {
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
}

fn truncate_with_dots(text: &str, max_chars: u16) -> String {
    let max_chars = max_chars as usize;

    if max_chars == 0 {
        return String::new();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let content_length = max_chars - 3;
    let truncated = text.chars().take(content_length).collect::<String>();

    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_stays_on_one_line() {
        assert_eq!(fit_big_text("Meddle", 40, 3), ["Meddle"]);
    }

    #[test]
    fn multi_word_text_wraps() {
        assert_eq!(
            fit_big_text("Depression Cherry", 40, 6),
            ["Depression", "Cherry"]
        );
    }

    #[test]
    fn long_word_is_truncated() {
        assert_eq!(fit_big_text("Supermassive", 24, 3), ["Sup..."]);
    }

    #[test]
    fn excess_lines_are_truncated() {
        assert_eq!(
            fit_big_text("one two three four five six seven", 36, 6),
            ["one two", "three", "four f..."]
        );
    }

    #[test]
    fn exact_fit_is_not_truncated() {
        assert_eq!(fit_big_text("one two", 28, 3), ["one two"]);
    }

    #[test]
    fn tiny_width_uses_visible_dots() {
        assert_eq!(truncate_with_dots("long", 2), "..");
    }
}
