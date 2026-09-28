#[cfg(feature = "gpio")]
use cli_module::GpioArgs;
use cli_module::{
    ConnectNameArgs, DelayArgs, SharedArgs, SharedCommands, create_player, default_audio_quality,
    get_client, handle_shared_commands, spawn_clean_up,
};
use player_module::{
    AppResult, client::StreamClient, database::Database, notification::NotificationBroadcast,
};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

use clap::Parser;

#[derive(Parser)]
#[clap(author, about, long_about = None)]
struct Arguments {
    #[clap(flatten)]
    shared: SharedArgs,

    #[clap(flatten)]
    delay: DelayArgs,

    #[clap(flatten)]
    connect: ConnectNameArgs,

    #[clap(long)]
    /// Skip the Qobuz login: the account is the one of the app that picks qobine on the LAN
    no_login: bool,

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
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let args = Arguments::parse();
    let database = Arc::new(Database::new().await?);
    let headless = true;

    if let Some(command) = args.command {
        handle_shared_commands(command, &database).await?;
        return Ok(());
    }

    let (exit_sender, exit_receiver) = broadcast::channel(5);

    let max_audio_quality = default_audio_quality(&database, args.shared.max_audio_quality).await?;
    let client = if args.no_login {
        StreamClient::new(None, max_audio_quality, args.shared.file_based_streaming)
    } else {
        get_client(
            &database,
            max_audio_quality,
            args.shared.file_based_streaming,
            headless,
        )
        .await?
    };
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

    {
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
                args.connect.connect_name,
                controls,
                position_receiver,
                tracklist_receiver,
                status_receiver,
                volume_receiver,
                max_audio_quality,
                connect_devices,
                activations,
                args.connect.connect_port,
            )
            .await
            {
                _ = exit_sender.send(true);
                eprintln!("{err}");
            }
        });
    }

    spawn_clean_up(database, args.shared.audio_cache_time_to_live);
    player.player_loop(exit_receiver).await?;

    Ok(())
}
