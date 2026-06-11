use qobuz_player_cli::{
    DelayArgs, SharedArgs, SharedCommands, create_player, default_audio_cache,
    default_audio_quality, get_client, handle_shared_commands, spawn_clean_up,
};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use clap::Parser;
use qobuz_player_player::{
    AppResult, database::Database, error::Error, notification::NotificationBroadcast,
};

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

    {
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let auto_play_receiver = player.auto_play();
        let controls = player.controls();

        let tracklist_sender = player.tracklist_sender();
        let position_sender = player.position_sender();
        let status_sender = player.status_sender();
        let volume_sender = player.volume_sender();
        let active_sender = player.active_sender();
        let auto_play_sender = player.auto_play_sender();

        tokio::spawn(async move {
            if let Err(e) = qobuz_player_disconnect::init(
                &args.server_url,
                &args.password,
                &args.device_name,
                controls,
                tracklist_sender,
                position_sender,
                volume_sender,
                auto_play_sender,
                status_sender,
                active_sender,
                available_devices_tx,
                active_device_sender,
                position_receiver,
                tracklist_receiver,
                status_receiver,
                volume_receiver,
                auto_play_receiver,
                active_device_receiver,
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
