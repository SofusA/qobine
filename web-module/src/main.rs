#[cfg(feature = "connect")]
use cli_module::ConnectArgs;
#[cfg(feature = "gpio")]
use cli_module::GpioArgs;
use cli_module::{
    DelayArgs, DisconnectArgs, ParsedDisconnect, RfidArgs, SharedArgs, SharedCommands,
    SharedPlaybackConfig, create_player, default_audio_quality, get_client, handle_shared_commands,
    parse_disconnect_args, spawn_clean_up,
};
use disconnect_module::spawn_disconnect;
use rfid_module::{RfidState, spawn_rfid};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use clap::Parser;
use player_module::{AppResult, database::Database, notification::NotificationBroadcast};

#[derive(Parser)]
#[clap(author, about, long_about = None)]
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

    #[cfg(target_os = "linux")]
    #[clap(long, default_value_t = false)]
    /// Enable mpris interface
    mpris: bool,

    #[clap(flatten)]
    rfid_config: RfidArgs,

    #[clap(flatten)]
    delay: DelayArgs,

    #[clap(flatten)]
    shared: SharedArgs,

    #[cfg(feature = "connect")]
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
            eprintln!("{err}");
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
        args.delay.state_change_delay_ms,
        args.delay.sample_rate_change_delay_ms,
        args.shared.output_device_id,
    )
    .await?;

    let rfid_state = args.rfid.then(RfidState::default);

    let disconnect_args = parse_disconnect_args(args.disconnect);
    let mut device_name = disconnect_args.clone().map(|x| x.device_name);

    let shared_playback_config = if let Some(disconnect) = disconnect_args.as_ref() {
        SharedPlaybackConfig::Disconnect(ParsedDisconnect {
            server_url: disconnect.server_url.clone(),
            password: disconnect.password.clone(),
            device_name: disconnect.device_name.clone(),
        })
    } else {
        #[cfg(feature = "connect")]
        {
            if args.connect.connect {
                device_name = Some(args.connect.name_args.connect_name.clone());
                SharedPlaybackConfig::Connect(args.connect.name_args.connect_name.clone())
            } else {
                SharedPlaybackConfig::None
            }
        }

        #[cfg(not(feature = "connect"))]
        {
            SharedPlaybackConfig::None
        }
    };

    let (_config_tx, shared_playback_config_rx) = watch::channel(shared_playback_config.clone());

    let (
        available_devices_tx,
        available_devices_rx,
        active_device_tx,
        active_device_rx,
        set_active_device_tx,
        set_active_device_rx,
    ) = if matches!(&shared_playback_config, SharedPlaybackConfig::Disconnect(_)) {
        let (available_devices_tx, available_devices_rx) = watch::channel(Vec::new());

        let (active_device_tx, active_device_rx) = watch::channel(String::new());

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
        let set_active_device_tx = set_active_device_tx.clone();
        let auto_play = player.auto_play();
        let exit_sender = exit_sender.clone();

        tokio::spawn(async move {
            if let Err(err) = web_module::init(
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
                device_name,
                available_devices_rx,
                active_device_rx,
                set_active_device_tx,
            )
            .await
            {
                _ = exit_sender.send(true);
                eprintln!("{err}");
            }
        });
    }

    #[cfg(feature = "gpio")]
    if args.gpio.gpio {
        let status_receiver = player.status();
        let active_receiver = player.active();
        tokio::spawn(async move {
            if let Err(err) = gpio_module::init(status_receiver, active_receiver).await {
                eprintln!("{err}");
            }
        });
    }

    if let Some(rfid_state) = rfid_state {
        spawn_rfid(
            &player,
            exit_sender.clone(),
            database.clone(),
            broadcast,
            shared_playback_config_rx.clone(),
            args.rfid_config.rfid_server_base_address,
            args.rfid_config.rfid_server_secret,
            rfid_state,
            set_active_device_tx,
        );
    }

    if let (
        SharedPlaybackConfig::Disconnect(_),
        Some(available_devices_tx),
        Some(active_device_tx),
        Some(set_active_device_rx),
    ) = (
        &shared_playback_config,
        available_devices_tx,
        active_device_tx,
        set_active_device_rx,
    ) {
        spawn_disconnect(
            &player,
            exit_sender.clone(),
            shared_playback_config_rx.clone(),
            available_devices_tx,
            active_device_tx,
            set_active_device_rx,
        );
    }

    #[cfg(feature = "connect")]
    if args.connect.connect {
        let client = client.clone();
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let controls = player.controls();
        let (connect_devices, _) = tokio::sync::watch::channel(Vec::new());
        let (_activate, activations) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            if let Err(err) = connect_module::init(
                client,
                shared_playback_config_rx,
                controls,
                position_receiver,
                tracklist_receiver,
                status_receiver,
                volume_receiver,
                max_audio_quality,
                connect_devices,
                activations,
                args.connect.name_args.connect_port,
            )
            .await
            {
                eprintln!("{err}");
            }
        });
    }

    #[cfg(target_os = "linux")]
    if args.mpris {
        use mpris_module::spawn_mpris;

        spawn_mpris(&player, &exit_sender, "qobine".to_string());
    }

    spawn_clean_up(database, args.shared.audio_cache_time_to_live);
    player.player_loop(exit_receiver).await?;

    Ok(())
}
