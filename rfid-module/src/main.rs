#[cfg(feature = "gpio")]
use cli_module::GpioArgs;
use cli_module::{
    ConnectArgs, DelayArgs, DisconnectArgs, RfidArgs, SharedArgs, SharedCommands, create_player,
    default_audio_cache, default_audio_quality, error_exit, get_client, handle_shared_commands,
    parse_disconnect_args, spawn_clean_up,
};
use disconnect_module::{DisconnectClientConfig, spawn_disconnect};
use rfid_module::{RfidState, spawn_rfid};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use clap::Parser;
use player_module::{AppResult, database::Database, notification::NotificationBroadcast};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
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
            error_exit(err);
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

    let disconnect_args = parse_disconnect_args(args.disconnect);
    let (
        available_devices_tx,
        active_device_tx,
        set_active_device_tx,
        set_active_device_rx,
        config_rx,
    ) = if let Some(disconnect_args) = disconnect_args.as_ref() {
        let (available_devices_tx, _) = watch::channel(Default::default());
        let (active_device_tx, _) = watch::channel(Default::default());
        let (set_active_device_tx, set_active_device_rx) = mpsc::unbounded_channel();
        let (_, config_rx) = watch::channel(Some(DisconnectClientConfig {
            server_url: disconnect_args.server_url.clone(),
            password: disconnect_args.password.clone(),
            device_name: disconnect_args.device_name.clone(),
        }));

        (
            Some(available_devices_tx),
            Some(active_device_tx),
            Some(set_active_device_tx),
            Some(set_active_device_rx),
            Some(config_rx),
        )
    } else {
        (None, None, None, None, None)
    };

    #[cfg(feature = "gpio")]
    if args.gpio.gpio {
        let status_receiver = player.status();
        let active_receiver = player.active();
        tokio::spawn(async move {
            if let Err(e) = gpio_module::init(status_receiver, active_receiver).await {
                error_exit(e.into());
            }
        });
    }

    spawn_rfid(
        &player,
        database.clone(),
        broadcast,
        disconnect_args.as_ref().map(|x| x.device_name.clone()),
        args.rfid_config.rfid_server_base_address,
        args.rfid_config.rfid_server_secret,
        RfidState::default(),
        set_active_device_tx,
    );

    if let (
        Some(config_rx),
        Some(available_devices_tx),
        Some(active_device_tx),
        Some(set_active_device_rx),
    ) = (
        config_rx,
        available_devices_tx,
        active_device_tx,
        set_active_device_rx,
    ) {
        spawn_disconnect(
            &player,
            config_rx,
            available_devices_tx,
            active_device_tx,
            set_active_device_rx,
        );
    }

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

    spawn_clean_up(database, args.shared.audio_cache_time_to_live);
    player.player_loop(exit_receiver).await?;

    Ok(())
}
