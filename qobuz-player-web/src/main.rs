#[cfg(feature = "gpio")]
use qobuz_player_cli::GpioArgs;
use qobuz_player_cli::{
    ConnectArgs, DelayArgs, DisconnectArgs, RfidArgs, SharedArgs, SharedCommands, create_player,
    default_audio_cache, default_audio_quality, get_client, handle_shared_commands,
    parse_disconnect_args, spawn_clean_up,
};
use qobuz_player_rfid::RfidState;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use clap::Parser;
use qobuz_player_player::{
    AppResult, database::Database, error::Error, notification::NotificationBroadcast,
};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Arguments {
    #[clap(long)]
    /// Secret used for web ui auth
    web_secret: Option<String>,

    #[clap(long, default_value_t = 9888)]
    /// Specify port for the web server
    port: u16,

    #[clap(long, default_value_t = false)]
    /// Enable rfid interface
    rfid: bool,

    #[clap(flatten)]
    rfid_config: RfidArgs,

    #[clap(flatten)]
    delay: DelayArgs,

    #[clap(flatten)]
    shared: SharedArgs,

    #[clap(flatten)]
    connect: ConnectArgs,

    #[clap(flatten)]
    disconnect: DisconnectArgs,

    #[cfg(feature = "gpio")]
    #[clap(flatten)]
    gpio: GpioArgs,

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
    tracing_subscriber::fmt().compact().init();
    let headless = true;

    let args = Arguments::parse();
    let database = Arc::new(Database::new().await?);

    if let Some(command) = args.command {
        handle_shared_commands(command, &database).await?;
        return Ok(());
    }

    let (_, exit_receiver) = broadcast::channel(5);

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
        args.delay.state_change_delay_ms,
        args.delay.sample_rate_change_delay_ms,
        args.shared.output_device_id,
    )
    .await?;

    let rfid_state = args.rfid.then(RfidState::default);

    let disconnect_args = parse_disconnect_args(args.disconnect);

    let (
        available_devices_tx,
        available_devices_rx,
        active_device_tx,
        active_device_rx,
        set_active_device_tx,
        set_active_device_rx,
    ) = if disconnect_args.is_some() {
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

    {
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let controls = player.controls();
        let broadcast = broadcast.clone();
        let client = client.clone();
        let database = database.clone();
        let rfid_state = rfid_state.clone();
        let active_device_rx = active_device_rx.clone();
        let connect_device_name = disconnect_args.as_ref().map(|x| x.device_name.clone());
        let set_active_device_tx = set_active_device_tx.clone();
        let auto_play = player.auto_play();

        tokio::spawn(async move {
            if let Err(e) = qobuz_player_web::init(
                controls,
                position_receiver,
                tracklist_receiver,
                volume_receiver,
                status_receiver,
                auto_play,
                args.port,
                args.web_secret,
                rfid_state,
                broadcast,
                client,
                database,
                connect_device_name,
                available_devices_rx,
                active_device_rx,
                set_active_device_tx,
            )
            .await
            {
                error_exit(e);
            }
        });
    }

    #[cfg(feature = "gpio")]
    if args.gpio.gpio {
        let status_receiver = player.status();
        let active_receiver = player.active();
        tokio::spawn(async move {
            if let Err(e) = qobuz_player_gpio::init(status_receiver, active_receiver).await {
                error_exit(e.into());
            }
        });
    }

    if let Some(rfid_state) = rfid_state {
        let controls = player.controls();
        let database = database.clone();
        let tracklist_receiver = player.tracklist();
        let connect_device_name = disconnect_args.as_ref().map(|x| x.device_name.clone());

        tokio::spawn(async move {
            if let Err(e) = qobuz_player_rfid::init(
                rfid_state,
                tracklist_receiver,
                controls,
                database,
                broadcast,
                args.rfid_config.rfid_server_base_address,
                args.rfid_config.rfid_server_secret,
                connect_device_name,
                set_active_device_tx,
            )
            .await
            {
                error_exit(e);
            }
        });
    }

    if let (
        Some(disconnect_args),
        Some(available_devices_tx),
        Some(active_device_tx),
        Some(set_active_device_rx),
    ) = (
        disconnect_args,
        available_devices_tx,
        active_device_tx,
        set_active_device_rx,
    ) {
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let auto_play_receiver = player.auto_play();
        let controls = player.controls();
        let active_sender = player.active_sender();

        let tracklist_sender = player.tracklist_sender();
        let position_sender = player.position_sender();
        let status_sender = player.status_sender();
        let volume_sender = player.volume_sender();
        let auto_play_sender = player.auto_play_sender();

        tokio::spawn(async move {
            if let Err(e) = qobuz_player_disconnect::init(
                &disconnect_args.server_url,
                &disconnect_args.password,
                &disconnect_args.device_name,
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

    if args.connect.connect {
        let app_id = client.app_id().await?;
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let controls = player.controls();

        tokio::spawn(async move {
            if let Err(e) = qobuz_player_connect::init(
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

    spawn_clean_up(database, args.shared.audio_cache_time_to_live);
    player.player_loop(exit_receiver).await?;

    Ok(())
}

fn error_exit(error: Error) {
    eprintln!("{error}");
    std::process::exit(1);
}
