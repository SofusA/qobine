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

    #[cfg(feature = "connect")]
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
        SharedPlaybackConfig::Disconnect(ParsedDisconnect {
            server_url: disconnect.server_url.clone(),
            password: disconnect.password.clone(),
            device_name: disconnect.device_name.clone(),
        })
    } else {
        #[cfg(feature = "connect")]
        {
            if args.connect.connect {
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

    let (_, shared_playback_config_rx) = watch::channel(shared_playback_config.clone());

    let (available_devices_tx, active_device_tx, set_active_device_tx, set_active_device_rx) =
        if matches!(&shared_playback_config, SharedPlaybackConfig::Disconnect(_)) {
            let (available_devices_tx, _) = watch::channel(Vec::new());
            let (active_device_tx, _) = watch::channel(String::new());
            let (set_active_device_tx, set_active_device_rx) = mpsc::unbounded_channel();

            (
                Some(available_devices_tx),
                Some(active_device_tx),
                Some(set_active_device_tx),
                Some(set_active_device_rx),
            )
        } else {
            (None, None, None, None)
        };

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

    spawn_rfid(
        &player,
        exit_sender.clone(),
        database.clone(),
        broadcast,
        shared_playback_config_rx.clone(),
        args.rfid_config.rfid_server_base_address,
        args.rfid_config.rfid_server_secret,
        RfidState::default(),
        set_active_device_tx,
    );

    if let (Some(available_devices_tx), Some(active_device_tx), Some(set_active_device_rx)) =
        (available_devices_tx, active_device_tx, set_active_device_rx)
    {
        spawn_disconnect(
            &player,
            exit_sender,
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

    spawn_clean_up(database, args.shared.audio_cache_time_to_live);
    player.player_loop(exit_receiver).await?;

    Ok(())
}
