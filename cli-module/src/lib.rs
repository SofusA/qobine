use clap::{Args, Subcommand};
use player_module::{
    AppResult, AudioQuality, client::Client, database::Database, error::Error,
    notification::NotificationBroadcast, player::Player,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_schedule::{Job, every};

#[derive(Args, Debug)]
pub struct SharedArgs {
    #[clap(long)]
    pub audio_cache: Option<PathBuf>,

    #[clap(long, default_value_t = 1)]
    pub audio_cache_time_to_live: u32,

    #[clap(short, long)]
    /// Provide max audio quality (overrides any configured value)
    pub max_audio_quality: Option<AudioQuality>,

    #[clap(long)]
    /// Use provided device for audio output, instead of default.
    /// Use qobuz-player list-devices for output device list
    pub output_device_id: Option<String>,

    /// Use the file based streaming endpoint instead endpoint from web player
    /// Less CPU intense
    #[clap(long, default_value_t = false)]
    pub file_based_streaming: bool,
}

#[derive(Args, Debug)]
pub struct ConnectArgs {
    #[clap(long)]
    pub connect: bool,

    #[clap(flatten)]
    pub name_args: ConnectNameArgs,
}

#[derive(Args, Debug)]
pub struct RfidArgs {
    #[clap(long)]
    /// Use other qobuz-player with web for rfid database
    pub rfid_server_base_address: Option<String>,

    #[clap(long)]
    /// Secret for optional qobuz-player rfid server
    pub rfid_server_secret: Option<String>,
}

#[derive(Args, Debug)]
pub struct ConnectNameArgs {
    #[clap(long, default_value = "qobuz-player")]
    pub connect_name: String,

    #[clap(long, default_value_t = 0)]
    /// Port for the Qobuz Connect session manager. Defaults to 0 (random port).
    pub connect_port: u16,
}

#[derive(Args, Debug)]
pub struct GpioArgs {
    #[clap(long, default_value_t = false)]
    /// Enable gpio interface for raspberry pi. Pin 16 (gpio-23) will be high when playing
    pub gpio: bool,
}

#[derive(Args, Debug)]
pub struct DelayArgs {
    #[clap(long)]
    /// Delay playback when changing state from paused to playing in milliseconds
    pub state_change_delay_ms: Option<u64>,

    #[clap(long)]
    /// Delay playback when changing sample rate in milliseconds
    pub sample_rate_change_delay_ms: Option<u64>,
}

#[derive(Args, Debug)]
pub struct DisconnectArgs {
    /// Disconnect device name
    #[clap(long)]
    disconnect_device_name: Option<String>,

    /// Disconnect password
    #[clap(long)]
    disconnect_password: Option<String>,

    /// Disconnect server url
    #[clap(long)]
    disconnect_server_url: Option<String>,
}

pub struct ParsedDisconnect {
    pub device_name: String,
    pub password: String,
    pub server_url: String,
}

impl TryFrom<DisconnectArgs> for Option<ParsedDisconnect> {
    type Error = &'static str;

    fn try_from(args: DisconnectArgs) -> Result<Self, Self::Error> {
        match (
            args.disconnect_device_name,
            args.disconnect_password,
            args.disconnect_server_url,
        ) {
            (None, None, None) => Ok(None),

            (Some(device_name), Some(password), Some(server_url)) => Ok(Some(ParsedDisconnect {
                device_name,
                password,
                server_url,
            })),

            _ => Err(
                "disconnect-device-name, disconnect-password and disconnect-server-url must all be provided",
            ),
        }
    }
}

pub fn parse_disconnect_args(args: DisconnectArgs) -> Option<ParsedDisconnect> {
    Option::<ParsedDisconnect>::try_from(args).unwrap_or_else(|msg| {
        eprintln!("{msg}");
        None
    })
}

#[derive(Subcommand, Debug)]
pub enum SharedCommands {
    /// Authenticate with Qobuz via browser
    Login,

    /// Logout from Qobuz
    Logout,

    /// Persistently set the maximum audio quality
    SetMaxAudioQuality {
        #[clap(value_enum)]
        quality: AudioQuality,
    },
}

pub async fn handle_shared_commands(command: SharedCommands, database: &Database) -> AppResult<()> {
    match command {
        SharedCommands::Login => {
            let (_client, oauth_result) =
                Client::new_with_oauth_login(AudioQuality::Mp3, false, true).await?;

            database.set_credentials(Some(oauth_result.into())).await?;
            println!("Login successful! You can now run qobuz-player.");
            Ok(())
        }
        SharedCommands::Logout => {
            database.clear_user_auth_token().await?;
            println!("Logout successful!");
            Ok(())
        }
        SharedCommands::SetMaxAudioQuality { quality } => {
            database.set_max_audio_quality(quality).await?;

            println!("Max audio quality saved.");
            Ok(())
        }
    }
}

pub async fn get_client(
    database: &Database,
    max_audio_quality: AudioQuality,
    file_based_streaming: bool,
    headless: bool,
) -> AppResult<Client> {
    let database_credentials = database.get_credentials().await?;

    let client = match database_credentials {
        Some(credentials) => {
            Client::new(Some(credentials), max_audio_quality, file_based_streaming)
        }
        None => {
            let (client, oauth_result) =
                Client::new_with_oauth_login(max_audio_quality, file_based_streaming, headless)
                    .await?;

            database.set_credentials(Some(oauth_result.into())).await?;

            client
        }
    };

    Ok(client)
}

pub fn spawn_clean_up(database: Arc<Database>, audio_cache_time_to_live: u32) {
    if audio_cache_time_to_live != 0 {
        let clean_up_schedule = every(1).hour().perform(move || {
            let database = database.clone();
            async move {
                if let Ok(deleted_paths) = database
                    .clean_up_cache_entries(time::Duration::hours(audio_cache_time_to_live.into()))
                    .await
                {
                    for path in deleted_paths {
                        _ = tokio::fs::remove_file(path.as_path()).await;
                    }
                };
            }
        });

        tokio::spawn(clean_up_schedule);
    }
}

pub fn spawn_clean_up_mut(
    database: Arc<Database>,
    initial_ttl: Option<u32>,
    mut ttl_rx: mpsc::UnboundedReceiver<u32>,
) {
    tokio::spawn(async move {
        let mut ttl = initial_ttl.unwrap_or(0);
        let mut ttl_rx_closed = false;

        let mut interval = tokio::time::interval(Duration::from_hours(1));

        loop {
            tokio::select! {
                new_ttl = ttl_rx.recv(), if !ttl_rx_closed => {
                    match new_ttl {
                        Some(new_ttl) => {
                            database.set_cache_ttl_hours(new_ttl).await.unwrap();

                            ttl = new_ttl;
                        }

                        None => {
                            ttl_rx_closed = true;
                        }
                    }
                }

                _ = interval.tick() => {
                    if ttl == 0 {
                        continue;
                    }

                    match database
                        .clean_up_cache_entries(time::Duration::hours(ttl.into()))
                        .await
                    {
                        Ok(deleted_paths) => {
                            for path in deleted_paths {
                                let _ = tokio::fs::remove_file(&path).await;
                            }
                        }

                        Err(err) => {
                            tracing::error!("Failed to clean up cache entries: {err:?}");
                        }
                    }
                }
            }
        }
    });
}

pub fn default_audio_cache(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| {
        let mut cache_dir = std::env::temp_dir();
        cache_dir.push("qobuz-player-cache");
        cache_dir
    })
}

pub async fn default_audio_quality(
    database: &Database,
    args: Option<AudioQuality>,
) -> AppResult<AudioQuality> {
    match args {
        Some(quality) => Ok(quality),
        None => {
            let database_configuration = database.get_configuration().await?;
            Ok(database_configuration.max_audio_quality)
        }
    }
}

pub async fn create_player(
    audio_cache: PathBuf,
    database: Arc<Database>,
    client: Arc<Client>,
    broadcast: Arc<NotificationBroadcast>,
    state_change_delay_ms: Option<u64>,
    sample_rate_change_delay_ms: Option<u64>,
    output_device_id: Option<String>,
) -> AppResult<Player> {
    let tracklist = database.get_tracklist().await.unwrap_or_default();
    let configuration = database.get_configuration().await?;

    let state_change_delay = state_change_delay_ms.map(Duration::from_millis);
    let sample_rate_change_delay = sample_rate_change_delay_ms.map(Duration::from_millis);

    let player = Player::new(
        tracklist,
        client,
        configuration.volume,
        configuration.auto_play,
        broadcast,
        audio_cache,
        database,
        state_change_delay,
        sample_rate_change_delay,
        output_device_id,
    )?;

    Ok(player)
}

pub fn error_exit(error: Error) {
    eprintln!("{error}");
    std::process::exit(1);
}
