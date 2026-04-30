use qobuz_player_cli::{
    ConnectArgs, SharedArgs, create_player, default_audio_quality, spawn_clean_up,
};
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use qobuz_player_controls::StatusReceiver;
use std::sync::Arc;
use tokio::sync::broadcast;

use clap::Parser;
use qobuz_player_controls::{
    AppResult,
    client::{Client, get_app_id},
    database::Database,
    error::Error,
    notification::NotificationBroadcast,
};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Arguments {
    #[clap(flatten)]
    shared: SharedArgs,

    #[clap(flatten)]
    connect: ConnectArgs,
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

    let args = Arguments::parse();

    let database = Arc::new(Database::new().await?);

    let (exit_sender, exit_receiver) = broadcast::channel(5);

    let max_audio_quality = default_audio_quality(&database, args.shared.max_audio_quality).await?;
    let credentials = database.get_credentials().await?;

    let app_id = get_app_id().await?;
    let client = Arc::new(Client::new(credentials, max_audio_quality));

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

    #[cfg(target_os = "linux")]
    {
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let controls = player.controls();
        let exit_sender = exit_sender.clone();
        tokio::spawn(async move {
            if let Err(e) = qobuz_player_mpris::init(
                position_receiver,
                tracklist_receiver,
                volume_receiver,
                status_receiver,
                controls,
                exit_sender,
            )
            .await
            {
                error_exit(e);
            }
        });
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    {
        let status_receiver = player.status();
        sleep_inhibitor(status_receiver);
    }

    let client = client.clone();

    if args.connect.connect {
        let position_receiver = player.position();
        let tracklist_receiver = player.tracklist();
        let volume_receiver = player.volume();
        let status_receiver = player.status();
        let controls = player.controls();
        let app_id = app_id.clone();

        tokio::spawn(async move {
            if let Err(e) = qobuz_player_connect::init(
                &app_id,
                args.connect.name_args.connect_name,
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

    let controls = player.controls();
    let tracklist_receiver = player.tracklist();
    let status_receiver = player.status();
    let position_receiver = player.position();
    let database_clone = database.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = qobuz_player_gtk::init(
            client,
            app_id,
            tracklist_receiver,
            status_receiver,
            position_receiver,
            controls,
            database_clone,
            exit_sender,
        ) {
            error_exit(e);
        };
    });

    spawn_clean_up(database, args.shared.audio_cache_time_to_live);
    player.player_loop(exit_receiver).await?;

    Ok(())
}

fn error_exit(error: Error) {
    eprintln!("{error}");
    std::process::exit(1);
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn sleep_inhibitor(mut status_receiver: StatusReceiver) {
    std::thread::spawn(move || {
        let mut sleep_inhibitor = SleepInhibitor::new();

        loop {
            use futures::executor::block_on;
            use qobuz_player_controls::Status;

            let changed = block_on(async { status_receiver.changed().await });
            if changed.is_err() {
                sleep_inhibitor.restore_sleep();
                break;
            }

            let status = *status_receiver.borrow_and_update();
            match status {
                Status::Paused | Status::Stopped => sleep_inhibitor.restore_sleep(),
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
