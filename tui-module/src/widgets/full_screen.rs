use crate::{
    now_playing::{NowPlayingState, render_progress},
    ui::{HIGHLIGHT_STYLE, HIGHLIGHT_TEXT_STYLE, center},
};
use controls_module::Status;
use ratatui::{layout::Flex, prelude::*, widgets::*};
use ratatui_image::{FilterType, Resize, StatefulImage};
use tui_big_text::{BigText, PixelSize};

const IMAGE_INFO_GAP: u16 = 6;
const ICON_CHARS: u16 = 2;

#[derive(Clone, Copy, PartialEq, Debug)]
struct BigTextSize {
    pixel_size: PixelSize,
    char_width: u16,
    char_height: u16,
}

const TITLE_SIZES: [BigTextSize; 2] = [
    BigTextSize {
        pixel_size: PixelSize::HalfHeight,
        char_width: 8,
        char_height: 4,
    },
    BigTextSize {
        pixel_size: PixelSize::Sextant,
        char_width: 4,
        char_height: 3,
    },
];

const ENTITY_SIZES: [BigTextSize; 1] = [BigTextSize {
    pixel_size: PixelSize::Sextant,
    char_width: 4,
    char_height: 3,
}];

#[derive(PartialEq, Debug)]
enum FittedText {
    Big {
        size: BigTextSize,
        lines: Vec<String>,
    },
    Plain(String),
}

impl FittedText {
    fn height(&self) -> u16 {
        match self {
            Self::Big { size, lines } => size.char_height * lines.len() as u16,
            Self::Plain(_) => 1,
        }
    }

    fn width(&self) -> u16 {
        match self {
            Self::Big { size, lines } => {
                lines
                    .iter()
                    .map(|line| line.chars().count())
                    .max()
                    .unwrap_or_default() as u16
                    * size.char_width
            }
            Self::Plain(text) => text.chars().count() as u16,
        }
    }
}

fn fit_text(
    text: &str,
    max_width: u16,
    max_height: u16,
    sizes: &[BigTextSize],
    reserved_chars: u16,
) -> FittedText {
    for size in sizes {
        let max_chars = (max_width / size.char_width).saturating_sub(reserved_chars);
        if max_chars == 0 {
            continue;
        }

        let lines = wrap_big_text(text, max_chars);
        let fits_width = lines
            .iter()
            .all(|line| line.chars().count() <= max_chars as usize);
        let fits_height = lines.len() as u16 * size.char_height <= max_height;

        if fits_width && fits_height {
            return FittedText::Big { size: *size, lines };
        }
    }

    FittedText::Plain(ellipsize(text, max_width))
}

fn ellipsize(text: &str, max_chars: u16) -> String {
    if text.chars().count() <= max_chars as usize {
        return text.to_string();
    }

    let truncated: String = text
        .chars()
        .take((max_chars as usize).saturating_sub(1))
        .collect();
    format!("{truncated}…")
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

    let entity = state.entity_title.as_ref().map(|entity| {
        fit_text(
            entity,
            info_area.width,
            info_area.height / 4,
            &ENTITY_SIZES,
            0,
        )
    });
    let entity_height = entity.as_ref().map(FittedText::height).unwrap_or(0);

    let title_budget = info_area.height.saturating_sub(entity_height + 6);
    let title = fit_text(
        &track.title,
        info_area.width,
        title_budget,
        &TITLE_SIZES,
        ICON_CHARS,
    );
    let title_height = title.height();

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

    match &entity {
        Some(FittedText::Big { size, lines }) => {
            frame.render_widget(
                BigText::builder()
                    .pixel_size(size.pixel_size)
                    .lines(lines.iter().map(Line::raw).collect::<Vec<_>>())
                    .centered()
                    .build(),
                rows[0],
            );
        }
        Some(FittedText::Plain(text)) => {
            frame.render_widget(
                Paragraph::new(text.as_str()).alignment(Alignment::Center),
                rows[0],
            );
        }
        None => {}
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

    let title_area = center(
        rows[5],
        Constraint::Length(title.width()),
        Constraint::Percentage(100),
    );

    match &title {
        FittedText::Big { size, lines } => {
            let icon_width = ICON_CHARS * size.char_width;
            let icon_area = Rect {
                x: title_area.x.saturating_sub(icon_width + 2),
                y: title_area.y + title_height.saturating_sub(size.char_height) / 2,
                width: icon_width,
                height: size.char_height,
            }
            .intersection(rows[5]);

            frame.render_widget(
                BigText::builder()
                    .pixel_size(size.pixel_size)
                    .lines(vec![Line::from(status_icon(state.status))])
                    .right_aligned()
                    .build(),
                icon_area,
            );

            frame.render_widget(
                BigText::builder()
                    .pixel_size(size.pixel_size)
                    .style(HIGHLIGHT_TEXT_STYLE)
                    .lines(lines.iter().map(Line::raw).collect::<Vec<_>>())
                    .centered()
                    .build(),
                title_area,
            );
        }
        FittedText::Plain(text) => {
            let text = ellipsize(
                &format!("{} {}", status_icon(state.status), text),
                info_area.width,
            );
            frame.render_widget(
                Paragraph::new(text)
                    .style(HIGHLIGHT_TEXT_STYLE)
                    .bold()
                    .alignment(Alignment::Center),
                rows[5],
            );
        }
    }

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
}

fn status_icon(status: Status) -> &'static str {
    match status {
        Status::Playing => ">",
        Status::Paused => "||",
        Status::Buffering => "~",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(fitted: &FittedText) -> Vec<&str> {
        match fitted {
            FittedText::Big { lines, .. } => lines.iter().map(String::as_str).collect(),
            FittedText::Plain(_) => panic!("expected big text"),
        }
    }

    fn pixel_size(fitted: &FittedText) -> PixelSize {
        match fitted {
            FittedText::Big { size, .. } => size.pixel_size,
            FittedText::Plain(_) => panic!("expected big text"),
        }
    }

    #[test]
    fn multi_word_title_falls_back_to_smaller_size_instead_of_overflowing() {
        let fitted = fit_text("Days of Candy", 50, 8, &TITLE_SIZES, ICON_CHARS);

        assert_eq!(pixel_size(&fitted), PixelSize::Sextant);
        assert_eq!(lines(&fitted), ["Days of", "Candy"]);
    }

    #[test]
    fn single_word_falls_back_instead_of_truncating() {
        let fitted = fit_text("Sparks", 50, 12, &TITLE_SIZES, ICON_CHARS);

        assert_eq!(pixel_size(&fitted), PixelSize::Sextant);
        assert_eq!(lines(&fitted), ["Sparks"]);
    }

    #[test]
    fn short_title_in_large_area_uses_the_largest_size() {
        let fitted = fit_text("Meddle", 200, 20, &TITLE_SIZES, ICON_CHARS);

        assert_eq!(pixel_size(&fitted), PixelSize::HalfHeight);
        assert_eq!(lines(&fitted), ["Meddle"]);
    }

    #[test]
    fn long_single_word_in_tiny_area_falls_back_to_plain_text() {
        let fitted = fit_text("Supermassive", 30, 6, &TITLE_SIZES, ICON_CHARS);

        assert_eq!(fitted, FittedText::Plain("Supermassive".to_string()));
    }

    #[test]
    fn plain_fallback_ellipsizes_with_a_single_character() {
        let fitted = fit_text("An Ending (Ascent)", 10, 2, &TITLE_SIZES, ICON_CHARS);

        assert_eq!(fitted, FittedText::Plain("An Ending…".to_string()));
    }

    #[test]
    fn entity_wraps_at_sextant() {
        let fitted = fit_text("Depression Cherry", 50, 8, &ENTITY_SIZES, 0);

        assert_eq!(pixel_size(&fitted), PixelSize::Sextant);
        assert_eq!(lines(&fitted), ["Depression", "Cherry"]);
    }

    #[test]
    fn wrap_never_splits_words_and_respects_width() {
        let wrapped = wrap_big_text("one two three four five", 9);

        assert_eq!(wrapped, ["one two", "three", "four five"]);
        assert!(wrapped.iter().all(|line| line.chars().count() <= 9));
    }
}
