use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use controls_module::models::Track;
use qobuz_client::stream::flac_source_stream::SeekableStreamReader;

use crate::{AppResult, client::Client, database::Database};

pub enum DownloadResult {
    Cached(PathBuf),
    Streaming(SeekableStreamReader),
}

pub struct Downloader {
    audio_cache_directory: PathBuf,
    database: Arc<Database>,
    client: Arc<Client>,
}

impl Downloader {
    pub fn new(
        audio_cache_directory: PathBuf,
        database: Arc<Database>,
        client: Arc<Client>,
    ) -> Self {
        Self {
            audio_cache_directory,
            database,
            client,
        }
    }

    pub async fn ensure_track_is_downloaded(&mut self, track: &Track) -> AppResult<DownloadResult> {
        if self.client.file_based_streaming().await {
            return self.ensure_track_is_downloaded_file_based(track).await;
        }

        let track_info = self.client.get_streaming_info(track.id).await?;

        let cache_path = cache_path(
            track,
            &track_info.mime_type,
            track_info.sampling_rate,
            &self.audio_cache_directory,
        );
        self.database.set_cache_entry(cache_path.as_path()).await;

        if cache_path.exists() {
            tracing::info!("Playing from cache: {}", cache_path.display());
            return Ok(DownloadResult::Cached(cache_path));
        }

        let stream = self.client.stream_track(cache_path, track_info).await?;

        Ok(DownloadResult::Streaming(stream))
    }

    /// File based path: cheap cpu as no crypto ops
    async fn ensure_track_is_downloaded_file_based(
        &mut self,
        track: &Track,
    ) -> AppResult<DownloadResult> {
        let track_url = self.client.get_file_based_streaming_info(track.id).await?;

        let cache_path = cache_path(
            track,
            &track_url.mime_type,
            Some(track_url.sampling_rate as u32),
            &self.audio_cache_directory,
        );
        self.database.set_cache_entry(cache_path.as_path()).await;

        if cache_path.exists() {
            tracing::info!("Playing from cache: {}", cache_path.display());
            return Ok(DownloadResult::Cached(cache_path));
        }

        tracing::info!("Streaming: {}", track.title);
        let stream = self
            .client
            .stream_track_file_based(&track_url.url, &cache_path)
            .await?;
        Ok(DownloadResult::Streaming(stream))
    }

    pub fn set_audio_cache_dir(&mut self, new_directory: PathBuf) {
        self.audio_cache_directory = new_directory;
    }
}

fn cache_path(
    track: &Track,
    mime: &str,
    sample_rate: Option<u32>,
    audio_cache_dir: &Path,
) -> PathBuf {
    let artist_name = track.artist_name.as_deref().unwrap_or("unknown");
    let artist_id = track
        .artist_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let album_title = track.album_title.as_deref().unwrap_or("unknown");
    let album_id = track.album_id.as_deref().unwrap_or("unknown");
    let track_title = &track.title;

    let artist_dir = format!(
        "{} ({})",
        sanitize_name(artist_name),
        sanitize_name(&artist_id),
    );
    let album_dir = format!(
        "{} ({})",
        sanitize_name(album_title),
        sanitize_name(album_id),
    );
    let extension = guess_extension(mime);

    let sample_rate_suffix = sample_rate.map(|sr| format!("_{sr}")).unwrap_or_default();

    let track_file = format!(
        "{}_{}{}.{}",
        track.number,
        sanitize_name(track_title),
        sample_rate_suffix,
        extension
    );

    audio_cache_dir
        .join(artist_dir)
        .join(album_dir)
        .join(track_file)
}

fn sanitize_name(input: &str) -> String {
    let mut s: String = input
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            _ => c,
        })
        .collect();

    s = s.trim_matches([' ', '.']).to_string();

    let mut out = String::with_capacity(s.len());
    let mut prev_underscore = false;
    for ch in s.chars() {
        let ch2 = if ch == ' ' { '_' } else { ch };
        if ch2 == '_' {
            if prev_underscore {
                continue;
            }
            prev_underscore = true;
        } else {
            prev_underscore = false;
        }
        out.push(ch2);
    }

    if out.is_empty() {
        return "unknown".to_string();
    }

    const MAX: usize = 100;
    out.chars().take(MAX).collect()
}

fn guess_extension(mime: &str) -> String {
    match mime {
        m if m.contains("flac") => "flac".to_string(),
        m if m.contains("mpeg") => "mp3".to_string(),
        m if m.contains("mp3") => "mp3".to_string(),
        _ => "unknown".to_string(),
    }
}
