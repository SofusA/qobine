use num_traits::ToPrimitive;
use player_module::notification::Notification;
use ratatui::{
    layout::Flex,
    prelude::*,
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Table, Tabs,
        Wrap,
    },
};
use tui_input::Input;

use crate::{
    app::{App, AppState, Tab},
    image_cache::ImageManager,
    now_playing::{self, NowPlayingState},
    widgets::focus,
};

pub const HIGHLIGHT_STYLE: Style = Style::new().white().on_blue();
pub const HIGHLIGHT_TEXT_STYLE: Style = Style::new().blue();
pub const SELECTED_STYLE: Style = Style::new().fg(Color::Cyan);
pub const COLUMN_SPACING: u16 = 2;

pub const ALBUM_COVER_WIDTH: u16 = 20;
pub const ALBUM_COVER_HEIGHT: u16 = 9;
pub const ALBUM_COVER_GAP: u16 = 2;

pub const fn album_cover_area(area: Rect) -> Option<Rect> {
    if area.width < ALBUM_COVER_WIDTH || area.height < ALBUM_COVER_HEIGHT {
        return None;
    }

    Some(Rect::new(
        area.x,
        area.y,
        ALBUM_COVER_WIDTH,
        ALBUM_COVER_HEIGHT,
    ))
}

impl App {
    pub fn render(&mut self, frame: &mut Frame) {
        match &mut self.state {
            AppState::Normal => {
                let tab_area =
                    render_now_playing_bar(frame, &self.now_playing, &mut self.image_cache);
                self.render_inner(frame, tab_area);
            }
            AppState::Help => {
                let tab_area =
                    render_now_playing_bar(frame, &self.now_playing, &mut self.image_cache);
                self.render_inner(frame, tab_area);
                render_help(frame, tab_area);
            }
            AppState::ConnectOverlay(selected) => {
                let tab_area =
                    render_now_playing_bar(frame, &self.now_playing, &mut self.image_cache);
                let available_devices: Vec<String> =
                    self.connect_available_devices.borrow().to_vec();
                let active_device: String = self.connect_active_device.borrow().to_string();
                render_connect(
                    frame,
                    tab_area,
                    &available_devices,
                    &active_device,
                    *selected,
                );
            }
            AppState::Focus => {
                focus::render(frame, &self.now_playing, &mut self.image_cache);
            }
            AppState::Overlay(popups) => {
                let favorite_ids = &self.favorite_ids;
                let tab_area =
                    render_now_playing_bar(frame, &self.now_playing, &mut self.image_cache);
                let breadcrumb_titles: Vec<String> = popups
                    .iter()
                    .rev()
                    .take(3)
                    .map(super::detail_pages::Overlay::title)
                    .rev()
                    .collect();

                if let Some(popup) = popups.last_mut() {
                    popup.render(
                        frame,
                        tab_area,
                        favorite_ids,
                        &mut self.image_cache,
                        &breadcrumb_titles,
                    );
                }
            }
        }

        self.render_notifications(frame);
    }

    fn render_inner(&mut self, frame: &mut Frame, area: Rect) {
        let [tabs_area, tab_content_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .areas(area);

        let labels: Vec<String> = Tab::VALUES
            .iter()
            .enumerate()
            .map(|(i, tab)| format!("[{}] {}", i.saturating_add(1), tab))
            .collect();

        let label_refs: Vec<&str> = labels.iter().map(std::string::String::as_str).collect();

        let tabs = tab_bar(
            label_refs,
            Tab::VALUES
                .iter()
                .position(|tab| tab == &self.current_screen)
                .unwrap_or(0),
        )
        .block(block(None));

        frame.render_widget(tabs, tabs_area);

        let favorite_ids = &self.favorite_ids;

        match self.current_screen {
            Tab::Favorites => self
                .favorites
                .render(frame, tab_content_area, &mut self.image_cache),
            Tab::Search => {
                self.search
                    .render(frame, tab_content_area, favorite_ids, &mut self.image_cache);
            }
            Tab::Queue => self.queue.render(frame, tab_content_area, favorite_ids),
            Tab::Discover => {
                self.discover
                    .render(frame, tab_content_area, favorite_ids, &mut self.image_cache);
            }
            Tab::Genres => {
                self.genres
                    .render(frame, tab_content_area, favorite_ids, &mut self.image_cache);
            }
            Tab::Preferences => self.preferences.render(frame, tab_content_area),
        }
    }

    fn render_notifications(&self, frame: &mut Frame) {
        let area = frame.area();
        let notifications: Vec<_> = self.notifications.notifications();

        let box_width = 60_u16.min(area.width);
        let inner_width = box_width.saturating_sub(2).max(1);
        let bottom = area.y.saturating_add(area.height);
        let x = area.x.saturating_add(area.width.saturating_sub(box_width));
        let mut y = area.y;

        for notification in notifications.into_iter().rev() {
            let (title, message, color) = match notification {
                Notification::Error(msg) => ("Error", msg, Color::Red),
                Notification::Warning(msg) => ("Warning", msg, Color::Yellow),
                Notification::Success(msg) => ("Success", msg, Color::Green),
                Notification::Info(msg) => ("Info", msg, Color::Blue),
            };

            let message_width = u16::try_from(message.chars().count()).unwrap_or(0);

            let box_height = message_width.div_ceil(inner_width).saturating_add(2);

            let Some(next_y) = y.checked_add(box_height) else {
                break;
            };

            if next_y > bottom {
                break;
            }

            let rect = Rect::new(x, y, box_width, box_height);

            let paragraph = Paragraph::new(message.as_str())
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .border_style(color)
                        .border_type(BorderType::Rounded)
                        .title(title)
                        .title_alignment(Alignment::Center)
                        .title_style(color),
                )
                .wrap(Wrap { trim: true });

            frame.render_widget(Clear, rect);
            frame.render_widget(paragraph, rect);

            y = next_y;
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

fn render_now_playing_bar(
    frame: &mut Frame,
    now_playing: &NowPlayingState,
    image_cache: &mut ImageManager,
) -> Rect {
    let area = frame.area();

    let [content_area, now_playing_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(11)])
        .areas(area);

    if now_playing.playing_track.is_some() {
        now_playing::render(frame, now_playing_area, now_playing, image_cache);
    }

    if now_playing.playing_track.is_some() {
        content_area
    } else {
        content_area.union(now_playing_area)
    }
}

fn render_connect(
    frame: &mut Frame,
    area: Rect,
    available_devices: &[String],
    active_device: &str,
    selected_device: usize,
) {
    const TITLE: &str = "Select output Connect device";
    const ACTIVE_SUFFIX: &str = " (active)";

    let items: Vec<ListItem> = available_devices
        .iter()
        .map(|device| {
            if device == active_device {
                ListItem::new(Line::from(vec![
                    Span::raw(device),
                    Span::styled(ACTIVE_SUFFIX, Style::new().dim()),
                ]))
            } else {
                ListItem::new(device.as_str())
            }
        })
        .collect();

    let content_width = available_devices
        .iter()
        .map(|device| {
            device.len().saturating_add(if device == active_device {
                ACTIVE_SUFFIX.len()
            } else {
                0
            })
        })
        .max()
        .unwrap_or_default();

    let width = content_width.max(TITLE.len()).saturating_add(6);
    let height = items.len().saturating_add(2);

    let width = u16::try_from(width).unwrap_or(u16::MAX);
    let height = u16::try_from(height).unwrap_or(u16::MAX);

    let area = center(
        area,
        Constraint::Length(width.min(area.width)),
        Constraint::Length(height.min(area.height)),
    );

    let list = List::new(items)
        .block(block(Some(TITLE)))
        .highlight_style(HIGHLIGHT_STYLE)
        .highlight_symbol("❯ ");

    let mut state = ListState::default();

    if selected_device < available_devices.len() {
        state.select(Some(selected_device));
    }

    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_help(frame: &mut Frame, area: Rect) {
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
        ["Shuffle tracks", "S"],
        ["Add to queue", "B"],
        ["Play next", "N"],
        ["Delete from queue", "D"],
        ["Clear queue", "X"],
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
        ["Currently playing album page", "I"],
        ["Currently playing artist page", "G"],
        ["Go to artist (album page)", "G"],
        ["Go to album / artist (track info)", "I / G"],
        ["Select Connect device (if configured)", "c"],
        ["Exit", "q"],
    ];

    let rows: Vec<_> = rows.into_iter().map(Row::new).collect();

    let block = block(Some("Help"));

    let table = Table::default().rows(rows).block(block);

    frame.render_widget(Clear, area);
    frame.render_widget(table, area);
}

pub fn render_input(input: &Input, editing: bool, area: Rect, frame: &mut Frame, title: &str) {
    let width = area.width.saturating_sub(3);
    let scroll = input.visual_scroll(usize::from(width));

    let style = if editing {
        HIGHLIGHT_TEXT_STYLE
    } else {
        Style::default()
    };

    let scroll_offset = u16::try_from(scroll).unwrap_or(u16::MAX);

    let paragraph = Paragraph::new(input.value())
        .style(style)
        .scroll((0, scroll_offset))
        .block(block(Some(title)));

    frame.render_widget(paragraph, area);

    if editing {
        let cursor_offset = input
            .visual_cursor()
            .saturating_sub(scroll)
            .saturating_add(1);

        if let Ok(cursor_offset) = u16::try_from(cursor_offset) {
            let x = area.x.saturating_add(cursor_offset);
            let y = area.y.saturating_add(1);

            frame.set_cursor_position((x, y));
        }
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

pub fn basic_list_table(rows: Vec<Row<'_>>, focus: bool) -> Table<'_> {
    Table::new(rows, [Constraint::Min(1)])
        .row_highlight_style(if focus {
            HIGHLIGHT_STYLE
        } else {
            SELECTED_STYLE
        })
        .column_spacing(COLUMN_SPACING)
}

pub fn tab_bar(tabs: Vec<&str>, selected: usize) -> Tabs<'_> {
    Tabs::new(tabs)
        .not_underlined()
        .highlight_style(HIGHLIGHT_STYLE)
        .divider(symbols::line::VERTICAL)
        .select(selected)
}

pub fn sidebar(tabs: Vec<&str>, focused: bool) -> (List<'_>, u16) {
    let width = tabs
        .iter()
        .map(|tab| tab.len())
        .max()
        .and_then(|x| x.to_u16())
        .unwrap_or_default()
        .saturating_add(3);

    let items = tabs.into_iter().map(ListItem::new).collect::<Vec<_>>();

    let highlight_style = if focused {
        HIGHLIGHT_STYLE
    } else {
        SELECTED_STYLE
    };

    let border_style = if focused {
        Style::default().blue()
    } else {
        Style::default()
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
    is_favorite: bool,
) -> Line<'static> {
    let mut parts: Vec<Span<'static>> = Vec::new();

    parts.push(Span::raw(title));

    if is_favorite {
        parts.push(Span::raw(" "));
        parts.push(Span::styled(
            "\u{f004}",
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

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

pub fn mark_as_favorite(line: Line<'static>, is_favorite: bool) -> Line<'static> {
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

pub fn mark_as_owned(line: Line<'static>, owned: bool) -> Line<'static> {
    if !owned {
        return line;
    }

    let mut spans = line.spans;
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        "\u{f007}",
        Style::default().add_modifier(Modifier::DIM),
    ));

    Line::from(spans)
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
