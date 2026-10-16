use cli_module::{ParsedDisconnect, SharedPlaybackConfig};
use controls_module::{ExitSender, controls::Controls};
use crossterm::event::KeyModifiers;
use player_module::{
    AudioQuality,
    database::{Configuration, Database},
};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    widgets::{Block, Borders, ListState, Paragraph},
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
    SharedPlayback,
    Logout,
}

impl PreferencesTab {
    fn next(self) -> Self {
        match self {
            Self::Cache => Self::Audio,
            Self::Audio => Self::SharedPlayback,
            Self::SharedPlayback => Self::Logout,
            Self::Logout => Self::Cache,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Cache => Self::Logout,
            Self::Audio => Self::Cache,
            Self::SharedPlayback => Self::Audio,
            Self::Logout => Self::SharedPlayback,
        }
    }

    const fn labels() -> [&'static str; 4] {
        ["Cache", "Audio", "Shared playback", "Logout"]
    }

    const fn index(self) -> usize {
        match self {
            Self::Cache => 0,
            Self::Audio => 1,
            Self::SharedPlayback => 2,
            Self::Logout => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SharedPlaybackMode {
    #[default]
    None,
    Connect,
    Disconnect,
}

impl SharedPlaybackMode {
    const fn next(self) -> Self {
        match self {
            Self::None => Self::Connect,
            Self::Connect => Self::Disconnect,
            Self::Disconnect => Self::None,
        }
    }

    const fn previous(self) -> Self {
        match self {
            Self::None => Self::Disconnect,
            Self::Connect => Self::None,
            Self::Disconnect => Self::Connect,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::None => "disabled",
            Self::Connect => "connect",
            Self::Disconnect => "disconnect",
        }
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
    SharedPlaybackMode,
    ConnectDeviceName,
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

    shared_playback_mode: SharedPlaybackMode,
    connect_device_name: Input,
    disconnect_server_url: Input,
    disconnect_password: Input,
    disconnect_device_name: Input,
    shared_playback_saved_config: SharedPlaybackConfig,
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

        let connect_enabled = configuration.enable_connect;
        let disconnect_enabled = configuration.enable_disconnect;

        let device_name_value = configuration.device_name.clone().unwrap_or_default();

        let disconnect_server_url_value = configuration
            .disconnect_server_url
            .clone()
            .unwrap_or_default();

        let disconnect_password_value = configuration
            .disconnect_password
            .clone()
            .unwrap_or_default();

        let shared_playback_saved_config =
            if connect_enabled && !device_name_value.trim().is_empty() {
                SharedPlaybackConfig::Connect(device_name_value.trim().to_string())
            } else if disconnect_enabled
                && !disconnect_server_url_value.trim().is_empty()
                && !disconnect_password_value.trim().is_empty()
                && !device_name_value.trim().is_empty()
            {
                SharedPlaybackConfig::Disconnect(ParsedDisconnect {
                    server_url: disconnect_server_url_value.trim().to_string(),
                    password: disconnect_password_value.trim().to_string(),
                    device_name: device_name_value.trim().to_string(),
                })
            } else {
                SharedPlaybackConfig::None
            };

        let shared_playback_mode = match &shared_playback_saved_config {
            SharedPlaybackConfig::Connect(_) => SharedPlaybackMode::Connect,
            SharedPlaybackConfig::Disconnect(_) => SharedPlaybackMode::Disconnect,
            SharedPlaybackConfig::None => SharedPlaybackMode::None,
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
            shared_playback_mode,
            connect_device_name: Input::default().with_value(device_name_value.clone()),
            disconnect_server_url: Input::default().with_value(disconnect_server_url_value),
            disconnect_password: Input::default().with_value(disconnect_password_value),
            disconnect_device_name: Input::default().with_value(device_name_value),
            shared_playback_saved_config,
            tab: PreferencesTab::default(),
            pane: PreferencesPane::default(),
            field: PreferenceFocus::default(),
            editing: false,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let outer = block(Some("Preferences"));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let [content_area, footer_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

        let (sidebar, width) = sidebar(
            PreferencesTab::labels().to_vec(),
            self.pane == PreferencesPane::Sidebar,
        );

        let [sidebar_area, content_area] =
            Layout::horizontal([Constraint::Length(width), Constraint::Min(1)]).areas(content_area);

        let mut state = ListState::default();
        state.select(Some(self.tab.index()));
        frame.render_stateful_widget(sidebar, sidebar_area, &mut state);

        match self.tab {
            PreferencesTab::Cache => self.render_cache(frame, content_area),
            PreferencesTab::Audio => self.render_audio(frame, content_area),
            PreferencesTab::SharedPlayback => {
                self.render_shared_playback(frame, content_area);
            }
            PreferencesTab::Logout => {
                frame.render_widget(
                    Paragraph::new("Enter to logout").alignment(Alignment::Center),
                    content_area,
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
        let [directory_area, ttl_area, _excess] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .areas(area);

        self.render_cache_directory(frame, directory_area);
        self.render_cache_ttl(frame, ttl_area);
    }

    fn render_audio(&self, frame: &mut Frame, area: Rect) {
        let [quality_area, file_based_area, auto_play_area, _excess] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .areas(area);

        self.render_audio_quality(frame, quality_area);
        self.render_file_based_streaming(frame, file_based_area);
        self.render_auto_play(frame, auto_play_area);
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
            let cursor_y = area.y.saturating_add(1);

            if let Ok(cursor_offset) = u16::try_from(self.cache_path_input.visual_cursor())
                && let Some(cursor_x) = area
                    .x
                    .checked_add(1)
                    .and_then(|x| x.checked_add(cursor_offset))
            {
                frame.set_cursor_position(Position::new(cursor_x, cursor_y));
            }
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
            let cursor_y = area.y.saturating_add(1);

            if let Ok(cursor_offset) = u16::try_from(self.cache_path_input.visual_cursor())
                && let Some(cursor_x) = area
                    .x
                    .checked_add(1)
                    .and_then(|x| x.checked_add(cursor_offset))
            {
                frame.set_cursor_position(Position::new(cursor_x, cursor_y));
            }
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

        let paragraph = Paragraph::new(format!("[ {value} ]")).style(style).block(
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

        let paragraph = Paragraph::new(format!("[ {value} ]")).style(style).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Add similar tracks to empty queue"),
        );

        frame.render_widget(paragraph, area);
    }

    fn render_shared_playback_input(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        input: &Input,
        field: PreferenceFocus,
    ) {
        let focused = self.pane == PreferencesPane::Content && self.field == field;

        let mut block = Block::default().borders(Borders::ALL).title(title);

        if Self::shared_playback_field_error(input) && !focused {
            block = block.border_style(Style::default().fg(Color::Red));
        }

        frame.render_widget(
            Paragraph::new(input.value())
                .style(if focused {
                    HIGHLIGHT_TEXT_STYLE
                } else {
                    Style::default()
                })
                .block(block),
            area,
        );

        if focused && self.editing {
            let cursor_y = area.y.saturating_add(1);

            if let Ok(cursor_offset) = u16::try_from(input.visual_cursor())
                && let Some(cursor_x) = area
                    .x
                    .checked_add(1)
                    .and_then(|x| x.checked_add(cursor_offset))
            {
                frame.set_cursor_position(Position::new(cursor_x, cursor_y));
            }
        }
    }

    fn render_shared_playback(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.pane == PreferencesPane::Content
            && self.field == PreferenceFocus::SharedPlaybackMode;

        let [mode_area, remaining_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);

        frame.render_widget(
            Paragraph::new(format!("< {} >", self.shared_playback_mode.label()))
                .style(if focused {
                    HIGHLIGHT_TEXT_STYLE
                } else {
                    Style::default()
                })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Shared playback"),
                ),
            mode_area,
        );

        match self.shared_playback_mode {
            SharedPlaybackMode::None => {}

            SharedPlaybackMode::Connect => {
                let [name_area, _] = Layout::vertical([Constraint::Length(3), Constraint::Min(0)])
                    .areas(remaining_area);

                self.render_shared_playback_input(
                    frame,
                    name_area,
                    "Connect name",
                    &self.connect_device_name,
                    PreferenceFocus::ConnectDeviceName,
                );
            }

            SharedPlaybackMode::Disconnect => {
                let [server_url_area, password_area, name_area, _] = Layout::vertical([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(0),
                ])
                .areas(remaining_area);

                self.render_shared_playback_input(
                    frame,
                    server_url_area,
                    "Server URL",
                    &self.disconnect_server_url,
                    PreferenceFocus::DisconnectServerUrl,
                );

                self.render_shared_playback_input(
                    frame,
                    password_area,
                    "Password",
                    &self.disconnect_password,
                    PreferenceFocus::DisconnectPassword,
                );

                self.render_shared_playback_input(
                    frame,
                    name_area,
                    "Device name",
                    &self.disconnect_device_name,
                    PreferenceFocus::DisconnectDeviceName,
                );
            }
        }
    }

    const fn field_is_input(&self) -> bool {
        match self.field {
            PreferenceFocus::CacheDirectory | PreferenceFocus::CacheTimeToLive => true,

            PreferenceFocus::ConnectDeviceName => {
                matches!(self.shared_playback_mode, SharedPlaybackMode::Connect)
            }

            PreferenceFocus::DisconnectServerUrl
            | PreferenceFocus::DisconnectPassword
            | PreferenceFocus::DisconnectDeviceName => {
                matches!(self.shared_playback_mode, SharedPlaybackMode::Disconnect)
            }

            _ => false,
        }
    }

    pub async fn handle_events(
        &mut self,
        event: Event,
        controls: &Controls,
        database: &Database,
        shared_playback_config_sender: &watch::Sender<SharedPlaybackConfig>,
    ) -> Output {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                if key_event.code == KeyCode::Char('s')
                    && key_event.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.save_all_settings(controls, database, shared_playback_config_sender)
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
                            self.handle_input_event(&event);
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

            PreferenceFocus::SharedPlaybackMode => {
                self.shared_playback_mode = self.shared_playback_mode.previous();
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

            PreferenceFocus::SharedPlaybackMode => {
                self.shared_playback_mode = self.shared_playback_mode.next();
                self.editing = false;
            }

            _ => {}
        }
    }

    const fn select_first_field(&mut self) {
        self.editing = false;

        self.field = match self.tab {
            PreferencesTab::Cache => PreferenceFocus::CacheDirectory,
            PreferencesTab::Audio => PreferenceFocus::AudioQuality,
            PreferencesTab::SharedPlayback => PreferenceFocus::SharedPlaybackMode,
            PreferencesTab::Logout => PreferenceFocus::Logout,
        };
    }

    const fn next_field(&mut self) {
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

            PreferencesTab::SharedPlayback => match self.shared_playback_mode {
                SharedPlaybackMode::None => PreferenceFocus::SharedPlaybackMode,

                SharedPlaybackMode::Connect => match self.field {
                    PreferenceFocus::SharedPlaybackMode => PreferenceFocus::ConnectDeviceName,
                    _ => PreferenceFocus::SharedPlaybackMode,
                },

                SharedPlaybackMode::Disconnect => match self.field {
                    PreferenceFocus::SharedPlaybackMode => PreferenceFocus::DisconnectServerUrl,
                    PreferenceFocus::DisconnectServerUrl => PreferenceFocus::DisconnectPassword,
                    PreferenceFocus::DisconnectPassword => PreferenceFocus::DisconnectDeviceName,
                    _ => PreferenceFocus::SharedPlaybackMode,
                },
            },

            PreferencesTab::Logout => PreferenceFocus::Logout,
        };
    }

    const fn previous_field(&mut self) {
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

            PreferencesTab::SharedPlayback => match self.shared_playback_mode {
                SharedPlaybackMode::None => PreferenceFocus::SharedPlaybackMode,

                SharedPlaybackMode::Connect => match self.field {
                    PreferenceFocus::SharedPlaybackMode => PreferenceFocus::ConnectDeviceName,
                    _ => PreferenceFocus::SharedPlaybackMode,
                },

                SharedPlaybackMode::Disconnect => match self.field {
                    PreferenceFocus::SharedPlaybackMode => PreferenceFocus::DisconnectDeviceName,
                    PreferenceFocus::DisconnectServerUrl => PreferenceFocus::SharedPlaybackMode,
                    PreferenceFocus::DisconnectPassword => PreferenceFocus::DisconnectServerUrl,
                    _ => PreferenceFocus::DisconnectPassword,
                },
            },

            PreferencesTab::Logout => PreferenceFocus::Logout,
        };
    }

    fn handle_input_event(&mut self, event: &Event) -> Output {
        if !self.editing {
            return Output::NotConsumed;
        }

        match self.field {
            PreferenceFocus::CacheDirectory => {
                self.cache_path_input.handle_event(event);
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
                    self.cache_ttl_input.handle_event(event);
                }
            }

            PreferenceFocus::ConnectDeviceName
                if self.shared_playback_mode == SharedPlaybackMode::Connect =>
            {
                self.connect_device_name.handle_event(event);
            }

            PreferenceFocus::DisconnectServerUrl
                if self.shared_playback_mode == SharedPlaybackMode::Disconnect =>
            {
                self.disconnect_server_url.handle_event(event);
            }

            PreferenceFocus::DisconnectPassword
                if self.shared_playback_mode == SharedPlaybackMode::Disconnect =>
            {
                self.disconnect_password.handle_event(event);
            }

            PreferenceFocus::DisconnectDeviceName
                if self.shared_playback_mode == SharedPlaybackMode::Disconnect =>
            {
                self.disconnect_device_name.handle_event(event);
            }

            _ => return Output::NotConsumed,
        }

        Output::Consumed
    }

    async fn save_all_settings(
        &mut self,
        controls: &Controls,
        database: &Database,
        shared_playback_config_sender: &watch::Sender<SharedPlaybackConfig>,
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

        let shared_playback_config = self.shared_playback_config();

        let _ = shared_playback_config_sender.send(shared_playback_config.clone());

        self.shared_playback_saved_config = shared_playback_config.clone();

        let disconnect_enabled =
            matches!(shared_playback_config, SharedPlaybackConfig::Disconnect(_));

        self.configuration.enable_disconnect = disconnect_enabled;
        self.configuration.disconnect_server_url =
            trimmed_optional_value(&self.disconnect_server_url);
        self.configuration.disconnect_password = trimmed_optional_value(&self.disconnect_password);

        match &shared_playback_config {
            SharedPlaybackConfig::Disconnect(config) => {
                self.configuration.enable_connect = false;
                self.configuration.enable_disconnect = true;
                self.configuration.device_name = Some(config.device_name.clone());

                let _ = database
                    .set_disconnect_config(
                        &config.server_url,
                        &config.password,
                        &config.device_name,
                    )
                    .await;

                let _ = database.set_connect_enabled(false).await;
                let _ = database.set_disconnect_enabled(true).await;
            }

            SharedPlaybackConfig::Connect(device_name) => {
                self.configuration.enable_connect = true;
                self.configuration.enable_disconnect = false;
                self.configuration.device_name = Some(device_name.clone());

                let _ = database.set_device_name(device_name).await;
                let _ = database.set_connect_enabled(true).await;
                let _ = database.set_disconnect_enabled(false).await;
            }

            SharedPlaybackConfig::None => {
                self.configuration.enable_connect = false;
                self.configuration.enable_disconnect = false;

                let _ = database.set_connect_enabled(false).await;
                let _ = database.set_disconnect_enabled(false).await;
            }
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
            || self.shared_playback_config() != self.shared_playback_saved_config
    }

    fn cache_directory_has_unsaved_changes(&self) -> bool {
        self.cache_path_input.value().trim() != self.configuration.cache_directory.to_string_lossy()
    }

    fn cache_ttl_has_unsaved_changes(&self) -> bool {
        self.parse_cache_ttl() != self.configuration.cache_ttl_hours
    }

    fn shared_playback_config(&self) -> SharedPlaybackConfig {
        match self.shared_playback_mode {
            SharedPlaybackMode::None => SharedPlaybackConfig::None,

            SharedPlaybackMode::Connect => {
                let name = self.connect_device_name.value().trim();

                if name.is_empty() {
                    SharedPlaybackConfig::None
                } else {
                    SharedPlaybackConfig::Connect(name.to_string())
                }
            }

            SharedPlaybackMode::Disconnect => {
                let server_url = self.disconnect_server_url.value().trim();
                let password = self.disconnect_password.value().trim();
                let device_name = self.disconnect_device_name.value().trim();

                if server_url.is_empty() || password.is_empty() || device_name.is_empty() {
                    return SharedPlaybackConfig::None;
                }

                SharedPlaybackConfig::Disconnect(ParsedDisconnect {
                    server_url: server_url.to_string(),
                    password: password.to_string(),
                    device_name: device_name.to_string(),
                })
            }
        }
    }

    fn shared_playback_field_error(value: &Input) -> bool {
        value.value().trim().is_empty()
    }
}

fn trimmed_optional_value(value: &Input) -> Option<String> {
    let value = value.value().trim();

    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

const fn next_audio_quality(current: AudioQuality) -> AudioQuality {
    match current {
        AudioQuality::Mp3 => AudioQuality::CD,
        AudioQuality::CD => AudioQuality::HIFI96,
        AudioQuality::HIFI96 => AudioQuality::HIFI192,
        AudioQuality::HIFI192 => AudioQuality::Mp3,
    }
}

const fn previous_audio_quality(current: AudioQuality) -> AudioQuality {
    match current {
        AudioQuality::Mp3 => AudioQuality::HIFI192,
        AudioQuality::CD => AudioQuality::Mp3,
        AudioQuality::HIFI96 => AudioQuality::CD,
        AudioQuality::HIFI192 => AudioQuality::HIFI96,
    }
}
