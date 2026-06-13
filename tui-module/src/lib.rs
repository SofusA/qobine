use std::sync::Arc;

use app::{App, get_current_state_without_image};
use controls_module::{
    ExitSender, PositionReceiver, StatusReceiver, TracklistReceiver, controls::Controls,
};
use disconnect_module::DisconnectClientConfig;
use favorites::FavoritesState;
use player_module::{
    AppResult, client::Client, database::Database, notification::NotificationBroadcast,
};
use queue::QueueState;
use ratatui::{prelude::*, widgets::*};
use tokio::sync::{mpsc, watch};
use ui::center;

mod app;
mod discover;
mod favorites;
mod genres;
mod now_playing;
mod popup;
mod preferences;
mod queue;
mod search;
mod sub_tab;
mod ui;
mod widgets;

#[allow(clippy::too_many_arguments)]
pub async fn init(
    client: Arc<Client>,
    broadcast: Arc<NotificationBroadcast>,
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    exit_sender: ExitSender,
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
    disable_tui_album_cover: bool,
    database: Arc<Database>,
    connect_available_devices: watch::Receiver<Vec<String>>,
    connect_active_device: watch::Receiver<String>,
    set_connect_active_device: mpsc::UnboundedSender<String>,
    disconnect_client_config_sender: watch::Sender<Option<DisconnectClientConfig>>,
) -> AppResult<()> {
    let mut terminal = ratatui::init();

    draw_loading_screen(&mut terminal);

    let tracklist_value = tracklist_receiver.borrow().clone();
    let status_value = *status_receiver.borrow();
    let queue_tracks = tracklist_value
        .queue()
        .into_iter()
        .map(|x| x.track.clone())
        .collect();
    let (now_playing, current_image_url) =
        get_current_state_without_image(&tracklist_value, status_value);

    let initial_configuration = database.get_configuration().await?;

    let mut app = App {
        broadcast,
        notifications: Default::default(),
        controls,
        database,
        now_playing,
        full_screen: false,
        position: position_receiver,
        tracklist: tracklist_receiver,
        status: status_receiver,
        current_screen: Default::default(),
        exit: Default::default(),
        should_draw: true,
        app_state: Default::default(),
        disable_tui_album_cover,
        current_image_url,
        favorites: FavoritesState::new(&client).await?,
        search: Default::default(),
        queue: QueueState::new(queue_tracks),
        discover: discover::DiscoverState::new(&client).await?,
        genres: genres::GenresState::new(&client).await?,
        preferences: preferences::PreferencesState::new(
            exit_sender.clone(),
            audio_cache_ttl_sender,
            initial_configuration,
        ),
        client,
        favorite_ids: Default::default(),
        connect_available_devices,
        connect_active_device,
        set_connect_active_device,
        disconnect_client_config_sender,
    };

    app.update_favorites().await;

    _ = app.run(&mut terminal).await;
    ratatui::restore();
    let _ = exit_sender.send(true);

    Ok(())
}

fn draw_loading_screen<B: Backend>(terminal: &mut Terminal<B>) {
    let ascii_art = r#"
             _                     _
  __ _  ___ | |__  _   _ _____ __ | | __ _ _   _  ___ _ __
 / _` |/ _ \| '_ \| | | |_  / '_ \| |/ _` | | | |/ _ \ '__|
| (_| | (_) | |_) | |_| |/ /| |_) | | (_| | |_| |  __/ |
 \__, |\___/|_.__/ \__,_/___| .__/|_|\__,_|\__, |\___|_|
    |_|                     |_|            |___/
"#;

    let width = ascii_art
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let height = ascii_art.lines().count() as u16;

    terminal
        .draw(|f| {
            let area = center(
                f.area(),
                Constraint::Length(width),
                Constraint::Length(height),
            );
            let paragraph = Paragraph::new(ascii_art).alignment(Alignment::Left);
            f.render_widget(paragraph, area);
        })
        .expect("infallible");
}
