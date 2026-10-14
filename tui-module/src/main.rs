use cli_module::{
    ParsedDisconnect, SharedArgs, SharedCommands, SharedPlaybackConfig, create_player,
    default_audio_quality, get_client, handle_shared_commands, spawn_clean_up_mut,
};
use disconnect_module::spawn_disconnect;
use futures::executor::block_on;
#[cfg(target_os = "linux")]
use mpris_module::spawn_mpris;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use clap::Parser;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use controls_module::StatusReceiver;
use player_module::{AppResult, database::Database, notification::NotificationBroadcast};

#[derive(Parser)]
#[clap(author, about, long_about = None)]
struct Arguments {
    #[clap(flatten)]
    shared: SharedArgs,

    #[clap(subcommand)]
    command: Option<SharedCommands>,
}

#[cfg(not(target_os = "macos"))]
#[tokio::main]
async fn main() {
    match run().await {
        Ok(()) => {}
        Err(err) => {
            eprintln!("{err}");
        }
    }
}

#[cfg(target_os = "macos")]
fn main() {
    macos_module::run_with_main_loop(|| async {
        match run().await {
            Ok(()) => {}
            Err(err) => {
                eprintln!("{err}");
            }
        }
    });
}

pub async fn run() -> AppResult<()> {
    let args = Arguments::parse();
    let database = Arc::new(Database::new().await?);
    let headless = false;
    let configuration = database.get_configuration().await?;

    if let Some(command) = args.command {
        handle_shared_commands(command, &database).await?;
        return Ok(());
    }

    let (exit_sender, exit_receiver) = broadcast::channel(5);

    let max_audio_quality = default_audio_quality(&database, args.shared.max_audio_quality).await?;
    let client = get_client(
        &database,
        max_audio_quality,
        args.shared.file_based_streaming,
        headless,
    )
    .await?;
    let client = Arc::new(client);

    let broadcast = Arc::new(NotificationBroadcast::new());

    let mut player = create_player(
        args.shared.audio_cache,
        database.clone(),
        client.clone(),
        broadcast.clone(),
        None,
        None,
        args.shared.output_device_id,
    )
    .await?;

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

    let (config_tx, config_rx) = watch::channel(shared_playback_config);

    #[cfg(target_os = "linux")]
    spawn_mpris(&player, &exit_sender, "qobine".to_string());

    #[cfg(target_os = "macos")]
    macos_module::spawn_now_playing(&player, &exit_sender);

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    {
        let status_receiver = player.status();
        sleep_inhibitor(status_receiver);
    }

    let client = client.clone();
    let broadcast = broadcast.clone();

    let position_receiver = player.position();
    let tracklist_receiver = player.tracklist();
    let status_receiver = player.status();
    let controls = player.controls();
    let client = client.clone();
    let broadcast = broadcast.clone();

    /*
     * UI channels for Disconnect.
     *
     * These remain alive even while Disconnect is disabled, allowing the TUI to
     * enable Disconnect later by updating config_tx.
     */
    let (available_devices_tx, available_devices_rx) = watch::channel(Vec::new());

    let (active_device_tx, active_device_rx) = watch::channel(String::new());

    let (set_active_device_tx, set_active_device_rx) = mpsc::unbounded_channel();

    /*
     * Start the Disconnect supervisor unconditionally.
     *
     * It waits while the configuration is Connect or None and starts its client
     * when the configuration becomes Disconnect.
     */
    spawn_disconnect(
        &player,
        exit_sender.clone(),
        config_rx.clone(),
        available_devices_tx.clone(),
        active_device_tx.clone(),
        set_active_device_rx,
    );

    let (connect_devices_tx, connect_devices_rx) = watch::channel(Vec::new());

    let (activate_connect_device_tx, activate_connect_device_rx) = mpsc::unbounded_channel();

    #[cfg(feature = "connect")]
    {
        let client = client.clone();
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let controls = player.controls();
        let connect_config_rx = config_rx.clone();

        tokio::spawn(async move {
            if let Err(err) = connect_module::init(
                client,
                connect_config_rx,
                controls,
                position_receiver,
                tracklist_receiver,
                status_receiver,
                volume_receiver,
                max_audio_quality,
                connect_devices_tx,
                activate_connect_device_rx,
                39621,
            )
            .await
            {
                eprintln!("{err}");
            }
        });
    }

    /*
     * Without the Connect feature, drop the receiving side used by the Connect
     * service. The UI still receives an empty device list and can retain the same
     * interface.
     */
    #[cfg(not(feature = "connect"))]
    drop((connect_devices_tx, activate_connect_device_rx));

    let (ttl_tx, ttl_rx) = mpsc::unbounded_channel::<u32>();
    spawn_clean_up_mut(
        database.clone(),
        Some(configuration.cache_ttl_hours),
        ttl_rx,
    );

    tokio::spawn(async move {
        if let Err(err) = tui_module::init(
            client,
            broadcast,
            controls,
            position_receiver,
            tracklist_receiver,
            status_receiver,
            exit_sender.clone(),
            ttl_tx,
            database,
            available_devices_rx,
            active_device_rx,
            set_active_device_tx,
            connect_devices_rx,
            activate_connect_device_tx,
            config_tx,
        )
        .await
        {
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
