use controls_module::{ExitSender, controls::Controls};
use crossterm::event::KeyModifiers;
use disconnect_module::DisconnectClientConfig;
use player_module::{
    AudioQuality,
    database::{Configuration, Database},
};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::*,
};
use tokio::sync::{mpsc, watch};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::{
    app::Output,
    ui::{HIGHLIGHT_TEXT_STYLE, block, sidebar},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PreferencesTab {
    #[default]
    Cache,
    Audio,
    Disconnect,
    Logout,
}

impl PreferencesTab {
    const ALL: [Self; 4] = [Self::Cache, Self::Audio, Self::Disconnect, Self::Logout];

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|x| *x == self).unwrap();
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn previous(self) -> Self {
        let i = Self::ALL.iter().position(|x| *x == self).unwrap();
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    fn labels() -> Vec<&'static str> {
        vec!["Cache", "Audio", "Disconnect", "Logout"]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PreferencesPane {
    #[default]
    Sidebar,
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PreferenceFocus {
    #[default]
    CacheDirectory,
    CacheTimeToLive,
    AudioQuality,
    FileBasedStreaming,
    AutoPlay,
    DisconnectEnabled,
    DisconnectServerUrl,
    DisconnectPassword,
    DisconnectDeviceName,
    Logout,
}

pub struct PreferencesState {
    exit_sender: ExitSender,
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
    configuration: Configuration,

    tab: PreferencesTab,
    pane: PreferencesPane,
    field: PreferenceFocus,
    editing: bool,

    cache_path_input: Input,
    cache_ttl_input: Input,
    audio_quality: AudioQuality,
    use_file_based_streaming: bool,
    auto_play: bool,

    disconnect_enabled: bool,
    disconnect_server_url: Input,
    disconnect_password: Input,
    disconnect_device_name: Input,
    disconnect_saved_config: Option<DisconnectClientConfig>,
}

impl PreferencesState {
    pub fn new(
        exit_sender: ExitSender,
        audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
        configuration: Configuration,
    ) -> Self {
        let cache_path_value = configuration.cache_directory.to_string_lossy().to_string();
        let cache_ttl_value = configuration.cache_ttl_hours.to_string();
        let audio_quality = configuration.max_audio_quality;
        let use_file_based_streaming = configuration.use_file_based_streaming;
        let auto_play = configuration.auto_play;

        let disconnect_enabled = configuration.enable_disconnect;
        let disconnect_server_url_value = configuration
            .disconnect_server_url
            .clone()
            .unwrap_or_default();
        let disconnect_password_value = configuration
            .disconnect_password
            .clone()
            .unwrap_or_default();
        let disconnect_device_name_value = configuration.device_name.clone().unwrap_or_default();

        let disconnect_saved_config = if disconnect_enabled
            && !disconnect_server_url_value.trim().is_empty()
            && !disconnect_password_value.trim().is_empty()
            && !disconnect_device_name_value.trim().is_empty()
        {
            Some(DisconnectClientConfig {
                server_url: disconnect_server_url_value.trim().to_string(),
                password: disconnect_password_value.trim().to_string(),
                device_name: disconnect_device_name_value.trim().to_string(),
            })
        } else {
            None
        };

        Self {
            exit_sender,
            audio_cache_ttl_sender,
            configuration,
            cache_path_input: Input::default().with_value(cache_path_value),
            cache_ttl_input: Input::default().with_value(cache_ttl_value),
            audio_quality,
            use_file_based_streaming,
            auto_play,
            disconnect_enabled,
            disconnect_server_url: Input::default().with_value(disconnect_server_url_value),
            disconnect_password: Input::default().with_value(disconnect_password_value),
            disconnect_device_name: Input::default().with_value(disconnect_device_name_value),
            disconnect_saved_config,
            tab: Default::default(),
            pane: Default::default(),
            field: Default::default(),
            editing: false,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let outer = block(Some("Preferences"));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

        let content_area = rows[0];
        let footer_area = rows[1];

        let (sidebar, width) = sidebar(
            PreferencesTab::labels(),
            self.pane == PreferencesPane::Sidebar,
        );

        let chunks =
            Layout::horizontal([Constraint::Length(width), Constraint::Min(1)]).split(content_area);

        let mut state = ListState::default();
        state.select(Some(self.tab as usize));
        frame.render_stateful_widget(sidebar, chunks[0], &mut state);

        match self.tab {
            PreferencesTab::Cache => self.render_cache(frame, chunks[1]),
            PreferencesTab::Audio => self.render_audio(frame, chunks[1]),
            PreferencesTab::Disconnect => self.render_disconnect(frame, chunks[1]),
            PreferencesTab::Logout => {
                frame.render_widget(
                    Paragraph::new("Enter to logout").alignment(Alignment::Center),
                    chunks[1],
                );
            }
        }

        if self.has_unsaved_changes() {
            frame.render_widget(
                Paragraph::new("Ctrl+S to save")
                    .alignment(Alignment::Right)
                    .style(Style::default().add_modifier(Modifier::BOLD)),
                footer_area,
            );
        }
    }

    fn render_cache(&mut self, frame: &mut Frame, area: Rect) {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

        self.render_cache_directory(frame, rows[0]);
        self.render_cache_ttl(frame, rows[1]);
    }

    fn render_audio(&self, frame: &mut Frame, area: Rect) {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

        self.render_audio_quality(frame, rows[0]);
        self.render_file_based_streaming(frame, rows[1]);
        self.render_auto_play(frame, rows[2]);
    }

    fn render_cache_directory(&mut self, frame: &mut Frame, area: Rect) {
        let focused =
            self.pane == PreferencesPane::Content && self.field == PreferenceFocus::CacheDirectory;

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        let paragraph = Paragraph::new(self.cache_path_input.value())
            .style(style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Cache directory"),
            );

        frame.render_widget(paragraph, area);

        if focused && self.editing {
            let cursor_x = area.x + 1 + self.cache_path_input.visual_cursor() as u16;
            let cursor_y = area.y + 1;

            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    }

    fn render_cache_ttl(&mut self, frame: &mut Frame, area: Rect) {
        let focused =
            self.pane == PreferencesPane::Content && self.field == PreferenceFocus::CacheTimeToLive;

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

        if focused && self.editing {
            let cursor_x = area.x + 1 + self.cache_ttl_input.visual_cursor() as u16;
            let cursor_y = area.y + 1;

            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    }

    fn render_audio_quality(&self, frame: &mut Frame, area: Rect) {
        let focused =
            self.pane == PreferencesPane::Content && self.field == PreferenceFocus::AudioQuality;

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        let paragraph = Paragraph::new(format!("< {} >", self.audio_quality.to_label_str()))
            .style(style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Audio quality"),
            );

        frame.render_widget(paragraph, area);
    }

    fn render_file_based_streaming(&self, frame: &mut Frame, area: Rect) {
        let focused = self.pane == PreferencesPane::Content
            && self.field == PreferenceFocus::FileBasedStreaming;

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        let value = if self.use_file_based_streaming {
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
        let focused =
            self.pane == PreferencesPane::Content && self.field == PreferenceFocus::AutoPlay;

        let style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        let value = if self.auto_play {
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

    fn render_disconnect_input(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        input: &Input,
        field: PreferenceFocus,
    ) {
        let focused = self.pane == PreferencesPane::Content && self.field == field;

        let mut style = if focused {
            HIGHLIGHT_TEXT_STYLE
        } else {
            Style::default()
        };

        if !self.disconnect_enabled {
            style = style.add_modifier(Modifier::DIM);
        }

        let mut block = Block::default().borders(Borders::ALL).title(title);

        if self.disconnect_enabled && Self::disconnect_field_error(input) {
            block = block.border_style(Style::default().fg(Color::Red));
        }

        frame.render_widget(
            Paragraph::new(input.value()).style(style).block(block),
            area,
        );

        if focused && self.editing && self.disconnect_enabled {
            let cursor_x = area.x + 1 + input.visual_cursor() as u16;
            let cursor_y = area.y + 1;

            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    }

    fn render_disconnect(&mut self, frame: &mut Frame, area: Rect) {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

        let focused = self.pane == PreferencesPane::Content
            && self.field == PreferenceFocus::DisconnectEnabled;

        frame.render_widget(
            Paragraph::new(if self.disconnect_enabled {
                "[ enabled ]"
            } else {
                "[ disabled ]"
            })
            .style(if focused {
                HIGHLIGHT_TEXT_STYLE
            } else {
                Style::default()
            })
            .block(Block::default().borders(Borders::ALL).title("Disconnect")),
            rows[0],
        );

        self.render_disconnect_input(
            frame,
            rows[1],
            "Server URL",
            &self.disconnect_server_url,
            PreferenceFocus::DisconnectServerUrl,
        );

        self.render_disconnect_input(
            frame,
            rows[2],
            "Password",
            &self.disconnect_password,
            PreferenceFocus::DisconnectPassword,
        );

        self.render_disconnect_input(
            frame,
            rows[3],
            "Device name",
            &self.disconnect_device_name,
            PreferenceFocus::DisconnectDeviceName,
        );
    }

    fn field_is_input(&self) -> bool {
        match self.field {
            PreferenceFocus::CacheDirectory | PreferenceFocus::CacheTimeToLive => true,

            PreferenceFocus::DisconnectServerUrl
            | PreferenceFocus::DisconnectPassword
            | PreferenceFocus::DisconnectDeviceName => self.disconnect_enabled,

            _ => false,
        }
    }

    pub async fn handle_events(
        &mut self,
        event: Event,
        controls: &Controls,
        database: &Database,
        disconnect_client_config_sender: &watch::Sender<Option<DisconnectClientConfig>>,
    ) -> Output {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                if key_event.code == KeyCode::Char('s')
                    && key_event.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.save_all_settings(controls, database, disconnect_client_config_sender)
                        .await;

                    return Output::Consumed;
                }

                match self.pane {
                    PreferencesPane::Sidebar => match key_event.code {
                        KeyCode::Up | KeyCode::Char('k') => self.tab = self.tab.previous(),
                        KeyCode::Down | KeyCode::Char('j') => self.tab = self.tab.next(),
                        KeyCode::Enter if self.tab == PreferencesTab::Logout => {
                            if database.set_credentials(None).await.is_ok() {
                                let _ = self.exit_sender.send(true);
                            }
                        }
                        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                            self.select_first_field();
                            self.pane = PreferencesPane::Content;
                        }
                        _ => return Output::NotConsumed,
                    },

                    PreferencesPane::Content => match key_event.code {
                        KeyCode::Esc => {
                            if self.editing {
                                self.editing = false;
                            } else {
                                self.pane = PreferencesPane::Sidebar;
                            }
                        }
                        KeyCode::Enter if self.field_is_input() => {
                            self.editing = !self.editing;
                        }
                        _ if self.editing => {
                            self.handle_input_event(event);
                        }

                        KeyCode::Up | KeyCode::Char('k') => self.previous_field(),
                        KeyCode::Down | KeyCode::Char('j') => self.next_field(),
                        KeyCode::Left | KeyCode::Char('h') => self.handle_left(),
                        KeyCode::Right | KeyCode::Char('l') => self.handle_right(),

                        _ => return Output::NotConsumed,
                    },
                }

                Output::Consumed
            }

            _ => Output::NotConsumed,
        }
    }

    fn handle_left(&mut self) {
        match self.field {
            PreferenceFocus::CacheTimeToLive => {
                let hours = self.parse_cache_ttl().saturating_sub(1);
                self.cache_ttl_input = Input::default().with_value(hours.to_string());
            }

            PreferenceFocus::AudioQuality => {
                self.audio_quality = previous_audio_quality(self.audio_quality);
            }

            PreferenceFocus::FileBasedStreaming => {
                self.use_file_based_streaming = !self.use_file_based_streaming;
            }

            PreferenceFocus::AutoPlay => {
                self.auto_play = !self.auto_play;
            }

            PreferenceFocus::DisconnectEnabled => {
                self.disconnect_enabled = !self.disconnect_enabled;
                self.editing = false;
            }

            _ => {}
        }
    }

    fn handle_right(&mut self) {
        match self.field {
            PreferenceFocus::CacheTimeToLive => {
                let hours = self.parse_cache_ttl().saturating_add(1);
                self.cache_ttl_input = Input::default().with_value(hours.to_string());
            }

            PreferenceFocus::AudioQuality => {
                self.audio_quality = next_audio_quality(self.audio_quality);
            }

            PreferenceFocus::FileBasedStreaming => {
                self.use_file_based_streaming = !self.use_file_based_streaming;
            }

            PreferenceFocus::AutoPlay => {
                self.auto_play = !self.auto_play;
            }

            PreferenceFocus::DisconnectEnabled => {
                self.disconnect_enabled = !self.disconnect_enabled;
                self.editing = false;
            }

            _ => {}
        }
    }

    fn select_first_field(&mut self) {
        self.editing = false;

        self.field = match self.tab {
            PreferencesTab::Cache => PreferenceFocus::CacheDirectory,
            PreferencesTab::Audio => PreferenceFocus::AudioQuality,
            PreferencesTab::Disconnect => PreferenceFocus::DisconnectEnabled,
            PreferencesTab::Logout => PreferenceFocus::Logout,
        };
    }

    fn next_field(&mut self) {
        self.editing = false;

        self.field = match self.tab {
            PreferencesTab::Cache => match self.field {
                PreferenceFocus::CacheDirectory => PreferenceFocus::CacheTimeToLive,
                _ => PreferenceFocus::CacheDirectory,
            },

            PreferencesTab::Audio => match self.field {
                PreferenceFocus::AudioQuality => PreferenceFocus::FileBasedStreaming,
                PreferenceFocus::FileBasedStreaming => PreferenceFocus::AutoPlay,
                _ => PreferenceFocus::AudioQuality,
            },

            PreferencesTab::Disconnect => {
                if !self.disconnect_enabled {
                    PreferenceFocus::DisconnectEnabled
                } else {
                    match self.field {
                        PreferenceFocus::DisconnectEnabled => PreferenceFocus::DisconnectServerUrl,
                        PreferenceFocus::DisconnectServerUrl => PreferenceFocus::DisconnectPassword,
                        PreferenceFocus::DisconnectPassword => {
                            PreferenceFocus::DisconnectDeviceName
                        }
                        _ => PreferenceFocus::DisconnectEnabled,
                    }
                }
            }

            PreferencesTab::Logout => PreferenceFocus::Logout,
        };
    }

    fn previous_field(&mut self) {
        self.editing = false;

        self.field = match self.tab {
            PreferencesTab::Cache => match self.field {
                PreferenceFocus::CacheDirectory => PreferenceFocus::CacheTimeToLive,
                _ => PreferenceFocus::CacheDirectory,
            },

            PreferencesTab::Audio => match self.field {
                PreferenceFocus::AudioQuality => PreferenceFocus::AutoPlay,
                PreferenceFocus::FileBasedStreaming => PreferenceFocus::AudioQuality,
                _ => PreferenceFocus::FileBasedStreaming,
            },

            PreferencesTab::Disconnect => {
                if !self.disconnect_enabled {
                    PreferenceFocus::DisconnectEnabled
                } else {
                    match self.field {
                        PreferenceFocus::DisconnectEnabled => PreferenceFocus::DisconnectDeviceName,
                        PreferenceFocus::DisconnectServerUrl => PreferenceFocus::DisconnectEnabled,
                        PreferenceFocus::DisconnectPassword => PreferenceFocus::DisconnectServerUrl,
                        _ => PreferenceFocus::DisconnectPassword,
                    }
                }
            }

            PreferencesTab::Logout => PreferenceFocus::Logout,
        };
    }

    fn handle_input_event(&mut self, event: Event) -> Output {
        if !self.editing {
            return Output::NotConsumed;
        }

        match self.field {
            PreferenceFocus::CacheDirectory => {
                self.cache_path_input.handle_event(&event);
            }

            PreferenceFocus::CacheTimeToLive => {
                let allowed = matches!(
                    &event,
                    Event::Key(key_event)
                        if matches!(
                            key_event.code,
                            KeyCode::Char(c) if c.is_ascii_digit()
                        ) || matches!(
                            key_event.code,
                            KeyCode::Backspace
                                | KeyCode::Delete
                                | KeyCode::Home
                                | KeyCode::End
                                | KeyCode::Left
                                | KeyCode::Right
                        )
                );

                if allowed {
                    self.cache_ttl_input.handle_event(&event);
                }
            }

            PreferenceFocus::DisconnectServerUrl if self.disconnect_enabled => {
                self.disconnect_server_url.handle_event(&event);
            }

            PreferenceFocus::DisconnectPassword if self.disconnect_enabled => {
                self.disconnect_password.handle_event(&event);
            }

            PreferenceFocus::DisconnectDeviceName if self.disconnect_enabled => {
                self.disconnect_device_name.handle_event(&event);
            }

            _ => return Output::NotConsumed,
        }

        Output::Consumed
    }

    async fn save_all_settings(
        &mut self,
        controls: &Controls,
        database: &Database,
        disconnect_client_config_sender: &watch::Sender<Option<DisconnectClientConfig>>,
    ) {
        let cache_directory = self.cache_path_input.value().trim().to_string();
        let cache_ttl_hours = self.parse_cache_ttl();

        if !cache_directory.is_empty() {
            controls.set_audio_cache_directory(cache_directory.clone().into());
            self.configuration.cache_directory = cache_directory.into();
        }

        let _ = self.audio_cache_ttl_sender.send(cache_ttl_hours);
        self.configuration.cache_ttl_hours = cache_ttl_hours;

        controls.set_audio_max_quality(self.audio_quality);
        self.configuration.max_audio_quality = self.audio_quality;

        controls.set_use_file_based_streaming(self.use_file_based_streaming);
        self.configuration.use_file_based_streaming = self.use_file_based_streaming;

        controls.set_auto_play(self.auto_play);
        self.configuration.auto_play = self.auto_play;

        let disconnect_config = self.disconnect_config();

        let _ = disconnect_client_config_sender.send(disconnect_config.clone());
        self.disconnect_saved_config = disconnect_config.clone();

        self.configuration.enable_disconnect = self.disconnect_enabled;
        self.configuration.disconnect_server_url =
            self.trimmed_optional_value(&self.disconnect_server_url);
        self.configuration.disconnect_password =
            self.trimmed_optional_value(&self.disconnect_password);
        self.configuration.device_name = self.trimmed_optional_value(&self.disconnect_device_name);

        if let Some(config) = disconnect_config {
            let _ = database
                .set_disconnect_config(&config.server_url, &config.password, &config.device_name)
                .await;

            let _ = database.set_disconnect_enabled(true).await;
        } else {
            let _ = database.set_disconnect_enabled(false).await;
        }
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

    fn has_unsaved_changes(&self) -> bool {
        self.cache_directory_has_unsaved_changes()
            || self.cache_ttl_has_unsaved_changes()
            || self.audio_quality != self.configuration.max_audio_quality
            || self.use_file_based_streaming != self.configuration.use_file_based_streaming
            || self.auto_play != self.configuration.auto_play
            || self.disconnect_has_unsaved_changes()
    }

    fn cache_directory_has_unsaved_changes(&self) -> bool {
        self.cache_path_input.value().trim() != self.configuration.cache_directory.to_string_lossy()
    }

    fn cache_ttl_has_unsaved_changes(&self) -> bool {
        self.parse_cache_ttl() != self.configuration.cache_ttl_hours
    }

    fn disconnect_config(&self) -> Option<DisconnectClientConfig> {
        let server_url = self.disconnect_server_url.value().trim();
        let password = self.disconnect_password.value().trim();
        let device_name = self.disconnect_device_name.value().trim();

        if !self.disconnect_enabled {
            return None;
        }

        if server_url.is_empty() || password.is_empty() || device_name.is_empty() {
            return None;
        }

        Some(DisconnectClientConfig {
            server_url: server_url.to_string(),
            password: password.to_string(),
            device_name: device_name.to_string(),
        })
    }

    fn disconnect_has_unsaved_changes(&self) -> bool {
        self.disconnect_enabled != self.configuration.enable_disconnect
            || self.trimmed_optional_value(&self.disconnect_server_url)
                != self.configuration.disconnect_server_url
            || self.trimmed_optional_value(&self.disconnect_password)
                != self.configuration.disconnect_password
            || self.trimmed_optional_value(&self.disconnect_device_name)
                != self.configuration.device_name
    }

    fn disconnect_field_error(value: &Input) -> bool {
        value.value().trim().is_empty()
    }

    fn trimmed_optional_value(&self, value: &Input) -> Option<String> {
        let value = value.value().trim();

        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
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
