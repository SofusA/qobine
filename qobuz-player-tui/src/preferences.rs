use std::sync::Arc;

use qobuz_player_controls::{ExitSender, controls::Controls};
use qobuz_player_player::{
    AudioQuality,
    database::{Configuration, Database},
};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::*,
};
use tokio::sync::mpsc;
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use crate::{
    app::Output,
    ui::{HIGHLIGHT_TEXT_STYLE, block},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PreferenceFocus {
    #[default]
    CacheDirectory,
    CacheTimeToLive,
    AudioQuality,
    FileBasedStreaming,
    AutoPlay,
    Logout,
}

impl PreferenceFocus {
    const ALL: [PreferenceFocus; 6] = [
        PreferenceFocus::CacheDirectory,
        PreferenceFocus::CacheTimeToLive,
        PreferenceFocus::AudioQuality,
        PreferenceFocus::FileBasedStreaming,
        PreferenceFocus::AutoPlay,
        PreferenceFocus::Logout,
    ];

    fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|focus| *focus == self)
            .unwrap_or_default();

        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    fn previous(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|focus| *focus == self)
            .unwrap_or_default();

        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

fn next_audio_quality(current: AudioQuality) -> AudioQuality {
    match current {
        AudioQuality::Mp3 => AudioQuality::CD,
        AudioQuality::CD => AudioQuality::HIFI96,
        AudioQuality::HIFI96 => AudioQuality::HIFI192,
        AudioQuality::HIFI192 => AudioQuality::Mp3,
    }
}

fn previous_audio_quality(current: AudioQuality) -> AudioQuality {
    match current {
        AudioQuality::Mp3 => AudioQuality::HIFI192,
        AudioQuality::CD => AudioQuality::Mp3,
        AudioQuality::HIFI96 => AudioQuality::CD,
        AudioQuality::HIFI192 => AudioQuality::HIFI96,
    }
}

pub struct PreferencesState {
    exit_sender: ExitSender,
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
    configuration: Configuration,
    focus: Option<PreferenceFocus>,
    cache_path_input: Input,
    cache_ttl_input: Input,
    database: Arc<Database>,
}

impl PreferencesState {
    pub fn new(
        exit_sender: ExitSender,
        audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
        configuration: Configuration,
        database: Arc<Database>,
    ) -> Self {
        let cache_path_value = configuration.cache_directory.to_string_lossy().to_string();
        let cache_ttl_value = configuration.cache_ttl_hours.to_string();

        Self {
            exit_sender,
            audio_cache_ttl_sender,
            configuration,
            focus: Some(PreferenceFocus::CacheDirectory),
            cache_path_input: Input::default().with_value(cache_path_value),
            cache_ttl_input: Input::default().with_value(cache_ttl_value),
            database,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let outer = block(Some("Preferences"));
        let inner = outer.inner(area);

        frame.render_widget(outer, area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(inner);

        self.render_cache_directory(frame, rows[0]);
        self.render_cache_ttl(frame, rows[1]);
        self.render_audio_quality(frame, rows[2]);
        self.render_file_based_streaming(frame, rows[3]);
        self.render_auto_play(frame, rows[4]);
        self.render_logout(frame, rows[5]);
    }

    fn render_cache_directory(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Some(PreferenceFocus::CacheDirectory);
        let has_unsaved_changes = self.cache_directory_has_unsaved_changes();

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        let title = if has_unsaved_changes {
            Line::from(vec![
                Span::styled("Cache directory", Style::default()),
                Span::raw(" "),
                Span::styled("(unsaved)", Style::default().add_modifier(Modifier::DIM)),
            ])
        } else {
            Line::from("Cache directory")
        };

        let paragraph = Paragraph::new(self.cache_path_input.value())
            .style(style)
            .block(Block::default().borders(Borders::ALL).title(title));

        frame.render_widget(paragraph, area);

        if focused {
            let cursor_x = area.x + 1 + self.cache_path_input.visual_cursor() as u16;
            let cursor_y = area.y + 1;

            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    }

    fn render_cache_ttl(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Some(PreferenceFocus::CacheTimeToLive);

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        let paragraph = Paragraph::new(format!("{} hours", self.cache_ttl_input.value()))
            .style(style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Cache time to live"),
            );

        frame.render_widget(paragraph, area);

        if focused {
            let cursor_x = area.x + 1 + self.cache_ttl_input.visual_cursor() as u16;
            let cursor_y = area.y + 1;

            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    }

    fn render_audio_quality(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Some(PreferenceFocus::AudioQuality);

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        let paragraph = Paragraph::new(format!(
            "< {} >",
            self.configuration.max_audio_quality.to_label_str()
        ))
        .style(style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Audio quality"),
        );

        frame.render_widget(paragraph, area);
    }

    fn render_file_based_streaming(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Some(PreferenceFocus::FileBasedStreaming);

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        let value = if self.configuration.use_file_based_streaming {
            "enabled"
        } else {
            "disabled"
        };

        let paragraph = Paragraph::new(format!("[ {} ]", value)).style(style).block(
            Block::default()
                .borders(Borders::ALL)
                .title("File based streaming"),
        );

        frame.render_widget(paragraph, area);
    }

    fn render_auto_play(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Some(PreferenceFocus::AutoPlay);

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        let value = if self.configuration.auto_play {
            "enabled"
        } else {
            "disabled"
        };

        let paragraph = Paragraph::new(format!("[ {} ]", value)).style(style).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Add similar tracks to empty queue"),
        );

        frame.render_widget(paragraph, area);
    }

    fn render_logout(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Some(PreferenceFocus::Logout);

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE.add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red)
        };

        let paragraph = Paragraph::new("Logout")
            .alignment(Alignment::Center)
            .style(style)
            .block(Block::default().borders(Borders::ALL));

        frame.render_widget(paragraph, area);
    }

    pub async fn handle_events(&mut self, event: Event, controls: &Controls) -> Output {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                let focus = self.focus.unwrap_or_default();

                match key_event.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.focus = Some(focus.next());
                        Output::Consumed
                    }

                    KeyCode::Up | KeyCode::Char('k') => {
                        self.focus = Some(focus.previous());
                        Output::Consumed
                    }

                    KeyCode::Left | KeyCode::Char('h') => {
                        self.handle_left(controls).await;
                        Output::Consumed
                    }

                    KeyCode::Right | KeyCode::Char('l') => {
                        self.handle_right(controls).await;
                        Output::Consumed
                    }

                    KeyCode::Enter => {
                        self.apply_focused_setting(controls).await;
                        Output::Consumed
                    }

                    KeyCode::Backspace
                    | KeyCode::Delete
                    | KeyCode::Char(_)
                    | KeyCode::Home
                    | KeyCode::End => match focus {
                        PreferenceFocus::CacheDirectory => {
                            self.cache_path_input.handle_event(&Event::Key(key_event));
                            Output::Consumed
                        }

                        PreferenceFocus::CacheTimeToLive => {
                            let changed = match key_event.code {
                                KeyCode::Char(c) if c.is_ascii_digit() => {
                                    self.cache_ttl_input.handle_event(&Event::Key(key_event));
                                    true
                                }
                                KeyCode::Backspace
                                | KeyCode::Delete
                                | KeyCode::Home
                                | KeyCode::End => {
                                    self.cache_ttl_input.handle_event(&Event::Key(key_event));
                                    true
                                }
                                _ => false,
                            };

                            if changed {
                                self.apply_cache_ttl();
                            }

                            Output::Consumed
                        }

                        _ => Output::NotConsumed,
                    },

                    _ => Output::NotConsumed,
                }
            }

            _ => Output::NotConsumed,
        }
    }

    async fn handle_left(&mut self, controls: &Controls) {
        match self.focus.unwrap_or_default() {
            PreferenceFocus::CacheTimeToLive => {
                let current = self.parse_cache_ttl();
                let new_hours = current.saturating_sub(1);

                self.cache_ttl_input = Input::default().with_value(new_hours.to_string());
                self.apply_cache_ttl();
            }

            PreferenceFocus::AudioQuality => {
                let new_quality = previous_audio_quality(self.configuration.max_audio_quality);

                controls.set_audio_max_quality(new_quality);
                self.configuration.max_audio_quality = new_quality;
            }

            PreferenceFocus::FileBasedStreaming => {
                let new_value = !self.configuration.use_file_based_streaming;

                controls.set_use_file_based_streaming(new_value);
                self.configuration.use_file_based_streaming = new_value;
            }

            PreferenceFocus::AutoPlay => {
                let new_value = !self.configuration.auto_play;

                controls.set_auto_play(new_value);
                self.configuration.auto_play = new_value;
            }

            _ => {}
        }
    }

    async fn handle_right(&mut self, controls: &Controls) {
        match self.focus.unwrap_or_default() {
            PreferenceFocus::CacheTimeToLive => {
                let current = self.parse_cache_ttl();
                let new_hours = current.saturating_add(1);

                self.cache_ttl_input = Input::default().with_value(new_hours.to_string());
                self.apply_cache_ttl();
            }

            PreferenceFocus::AudioQuality => {
                let new_quality = next_audio_quality(self.configuration.max_audio_quality);

                controls.set_audio_max_quality(new_quality);
                self.configuration.max_audio_quality = new_quality;
            }

            PreferenceFocus::FileBasedStreaming => {
                let new_value = !self.configuration.use_file_based_streaming;

                controls.set_use_file_based_streaming(new_value);
                self.configuration.use_file_based_streaming = new_value;
            }

            PreferenceFocus::AutoPlay => {
                let new_value = !self.configuration.auto_play;

                controls.set_auto_play(new_value);
                self.configuration.auto_play = new_value;
            }

            _ => {}
        }
    }

    async fn apply_focused_setting(&mut self, controls: &Controls) {
        let Some(focus) = self.focus else {
            return;
        };

        match focus {
            PreferenceFocus::CacheDirectory => {
                let new_path = self.cache_path_input.value().trim();

                if !new_path.is_empty() {
                    controls.set_audio_cache_directory(new_path.into());
                    self.configuration.cache_directory = new_path.into();
                }
            }

            PreferenceFocus::Logout if self.database.set_credentials(None).await.is_ok() => {
                let _ = self.exit_sender.send(true);
            }

            _ => {}
        }
    }

    fn apply_cache_ttl(&mut self) {
        let new_hours = self.parse_cache_ttl();

        let _ = self.audio_cache_ttl_sender.send(new_hours);
        self.configuration.cache_ttl_hours = new_hours;
    }

    fn parse_cache_ttl(&self) -> u32 {
        let value = self.cache_ttl_input.value().trim();

        if value.is_empty() {
            return 0;
        }

        value
            .parse::<u32>()
            .unwrap_or(self.configuration.cache_ttl_hours)
    }

    fn cache_directory_has_unsaved_changes(&self) -> bool {
        self.cache_path_input.value().trim() != self.configuration.cache_directory.to_string_lossy()
    }
}
