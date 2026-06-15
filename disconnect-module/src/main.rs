use cli_module::{
    DelayArgs, SharedArgs, SharedCommands, create_player, default_audio_cache,
    default_audio_quality, error_exit, get_client, handle_shared_commands, spawn_clean_up,
};
use disconnect_module::{DisconnectClientConfig, spawn_disconnect};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use clap::Parser;
use player_module::{AppResult, database::Database, notification::NotificationBroadcast};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Arguments {
    /// Device name
    #[clap(short, long)]
    device_name: String,

    /// Password
    #[clap(short, long)]
    password: String,

    /// Server url
    #[clap(short, long)]
    server_url: String,

    #[clap(flatten)]
    shared: SharedArgs,

    #[clap(flatten)]
    delay: DelayArgs,

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

    let (available_devices_tx, _) = watch::channel(Default::default());
    let (active_device_sender, _) = watch::channel(Default::default());
    let (_, active_device_receiver) = mpsc::unbounded_channel();

    let (_, config_rx) = watch::channel(Some(DisconnectClientConfig {
        server_url: args.server_url,
        password: args.password,
        device_name: args.device_name,
    }));

    spawn_disconnect(
        &player,
        config_rx,
        available_devices_tx,
        active_device_sender,
        active_device_receiver,
    );

    spawn_clean_up(database, args.shared.audio_cache_time_to_live);
    player.player_loop(exit_receiver).await?;

    Ok(())
}
