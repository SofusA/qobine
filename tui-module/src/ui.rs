use image::load_from_memory;
use player_module::notification::Notification;
use ratatui::{layout::Flex, prelude::*, widgets::*};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use tui_input::Input;

use crate::{
    app::{App, AppState, Tab},
    now_playing::{self},
};

pub const HIGHLIGHT_STYLE: Style = Style::new().white().on_blue();
pub const HIGHLIGHT_TEXT_STYLE: Style = Style::new().blue();
pub const SELECTED_STYLE: Style = Style::new().fg(Color::Cyan);
pub const COLUMN_SPACING: u16 = 2;

impl App {
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        self.render_inner(frame);

        match self.app_state {
            AppState::Normal | AppState::Popup(_) => {}
            AppState::Help => {
                render_help(frame);
            }
            AppState::ConnectPopup(selected) => {
                let available_devices: Vec<String> =
                    self.connect_available_devices.borrow().to_vec();
                let active_device: String = self.connect_active_device.borrow().to_string();
                render_connect(frame, available_devices, active_device, selected);
            }
        }

        self.render_notifications(frame, area);
    }

    fn render_inner(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let hide_album_cover = self.disable_tui_album_cover;

        if self.full_screen {
            let area = center(area, Constraint::Percentage(80), Constraint::Length(10));
            now_playing::render(
                frame,
                area,
                &mut self.now_playing,
                self.full_screen,
                hide_album_cover,
            );
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(10),
            ])
            .split(area);

        let labels: Vec<String> = Tab::VALUES
            .iter()
            .enumerate()
            .map(|(i, tab)| format!("[{}] {}", i + 1, tab))
            .collect();

        let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();

        let tabs = tab_bar(
            label_refs,
            Tab::VALUES
                .iter()
                .position(|tab| tab == &self.current_screen)
                .unwrap_or(0),
        )
        .block(block(None));

        frame.render_widget(tabs, chunks[0]);

        if self.now_playing.playing_track.is_some() {
            now_playing::render(
                frame,
                chunks[2],
                &mut self.now_playing,
                self.full_screen,
                hide_album_cover,
            );
        }

        let tab_content_area = if self.now_playing.playing_track.is_some() {
            chunks[1]
        } else {
            chunks[1].union(chunks[2])
        };

        let favorite_ids = &self.favorite_ids;
        match self.current_screen {
            Tab::Favorites => self.favorites.render(frame, tab_content_area, favorite_ids),
            Tab::Search => self.search.render(frame, tab_content_area, favorite_ids),
            Tab::Queue => self
                .queue
                .render(frame, tab_content_area, &favorite_ids.tracks),
            Tab::Discover => self.discover.render(frame, tab_content_area, favorite_ids),
            Tab::Genres => self.genres.render(frame, tab_content_area, favorite_ids),
            Tab::Preferences => self.preferences.render(frame, tab_content_area),
        }

        if let AppState::Popup(popups) = &mut self.app_state {
            for popup in popups {
                popup.render(frame, &self.favorite_ids);
            }
        }
    }

    fn render_notifications(&self, frame: &mut Frame, area: Rect) {
        let notifications: Vec<_> = self.notifications.notifications();

        if notifications.is_empty() {
            return;
        }

        let messages = notifications
            .into_iter()
            .map(|notification| match notification {
                Notification::Error(msg) => ("Error", msg, Color::Red),
                Notification::Warning(msg) => ("Warning", msg, Color::Yellow),
                Notification::Success(msg) => ("Success", msg, Color::Green),
                Notification::Info(msg) => ("Info", msg, Color::Blue),
            });

        let inner_width = 60;
        let box_width = inner_width;
        let x = area.x + area.width.saturating_sub(box_width);
        let mut y = area.y;

        for msg in messages.rev() {
            let lines = (msg.1.len() as u16).div_ceil(inner_width);
            let box_height = lines + 2;

            if y + box_height > area.y + area.height {
                break;
            }

            let rect = Rect {
                x,
                y,
                width: box_width,
                height: box_height,
            };

            let paragraph = Paragraph::new(msg.1.as_str())
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .border_style(msg.2)
                        .border_type(BorderType::Rounded)
                        .title(msg.0)
                        .title_alignment(Alignment::Center)
                        .title_style(msg.2),
                )
                .wrap(Wrap { trim: true });

            frame.render_widget(Clear, rect);
            frame.render_widget(paragraph, rect);

            y += box_height;
        }
    }
}

pub fn center(area: Rect, horizontal: Constraint, vertical: Constraint) -> Rect {
    let [area] = Layout::horizontal([horizontal])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([vertical]).flex(Flex::Center).areas(area);
    area
}

pub fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(h) / 2),
            Constraint::Length(h),
            Constraint::Min(0),
        ])
        .split(area);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(w) / 2),
            Constraint::Length(w),
            Constraint::Min(0),
        ])
        .split(vertical[1]);

    horizontal[1]
}

fn render_connect(
    frame: &mut Frame,
    available_devices: Vec<String>,
    active_device: String,
    selected_device: usize,
) {
    let title = "Select output Connect device";

    let items: Vec<ListItem> = available_devices
        .iter()
        .map(|device| {
            if *device == active_device {
                ListItem::new(Line::from(vec![
                    Span::raw(device.clone()),
                    Span::styled(" (active)", Style::new().dim()),
                ]))
            } else {
                ListItem::new(device.clone())
            }
        })
        .collect();

    let content_width = available_devices
        .iter()
        .map(|d| {
            if *d == active_device {
                d.len() + " (active)".len()
            } else {
                d.len()
            }
        })
        .max()
        .unwrap_or(0);

    let width = std::cmp::max(content_width, title.len());

    let area = center(
        frame.area(),
        Constraint::Length(width as u16 + 6),
        Constraint::Length(items.len() as u16 + 2),
    );

    let list = List::new(items)
        .block(block(Some(title)))
        .highlight_style(HIGHLIGHT_STYLE)
        .highlight_symbol("❯ ");

    let mut state = ListState::default();
    state.select(Some(selected_device));

    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_help(frame: &mut Frame) {
    let rows = [
        ["Toggle focus mode", "F"],
        ["Next song", "n"],
        ["Previous song", "p"],
        ["Jump forward", "f"],
        ["Jump backwards", "b"],
        ["Edit filter", "e"],
        ["Stop edit filter", "escape"],
        ["Select in list", "Up/Down"],
        ["Select selected item", "Enter"],
        ["Cycle subgroup", "Left/right"],
        ["Add to queue", "B"],
        ["Shuffle tracks", "S"],
        ["Play next", "N"],
        ["Delete from queue", "D"],
        ["Move up in queue", "u"],
        ["Move down in queue", "d"],
        ["Remove from favorites", "U"],
        ["Add to favorites", "A"],
        ["Create playlist", "C (playlist page)"],
        ["Unfavorite (delete) playlist", "U (playlist page)"],
        ["Add track to playlist", "a"],
        ["Move playlist track up", "u"],
        ["Move playlist track down", "d"],
        ["Selected info", "i"],
        ["Currently playing album info", "I"],
        ["Select Connect device (if configured)", "c"],
        ["Exit", "q"],
    ];

    let max_left = rows.iter().map(|x| x[0].len()).max().expect("infallible");
    let max_right = rows.iter().map(|x| x[1].len()).max().expect("infallible");
    let max = std::cmp::max(max_left, max_right);
    let max = max + max;

    let rows: Vec<_> = rows.into_iter().map(Row::new).collect();

    let area = center(
        frame.area(),
        Constraint::Length(max as u16 + 2 + 1),
        Constraint::Length(rows.len() as u16 + 2),
    );

    let block = block(Some("Help"));

    let table = Table::default().rows(rows).block(block);

    frame.render_widget(Clear, area);
    frame.render_widget(table, area);
}

pub fn render_input(input: &Input, editing: bool, area: Rect, frame: &mut Frame, title: &str) {
    let width = area.width.max(3) - 3;
    let scroll = input.visual_scroll(width as usize);
    let style = match editing {
        true => HIGHLIGHT_TEXT_STYLE,
        _ => Style::default(),
    };

    let input_paragraph = Paragraph::new(input.value())
        .style(style)
        .scroll((0, scroll as u16))
        .block(block(Some(title)));

    frame.render_widget(input_paragraph, area);

    if editing {
        let x = input.visual_cursor().max(scroll) - scroll + 1;
        frame.set_cursor_position((area.x + x as u16, area.y + 1))
    }
}

pub fn block(title: Option<&str>) -> Block<'_> {
    let mut block = Block::bordered()
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded);

    if let Some(title) = title {
        block = block.title(format!(" {title} "));
    }

    block
}

pub fn basic_list_table<'a>(rows: Vec<Row<'a>>, focus: bool) -> Table<'a> {
    Table::new(rows, [Constraint::Min(1)])
        .row_highlight_style(if focus {
            HIGHLIGHT_STYLE
        } else {
            SELECTED_STYLE
        })
        .column_spacing(COLUMN_SPACING)
}

pub fn tab_bar<'a>(tabs: Vec<&'a str>, selected: usize) -> Tabs<'a> {
    Tabs::new(tabs)
        .not_underlined()
        .highlight_style(HIGHLIGHT_STYLE)
        .divider(symbols::line::VERTICAL)
        .select(selected)
}

pub fn sidebar<'a>(tabs: Vec<&'a str>, focused: bool) -> (List<'a>, u16) {
    let width = tabs.iter().map(|tab| tab.len()).max().unwrap_or_default() as u16 + 3;

    let items = tabs.into_iter().map(ListItem::new).collect::<Vec<_>>();

    let highlight_style = match focused {
        false => SELECTED_STYLE,
        true => HIGHLIGHT_STYLE,
    };

    let border_style = match focused {
        false => Style::default(),
        true => Style::default().blue(),
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(border_style),
        )
        .highlight_style(highlight_style);

    (list, width)
}

pub fn mark_explicit_and_hifi(
    title: String,
    explicit: bool,
    hires_available: bool,
) -> Line<'static> {
    let mut parts: Vec<Span<'static>> = Vec::new();

    parts.push(Span::raw(title));

    if explicit {
        parts.push(Span::raw(" "));
        parts.push(Span::styled(
            "\u{f0b0c}",
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    if hires_available {
        parts.push(Span::raw(" "));
        parts.push(Span::styled(
            "\u{f0435}",
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    Line::from(parts)
}

pub fn mark_favorite(line: Line<'static>, is_favorite: bool) -> Line<'static> {
    if !is_favorite {
        return line;
    }

    let mut spans = line.spans;
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        "\u{f004}",
        Style::default().add_modifier(Modifier::DIM),
    ));
    Line::from(spans)
}

pub fn mark_as_owned(title: String, owned: bool) -> Line<'static> {
    let mut parts: Vec<Span<'static>> = Vec::new();

    parts.push(Span::raw(title));

    if owned {
        parts.push(Span::raw(" "));
        parts.push(Span::styled(
            "\u{f007}",
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    Line::from(parts)
}

pub fn format_duration(secs: u32) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub fn format_mseconds(mseconds: u32) -> String {
    let seconds = mseconds / 1000;

    format_seconds(seconds)
}

pub fn format_seconds(seconds: u32) -> String {
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

pub async fn fetch_image(image_url: &str) -> Option<(StatefulProtocol, f32)> {
    let client = reqwest::Client::new();
    let response = client.get(image_url).send().await.ok()?;
    let img_bytes = response.bytes().await.ok()?;

    tokio::task::spawn_blocking(move || {
        let image = load_from_memory(&img_bytes).ok()?;
        let ratio = image.width() as f32 / image.height() as f32;
        let picker = Picker::from_query_stdio().ok()?;
        Some((picker.new_resize_protocol(image), ratio))
    })
    .await
    .ok()?
}
