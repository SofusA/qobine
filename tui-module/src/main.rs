use cli_module::{
    ConnectArgs, SharedArgs, SharedCommands, create_player, default_audio_cache,
    default_audio_quality, error_exit, get_client, handle_shared_commands, spawn_clean_up_mut,
};
use disconnect_module::{DisconnectClientConfig, spawn_disconnect};
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
#[clap(author, version, about, long_about = None)]
struct Arguments {
    /// Disable the album cover image
    #[clap(long)]
    disable_album_cover: bool,

    #[clap(flatten)]
    shared: SharedArgs,

    #[clap(flatten)]
    connect: ConnectArgs,

    #[clap(subcommand)]
    command: Option<SharedCommands>,
}

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
    let audio_cache = default_audio_cache(args.shared.audio_cache);

    let mut player = create_player(
        audio_cache,
        database.clone(),
        client.clone(),
        broadcast.clone(),
        None,
        None,
        args.shared.output_device_id,
    )
    .await?;

    #[cfg(target_os = "linux")]
    spawn_mpris(&player, &exit_sender, "qobuz-player".to_string());

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    {
        let status_receiver = player.status();
        sleep_inhibitor(status_receiver);
    }

    let position_receiver = player.position();
    let tracklist_receiver = player.tracklist();
    let status_receiver = player.status();
    let controls = player.controls();
    let client = client.clone();
    let broadcast = broadcast.clone();

    if args.connect.connect {
        let app_id = client.app_id().await?;
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let controls = player.controls();

        tokio::spawn(async move {
            if let Err(e) = connect_module::init(
                &app_id,
                args.connect.name_args.connect_name,
                args.connect.name_args.connect_port,
                controls,
                position_receiver,
                tracklist_receiver,
                status_receiver,
                volume_receiver,
                max_audio_quality,
            )
            .await
            {
                error_exit(e);
            }
        });
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

    tokio::spawn(async move {
        if let Err(e) = tui_module::init(
            client,
            broadcast,
            controls,
            position_receiver,
            tracklist_receiver,
            status_receiver,
            exit_sender,
            ttl_tx,
            args.disable_album_cover,
            database,
            available_devices_rx,
            active_device_rx,
            set_active_device_tx,
            config_tx,
        )
        .await
        {
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
