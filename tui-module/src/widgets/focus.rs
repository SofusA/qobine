use crate::{
    image_cache::ImageManager,
    now_playing::{NowPlayingState, get_status, render_progress},
    ui::{HIGHLIGHT_TEXT_STYLE, center},
};
use num_traits::ToPrimitive;
use ratatui::{layout::Flex, prelude::*, widgets::Paragraph};
use ratatui_image::{FilterType, Resize, StatefulImage};
use tui_big_text::{BigText, PixelSize};

const IMAGE_INFO_GAP: u16 = 6;
const CHAR_WIDTH: u16 = 4;
const CHAR_HEIGHT: u16 = 2;

pub fn render(frame: &mut Frame, state: &NowPlayingState, image_cache: &mut ImageManager) {
    let area = frame.area();
    let Some(track) = &state.playing_track else {
        return;
    };

    let image = track.image.as_ref().and_then(|x| image_cache.get_mut(x));

    let image_size = image.as_deref().map(|image| {
        image.protocol.size_for(
            Resize::Scale(Some(FilterType::Triangle)),
            Size::new(
                area.width
                    .saturating_mul(2)
                    .checked_div(5)
                    .unwrap_or_default(),
                area.height
                    .saturating_mul(9)
                    .checked_div(10)
                    .unwrap_or_default(),
            ),
        )
    });

    let info_area = match image_size {
        Some(size) => {
            let info_width = size.width.max(50).min(
                area.width
                    .saturating_sub(size.width.saturating_add(IMAGE_INFO_GAP)),
            );

            let [image_area, info_area] = Layout::horizontal([
                Constraint::Length(size.width),
                Constraint::Length(info_width),
            ])
            .spacing(IMAGE_INFO_GAP)
            .flex(Flex::Center)
            .areas(area);

            let image_area = center(
                image_area,
                Constraint::Length(size.width),
                Constraint::Length(size.height),
            );

            if let Some(image) = image {
                frame.render_stateful_widget(
                    StatefulImage::new().resize(Resize::Scale(Some(FilterType::Triangle))),
                    image_area,
                    &mut image.protocol,
                );
            }

            Rect {
                x: info_area.x,
                y: image_area.y.saturating_add(1),
                width: info_area.width,
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

    let entity_height = entity_lines
        .len()
        .to_u16()
        .unwrap_or_default()
        .saturating_mul(CHAR_HEIGHT);
    let title_budget = info_area
        .height
        .saturating_sub(entity_height.saturating_add(7));
    let title_lines = fit_big_text(&track.title, info_area.width, title_budget);
    let title_height = title_lines
        .len()
        .to_u16()
        .unwrap_or_default()
        .saturating_mul(CHAR_HEIGHT);

    let top_spacer = (info_area.height / 2)
        .saturating_sub(entity_height.saturating_add(3))
        .saturating_sub(title_height / 2);

    let [
        entity_area,
        artist_area,
        _spacer,
        of_area,
        _spacer_2,
        title_area,
        status_area,
        _spacer_3,
        gauge_area,
    ] = Layout::vertical([
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
    .areas(info_area);

    if !entity_lines.is_empty() {
        frame.render_widget(
            BigText::builder()
                .pixel_size(PixelSize::Octant)
                .lines(entity_lines.iter().map(Line::raw).collect::<Vec<_>>())
                .centered()
                .build(),
            entity_area,
        );
    }

    if let Some(artist) = &track.artist_name {
        frame.render_widget(
            Paragraph::new(format!("by {artist}")).alignment(Alignment::Center),
            artist_area,
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::raw(format!(
            "{} of {}",
            state.tracklist_position.saturating_add(1),
            state.tracklist_length
        ))]))
        .alignment(Alignment::Center),
        of_area,
    );

    let title_width = title_lines
        .iter()
        .map(|line| line.chars().count().to_u16().unwrap_or_default())
        .max()
        .unwrap_or_default()
        .saturating_mul(CHAR_WIDTH)
        .min(title_area.width);

    let title_area = center(
        title_area,
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

    let status_area = center(
        status_area,
        Constraint::Percentage(100),
        Constraint::Length(1),
    );

    frame.render_widget(
        Paragraph::new(get_status(state.status)).alignment(Alignment::Center),
        status_area,
    );

    render_progress(frame, gauge_area, state.duration_ms, track);
}

fn fit_big_text(text: &str, max_width: u16, max_height: u16) -> Vec<String> {
    let max_chars = max_width / CHAR_WIDTH;
    let max_lines = max_height / CHAR_HEIGHT;

    if max_chars == 0 || max_lines == 0 {
        return Vec::new();
    }

    let mut lines = wrap_big_text(text, max_chars);

    if lines.len() > max_lines.to_usize().unwrap_or_default() {
        lines.truncate(max_lines.to_usize().unwrap_or_default());

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

    let max_chars_usize = max_chars.to_usize().unwrap_or_default();
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let word_length = word.chars().count();

        if word_length > max_chars_usize {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }

            lines.push(truncate_with_dots(word, max_chars));
            continue;
        }

        let required_length = current
            .chars()
            .count()
            .saturating_add(usize::from(!current.is_empty()))
            .saturating_add(word_length);

        if required_length > max_chars_usize {
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
    let max_chars = max_chars.to_usize().unwrap_or_default();

    if max_chars == 0 {
        return String::new();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let content_length = max_chars.saturating_sub(3);
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
