use cli_module::ConnectArgs;
#[cfg(feature = "gpio")]
use cli_module::GpioArgs;
use cli_module::{
    DelayArgs, DisconnectArgs, ParsedDisconnect, RfidArgs, SharedArgs, SharedCommands,
    SharedPlaybackConfig, create_player, default_audio_quality, get_client, handle_shared_commands,
    parse_disconnect_args, spawn_clean_up,
};
use rfid_module::{RfidState, spawn_rfid};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use clap::Parser;
use player_module::{AppResult, database::Database, notification::NotificationBroadcast};

#[derive(Parser)]
#[clap(author, about, long_about = None)]
struct Arguments {
    #[clap(flatten)]
    rfid_config: RfidArgs,

    #[clap(flatten)]
    delay: DelayArgs,

    #[clap(flatten)]
    shared: SharedArgs,

    #[cfg(feature = "gpio")]
    #[clap(flatten)]
    gpio: GpioArgs,

    #[clap(flatten)]
    disconnect: DisconnectArgs,

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
            eprintln!("{err}");
        }
    }
}

pub async fn run() -> AppResult<()> {
    let args = Arguments::parse();
    let database = Arc::new(Database::new().await?);
    let headless = true;

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

    let disconnect_args = parse_disconnect_args(args.disconnect);

    let shared_playback_config = if let Some(disconnect) = disconnect_args.as_ref() {
        Some(SharedPlaybackConfig::Disconnect(ParsedDisconnect {
            server_url: disconnect.server_url.clone(),
            password: disconnect.password.clone(),
            device_name: disconnect.device_name.clone(),
        }))
    } else if args.connect.connect {
        Some(SharedPlaybackConfig::Connect(
            args.connect.name_args.connect_name.clone(),
        ))
    } else {
        None
    };

    let shared_playback_enabled = shared_playback_config.is_some();
    let shared_playback_config_channel = shared_playback_config.map(watch::channel);

    let (shared_devices_tx, shared_devices_rx) = watch::channel(Vec::new());
    let (activate_shared_device_tx, activate_shared_device_rx) = mpsc::unbounded_channel();

    if let Some((config_tx, shared_playback_config_rx)) = shared_playback_config_channel {
        let shared_playback_player = player.bindings();
        let shared_playback_client = client.clone();
        let shared_playback_exit_sender = exit_sender.clone();

        tokio::spawn(async move {
            // Keep the sender alive while shared playback is running.
            let _config_tx = config_tx;

            if let Err(err) = shared_playback_module::init(
                shared_playback_player,
                shared_playback_client,
                shared_playback_config_rx,
                max_audio_quality,
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
    }

    spawn_rfid(
        &player,
        exit_sender.clone(),
        database.clone(),
        broadcast,
        args.rfid_config.rfid_server_base_address,
        args.rfid_config.rfid_server_secret,
        RfidState::default(),
        shared_playback_enabled.then_some(shared_devices_rx),
        shared_playback_enabled.then_some(activate_shared_device_tx),
    );

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

    spawn_clean_up(database, args.shared.audio_cache_time_to_live);
    player.player_loop(exit_receiver).await?;

    Ok(())
}
