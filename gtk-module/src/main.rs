use cli_module::{create_player, error_exit, spawn_clean_up_mut};
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use controls_module::StatusReceiver;
use disconnect_module::{DisconnectClientConfig, spawn_disconnect};
#[cfg(target_os = "linux")]
use mpris_module::spawn_mpris;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use player_module::{
    AppResult,
    client::{Client, get_app_id},
    database::Database,
    notification::NotificationBroadcast,
};

#[tokio::main]
async fn main() {
    match run().await {
        Ok(()) => {}
        Err(err) => {
            error_exit(err);
        }
    }
}

pub async fn run() -> AppResult<()> {
    tracing_subscriber::fmt().compact().init();

    let database = Arc::new(Database::new().await?);

    let (exit_sender, exit_receiver) = broadcast::channel(5);

    let credentials = database.get_credentials().await?;
    let configuration = database.get_configuration().await?;

    let app_id = get_app_id().await?;
    let client = Arc::new(Client::new(
        credentials,
        configuration.max_audio_quality,
        configuration.use_file_based_streaming,
    ));

    let broadcast = Arc::new(NotificationBroadcast::new());

    let mut player = create_player(
        configuration.cache_directory,
        database.clone(),
        client.clone(),
        broadcast.clone(),
        None,
        None,
        None,
    )
    .await?;

    #[cfg(target_os = "linux")]
    spawn_mpris(&player, &exit_sender, "io.github.sofusa.qobine".to_string());

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    {
        let status_receiver = player.status();
        sleep_inhibitor(status_receiver);
    }

    let (ttl_tx, ttl_rx) = mpsc::unbounded_channel::<u32>();
    spawn_clean_up_mut(
        database.clone(),
        Some(configuration.cache_ttl_hours),
        ttl_rx,
    );

    let disconnect_client_config = if configuration.enable_disconnect
        && let Some(server_url) = configuration.disconnect_server_url
        && let Some(password) = configuration.disconnect_password
        && let Some(device_name) = configuration.device_name
    {
        Some(DisconnectClientConfig {
            server_url,
            password,
            device_name,
        })
    } else {
        None
    };

    let (config_tx, config_rx) = watch::channel(disconnect_client_config);
    let (available_devices_tx, available_devices_rx) = watch::channel(Default::default());
    let (active_device_tx, active_device_rx) = watch::channel(Default::default());
    let (set_active_device_tx, set_active_device_rx) = mpsc::unbounded_channel();

    spawn_disconnect(
        &player,
        config_rx,
        available_devices_tx,
        active_device_tx,
        set_active_device_rx,
    );

    let client = client.clone();

    let controls = player.controls();
    let tracklist_receiver = player.tracklist();
    let status_receiver = player.status();
    let position_receiver = player.position();
    let volume_receiver = player.volume();
    let database_clone = database.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = gtk_module::init(
            client,
            app_id,
            tracklist_receiver,
            status_receiver,
            position_receiver,
            volume_receiver,
            controls,
            database_clone,
            exit_sender,
            ttl_tx,
            broadcast,
            available_devices_rx,
            active_device_rx,
            set_active_device_tx,
            config_tx,
        ) {
            error_exit(e);
        };
    });

    player.player_loop(exit_receiver).await?;

    Ok(())
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn sleep_inhibitor(mut status_receiver: StatusReceiver) {
    std::thread::spawn(move || {
        let mut sleep_inhibitor = SleepInhibitor::new();

        loop {
            use controls_module::Status;
            use futures::executor::block_on;

            let changed = block_on(async { status_receiver.changed().await });
            if changed.is_err() {
                sleep_inhibitor.restore_sleep();
                break;
            }

            let status = *status_receiver.borrow_and_update();
            match status {
                Status::Paused => sleep_inhibitor.restore_sleep(),
                Status::Playing | Status::Buffering => sleep_inhibitor.block_sleep(),
            }
        }
    });
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
struct SleepInhibitor {
    awake: Option<keepawake::KeepAwake>,
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
impl SleepInhibitor {
    fn new() -> Self {
        Self { awake: None }
    }

    fn block_sleep(&mut self) {
        if self.awake.is_none() {
            let mut builder = keepawake::Builder::default();
            builder
                .idle(true)
                .sleep(true)
                .reason("Audio playback")
                .app_name("qobuz-player");

            if let Ok(awake) = builder.create() {
                self.awake = Some(awake);
            }
        }
    }

    fn restore_sleep(&mut self) {
        let _ = self.awake.take();
    }
}
