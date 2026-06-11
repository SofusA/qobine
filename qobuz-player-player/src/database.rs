use crate::{AppResult, AudioQuality, Error};
use qobuz_player_client::client::OAuthResult;
use qobuz_player_controls::tracklist::Tracklist;
use serde_json::to_string;
use sqlx::types::Json;
use sqlx::{Pool, Sqlite, SqlitePool, sqlite::SqliteConnectOptions};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub struct Database {
    pool: Pool<Sqlite>,
}

impl Database {
    pub async fn new() -> AppResult<Self> {
        let database_url = if let Ok(url) = std::env::var("DATABASE_URL") {
            PathBuf::from(url.replace("sqlite://", ""))
        } else {
            let Some(mut url) = dirs::data_local_dir() else {
                return Err(Error::DatabaseLocationError);
            };
            url.push("qobuz-player");

            if !url.exists() {
                let Ok(_) = std::fs::create_dir_all(&url) else {
                    return Err(Error::DatabaseLocationError);
                };
            }

            url.push("data.db");

            url
        };

        let options = SqliteConnectOptions::new()
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .filename(database_url)
            .create_if_missing(true);

        let pool = SqlitePool::connect_with(options).await?;

        Database::init(pool).await
    }

    async fn init(pool: sqlx::Pool<sqlx::Sqlite>) -> AppResult<Self> {
        sqlx::migrate!("./migrations").run(&pool).await?;

        create_credentials_row(&pool).await?;
        create_configuration(&pool).await?;

        Ok(Self { pool })
    }

    pub async fn set_credentials(&self, credentials: Option<Credentials>) -> AppResult<()> {
        let token = credentials.as_ref().map(|c| c.user_auth_token.clone());
        let user_id = credentials.as_ref().map(|c| c.user_id);

        sqlx::query!(
            "UPDATE credentials SET user_auth_token = ?, user_id = ? WHERE rowid = 1",
            token,
            user_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn clear_user_auth_token(&self) -> AppResult<()> {
        sqlx::query!("update credentials set user_auth_token = null where rowid = 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_tracklist(&self, tracklist: &Tracklist) -> AppResult<()> {
        let serialized = to_string(&tracklist)?;

        sqlx::query!("delete from tracklist")
            .execute(&self.pool)
            .await?;

        sqlx::query!("insert into tracklist (tracklist) values (?1)", serialized)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn get_tracklist(&self) -> Option<Tracklist> {
        let row = sqlx::query_as!(
            TracklistDb,
            r#"SELECT tracklist as "tracklist: Json<Tracklist>" FROM tracklist"#
        )
        .fetch_one(&self.pool)
        .await;

        row.ok().map(|x| x.tracklist.0)
    }

    pub async fn set_volume(&self, volume: f32) -> AppResult<()> {
        sqlx::query!(
            r#"
             update configuration
             set volume=?1
             where rowid = 1
             "#,
            volume
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_auto_play(&self, auto_play: bool) -> AppResult<()> {
        sqlx::query!(
            r#"
             update configuration
             set auto_play=?1
             where rowid = 1
             "#,
            auto_play
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_cache_directory(&self, directory: &Path) -> AppResult<()> {
        let directory = directory
            .canonicalize()
            .map_err(|e| Error::StorageError {
                error: e.to_string(),
            })?
            .into_os_string()
            .into_string()
            .map_err(|_| Error::StorageError {
                error: "Error storing cache path".to_string(),
            })?;

        sqlx::query!(
            r#"
             update configuration
             set cache_directory=?1
             where rowid = 1
             "#,
            directory
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_cache_ttl_hours(&self, ttl: u32) -> AppResult<()> {
        sqlx::query!(
            r#"
             update configuration
             set cache_ttl_hours=?1
             where rowid = 1
             "#,
            ttl
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_use_file_based_streaming(
        &self,
        use_file_based_streaming: bool,
    ) -> AppResult<()> {
        sqlx::query!(
            r#"
             update configuration
             set use_file_based_streaming=?1
             where rowid = 1
             "#,
            use_file_based_streaming
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_max_audio_quality(&self, quality: AudioQuality) -> AppResult<()> {
        let quality_id = quality as i32;

        sqlx::query!(
            r#"
             update configuration
             set max_audio_quality=?1
             where rowid = 1
             "#,
            quality_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_credentials(&self) -> AppResult<Option<Credentials>> {
        let credentials = sqlx::query_as!(
            DatabaseCredentials,
            "select user_auth_token, user_id from credentials where rowid = 1"
        )
        .fetch_one(&self.pool)
        .await?;

        let credentials = match (credentials.user_auth_token, credentials.user_id) {
            (Some(token), Some(user_id)) => Some(Credentials {
                user_auth_token: token,
                user_id,
            }),
            _ => None,
        };

        Ok(credentials)
    }

    pub async fn get_configuration(&self) -> AppResult<Configuration> {
        let configuration = sqlx::query_as!(
            DatabaseConfiguration,
            r#"
             select
                 cache_directory,
                 cache_ttl_hours,
                 volume,
                 max_audio_quality,
                 use_file_based_streaming,
                 disconnect_server_url,
                 disconnect_password,
                 device_name,
                 enable_disconnect,
                 auto_play
             from configuration where rowid = 1"#
        )
        .fetch_one(&self.pool)
        .await?;

        let max_audio_quality = AudioQuality::from(configuration.max_audio_quality);
        let cache_directory = configuration
            .cache_directory
            .and_then(|x| PathBuf::from_str(&x).ok())
            .unwrap_or_else(|| {
                let mut cache_dir = std::env::temp_dir();
                cache_dir.push("qobuz-player-cache");
                cache_dir
            });

        let cache_ttl_hours = configuration.cache_ttl_hours.unwrap_or(1) as u32;
        let volume = configuration.volume.unwrap_or(1.0);
        let use_file_based_streaming = configuration.use_file_based_streaming.unwrap_or(false);
        let auto_play = configuration.auto_play.unwrap_or(false);

        Ok(Configuration {
            max_audio_quality,
            cache_directory,
            cache_ttl_hours,
            use_file_based_streaming,
            volume: volume as f32,
            enable_disconnect: configuration.enable_disconnect,
            device_name: configuration.device_name,
            disconnect_server_url: configuration.disconnect_server_url,
            disconnect_password: configuration.disconnect_password,
            auto_play,
        })
    }

    pub async fn add_rfid_reference(
        &self,
        rfid_id: String,
        reference: ReferenceType,
    ) -> AppResult<()> {
        match reference {
            ReferenceType::Album(id) => {
                let id = Some(id);

                sqlx::query!(
                     "insert into rfid_references (id, reference_type, album_id, playlist_id) values ($1, $2, $3, $4) on conflict(id) do update set reference_type = excluded.reference_type, album_id = excluded.album_id, playlist_id = excluded.playlist_id returning *",
                     rfid_id,
                     1,
                     id,
                     None::<u32>,
                 ).fetch_one(&self.pool).await?;
            }
            ReferenceType::Playlist(id) => {
                let id = Some(id);

                sqlx::query!(
                     "insert into rfid_references (id, reference_type, album_id, playlist_id) values ($1, $2, $3, $4) on conflict(id) do update set reference_type = excluded.reference_type, album_id = excluded.album_id, playlist_id = excluded.playlist_id returning *",
                     rfid_id,
                     2,
                     None::<String>,
                     id,
                 ).fetch_one(&self.pool).await?;
            }
        }
        Ok(())
    }

    pub async fn get_reference(&self, id: &str) -> Option<ReferenceType> {
        let db_reference = match sqlx::query_as!(
            RFIDReference,
            "select * from rfid_references where id = $1",
            id
        )
        .fetch_one(&self.pool)
        .await
        {
            Ok(res) => res,
            Err(_) => return None,
        };

        match db_reference.reference_type {
            ReferenceTypeDatabase::Album => Some(ReferenceType::Album(db_reference.album_id?)),
            ReferenceTypeDatabase::Playlist => {
                Some(ReferenceType::Playlist(db_reference.playlist_id? as u32))
            }
        }
    }

    pub async fn clean_up_cache_entries(
        &self,
        older_than: time::Duration,
    ) -> AppResult<Vec<PathBuf>> {
        let cutoff = time::OffsetDateTime::now_utc() - older_than;
        let cutoff_str = cutoff
            .format(&time::format_description::well_known::Rfc3339)
            .expect("infallible");

        let rows = sqlx::query!(
            "SELECT path FROM cache_entries WHERE last_opened < ?",
            cutoff_str
        )
        .fetch_all(&self.pool)
        .await?;

        sqlx::query!(
            "DELETE FROM cache_entries WHERE last_opened < ?",
            cutoff_str
        )
        .execute(&self.pool)
        .await?;

        let paths: Vec<PathBuf> = rows
            .into_iter()
            .map(|row| PathBuf::from(row.path))
            .collect();

        Ok(paths)
    }

    pub async fn set_cache_entry(&self, path: &Path) {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .expect("infallible");

        let path_str: String = path.to_string_lossy().into_owned();

        sqlx::query!(
            r#"
                 insert into cache_entries (path, last_opened)
                 values (?, ?)
                 on conflict(path) do update set
                     path = excluded.path,
                     last_opened = excluded.last_opened
             "#,
            path_str,
            now
        )
        .execute(&self.pool)
        .await
        .expect("infallible");
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum ReferenceType {
    Album(String),
    Playlist(u32),
}

#[derive(sqlx::FromRow)]
struct RFIDReference {
    #[allow(dead_code)]
    id: String,
    reference_type: ReferenceTypeDatabase,
    album_id: Option<String>,
    playlist_id: Option<i64>,
}

enum ReferenceTypeDatabase {
    Album = 1,
    Playlist = 2,
}

impl From<i64> for ReferenceTypeDatabase {
    fn from(value: i64) -> Self {
        match value {
            1 => ReferenceTypeDatabase::Album,
            2 => ReferenceTypeDatabase::Playlist,
            _ => panic!("Unable to parse reference type!"),
        }
    }
}

struct DatabaseCredentials {
    user_auth_token: Option<String>,
    user_id: Option<i64>,
}

#[derive(Clone)]
pub struct Credentials {
    pub user_auth_token: String,
    pub user_id: i64,
}

impl From<OAuthResult> for Credentials {
    fn from(value: OAuthResult) -> Self {
        Self {
            user_auth_token: value.user_auth_token,
            user_id: value.user_id,
        }
    }
}

struct DatabaseConfiguration {
    max_audio_quality: Option<i64>,
    cache_directory: Option<String>,
    cache_ttl_hours: Option<i64>,
    volume: Option<f64>,
    use_file_based_streaming: Option<bool>,
    enable_disconnect: bool,
    disconnect_server_url: Option<String>,
    disconnect_password: Option<String>,
    device_name: Option<String>,
    auto_play: Option<bool>,
}

#[derive(Default, Debug)]
pub struct Configuration {
    pub max_audio_quality: AudioQuality,
    pub use_file_based_streaming: bool,
    pub cache_directory: PathBuf,
    pub cache_ttl_hours: u32,
    pub volume: f32,
    pub enable_disconnect: bool,
    pub disconnect_server_url: Option<String>,
    pub disconnect_password: Option<String>,
    pub device_name: Option<String>,
    pub auto_play: bool,
}

#[derive(Debug, sqlx::FromRow, serde::Deserialize)]
struct TracklistDb {
    tracklist: Json<Tracklist>,
}

async fn create_credentials_row(pool: &Pool<Sqlite>) -> AppResult<()> {
    let rowid = 1;

    sqlx::query!(
        "insert or ignore into credentials (rowid) values (?1)",
        rowid
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn create_configuration(pool: &Pool<Sqlite>) -> AppResult<()> {
    let rowid = 1;
    sqlx::query!(
        "insert or ignore into configuration (rowid) values (?1)",
        rowid
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Duration, OffsetDateTime};

    #[sqlx::test]
    async fn clean_up_cache_entries(pool: sqlx::Pool<sqlx::Sqlite>) {
        let db = Database::init(pool).await.unwrap();

        let old_path_str = "path/old";
        let old_path = Path::new(old_path_str);
        let new_path_str = "path/new";
        let new_path = Path::new(new_path_str);
        db.set_cache_entry(old_path).await;
        db.set_cache_entry(new_path).await;

        let old_time = OffsetDateTime::now_utc() - Duration::days(10);
        let old_time = old_time
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();

        sqlx::query("update cache_entries set last_opened = ? where path = ?")
            .bind(&old_time)
            .bind(old_path_str)
            .execute(&db.pool)
            .await
            .unwrap();

        let deleted = db.clean_up_cache_entries(Duration::days(5)).await.unwrap();

        let remaining: Vec<_> = sqlx::query_scalar::<_, String>("select path from cache_entries")
            .fetch_all(&db.pool)
            .await
            .unwrap();

        assert_eq!(remaining, vec![new_path_str]);
        assert_eq!(deleted, vec![old_path]);
    }
}
