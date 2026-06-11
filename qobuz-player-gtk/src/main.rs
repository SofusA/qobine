use qobuz_player_cli::{create_player, spawn_clean_up_mut};
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use qobuz_player_controls::StatusReceiver;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use qobuz_player_player::{
    AppResult,
    client::{Client, get_app_id},
    database::Database,
    error::Error,
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
    {
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let controls = player.controls();
        let exit_sender = exit_sender.clone();
        tokio::spawn(async move {
            if let Err(e) = qobuz_player_mpris::init(
                position_receiver,
                tracklist_receiver,
                volume_receiver,
                status_receiver,
                controls,
                exit_sender,
                "io.github.sofusa.qobine".to_string(),
            )
            .await
            {
                error_exit(e);
            }
        });
    }

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

    let (
        available_devices_tx,
        available_devices_rx,
        active_device_tx,
        active_device_rx,
        set_active_device_tx,
        set_active_device_rx,
    ) = if configuration.enable_disconnect {
        let (available_devices_tx, available_devices_rx) = watch::channel(Default::default());

        let (active_device_tx, active_device_rx) = watch::channel(Default::default());

        let (set_active_device_tx, set_active_device_rx) = mpsc::unbounded_channel();

        (
            Some(available_devices_tx),
            Some(available_devices_rx),
            Some(active_device_tx),
            Some(active_device_rx),
            Some(set_active_device_tx),
            Some(set_active_device_rx),
        )
    } else {
        (None, None, None, None, None, None)
    };

    let client = client.clone();

    let controls = player.controls();
    let tracklist_receiver = player.tracklist();
    let status_receiver = player.status();
    let position_receiver = player.position();
    let volume_receiver = player.volume();
    let database_clone = database.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = qobuz_player_gtk::init(
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
        ) {
            error_exit(e);
        };
    });

    if let (
        Some(server_url),
        Some(password),
        Some(device_name),
        Some(available_devices_tx),
        Some(active_device_tx),
        Some(set_active_device_rx),
    ) = (
        configuration.disconnect_server_url,
        configuration.disconnect_password,
        configuration.device_name,
        available_devices_tx,
        active_device_tx,
        set_active_device_rx,
    ) {
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let controls = player.controls();
        let active_sender = player.active_sender();
        let auto_play_receiver = player.auto_play();

        let tracklist_sender = player.tracklist_sender();
        let position_sender = player.position_sender();
        let status_sender = player.status_sender();
        let volume_sender = player.volume_sender();
        let auto_play_sender = player.auto_play_sender();

        tokio::spawn(async move {
            if let Err(e) = qobuz_player_disconnect::init(
                &server_url,
                &password,
                &device_name,
                controls,
                tracklist_sender,
                position_sender,
                volume_sender,
                auto_play_sender,
                status_sender,
                active_sender,
                available_devices_tx,
                active_device_tx,
                position_receiver,
                tracklist_receiver,
                status_receiver,
                volume_receiver,
                auto_play_receiver,
                set_active_device_rx,
            )
            .await
            {
                error_exit(e);
            }
        });
    }

    player.player_loop(exit_receiver).await?;

    Ok(())
}

fn error_exit(error: Error) {
    eprintln!("{error}");
    std::process::exit(1);
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn sleep_inhibitor(mut status_receiver: StatusReceiver) {
    std::thread::spawn(move || {
        let mut sleep_inhibitor = SleepInhibitor::new();

        loop {
            use futures::executor::block_on;
            use qobuz_player_controls::Status;

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
