use cli_module::{ParsedDisconnect, SharedPlaybackConfig, create_player, spawn_clean_up_mut};
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use controls_module::StatusReceiver;
#[cfg(target_os = "linux")]
use mpris_module::spawn_mpris;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use player_module::{
    AppResult, client::StreamClient, database::Database, notification::NotificationBroadcast,
};

#[tokio::main]
async fn main() {
    match run().await {
        Ok(()) => {}
        Err(err) => {
            eprintln!("{err}");
        }
    }
}

pub async fn run() -> AppResult<()> {
    tracing_subscriber::fmt().compact().init();

    let database = Arc::new(Database::new().await?);

    let (exit_sender, exit_receiver) = broadcast::channel(5);

    let credentials = database.get_credentials().await?;
    let configuration = database.get_configuration().await?;

    let client = Arc::new(StreamClient::new(
        credentials,
        configuration.max_audio_quality,
        configuration.use_file_based_streaming,
        database.clone(),
    ));
    let app_id = client.app_id().await?;

    let broadcast = Arc::new(NotificationBroadcast::new());

    let mut player = create_player(
        None,
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

    let shared_playback_config = if configuration.enable_connect {
        match configuration.device_name.clone() {
            Some(device_name) => SharedPlaybackConfig::Connect(device_name),
            None => SharedPlaybackConfig::None,
        }
    } else if configuration.enable_disconnect {
        match (
            configuration.disconnect_server_url.clone(),
            configuration.disconnect_password.clone(),
            configuration.device_name.clone(),
        ) {
            (Some(server_url), Some(password), Some(device_name)) => {
                SharedPlaybackConfig::Disconnect(ParsedDisconnect {
                    server_url,
                    password,
                    device_name,
                })
            }
            _ => SharedPlaybackConfig::None,
        }
    } else {
        SharedPlaybackConfig::None
    };

    let (shared_playback_config_tx, shared_playback_config_rx) =
        watch::channel(shared_playback_config);

    let (shared_devices_tx, shared_devices_rx) = watch::channel(Vec::new());
    let (activate_shared_device_tx, activate_shared_device_rx) = mpsc::unbounded_channel();
    let shared_playback_player = player.bindings();
    let shared_playback_client = client.clone();
    let shared_playback_exit_sender = exit_sender.clone();

    tokio::spawn(async move {
        if let Err(err) = shared_playback_module::init(
            shared_playback_player,
            shared_playback_client,
            shared_playback_config_rx,
            configuration.max_audio_quality,
            shared_devices_tx,
            activate_shared_device_rx,
            39621,
        )
        .await
        {
            _ = shared_playback_exit_sender.send(true);
            eprintln!("{err}");
        }
    });

    let client = client.clone();
    let controls = player.controls();
    let tracklist_receiver = player.tracklist();
    let status_receiver = player.status();
    let position_receiver = player.position();
    let volume_receiver = player.volume();
    let database_clone = database.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(err) = gtk_module::init(
            client,
            app_id,
            tracklist_receiver,
            status_receiver,
            position_receiver,
            volume_receiver,
            controls,
            database_clone,
            &exit_sender,
            ttl_tx,
            broadcast,
            shared_devices_rx,
            activate_shared_device_tx,
            shared_playback_config_tx,
        ) {
            _ = exit_sender.send(true);
            eprintln!("{err}");
        }
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
    const fn new() -> Self {
        Self { awake: None }
    }

    fn block_sleep(&mut self) {
        if self.awake.is_none() {
            let mut builder = keepawake::Builder::default();
            builder
                .idle(true)
                .sleep(true)
                .reason("Audio playback")
                .app_name("qobine");

            if let Ok(awake) = builder.create() {
                self.awake = Some(awake);
            }
        }
    }

    fn restore_sleep(&mut self) {
        let _ = self.awake.take();
    }
}
