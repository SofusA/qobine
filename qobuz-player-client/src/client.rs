use crate::{
    Error, Result,
    qobuz_models::{
        TrackInfo,
        album::Album,
        album_suggestion::{AlbumSuggestionResponse, ReleaseQuery},
        artist::ArtistsResponse,
        artist_page::ArtistPage,
        discover::Discover,
        favorites::Favorites,
        genre::{GenreFeaturedPlaylists, GenreResponse},
        playlist::{Playlist, UserPlaylistsResult},
        search_results::SearchAllResults,
        track::{SuggestTrackInput, SuggestTrackRequest, Track, TrackSuggestionResponse},
    },
    stream::{
        cmaf, crypto, fetch_segment,
        flac_source_stream::{
            FlacSourceParams, FlacSourceStream, SeekableStreamReader, SegmentByteInfo,
        },
    },
};
use axum::{extract::Query, response::Html, routing::get};
use regex::Regex;
use reqwest::{
    Method, Response, StatusCode,
    header::{HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    net::TcpListener,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use stream_download::{Settings, StreamDownload, storage::temp::TempStorageProvider};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

const RNG_INIT: &str = "abb21364945c0583309667d13ca3d93a";

#[derive(Debug)]
pub struct Client {
    session: Option<StartResponse>,
    app_id: String,
    base_url: String,
    http_client: reqwest::Client,
    user_token: String,
    user_id: i64,
    max_audio_quality: AudioQuality,
    active_secret: Option<String>,
}

#[derive(
    Default,
    Clone,
    Copy,
    Debug,
    clap::ValueEnum,
    serde::Deserialize,
    serde::Serialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
pub enum AudioQuality {
    Mp3 = 5,
    CD = 6,
    HIFI96 = 7,
    #[default]
    HIFI192 = 27,
}

impl AudioQuality {
    pub fn to_label_str(&self) -> &str {
        match self {
            AudioQuality::Mp3 => "mp3",
            AudioQuality::CD => "cd",
            AudioQuality::HIFI96 => "hifi 96",
            AudioQuality::HIFI192 => "hifi 192",
        }
    }
}

impl Display for AudioQuality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AudioQuality::Mp3 => "5",
            AudioQuality::CD => "6",
            AudioQuality::HIFI96 => "7",
            AudioQuality::HIFI192 => "27",
        })
    }
}

impl From<Option<i64>> for AudioQuality {
    fn from(value: Option<i64>) -> Self {
        let default = AudioQuality::HIFI192;

        match value {
            Some(value) => match value {
                5 => AudioQuality::Mp3,
                6 => AudioQuality::CD,
                7 => AudioQuality::HIFI96,
                27 => AudioQuality::HIFI192,
                _ => default,
            },
            None => default,
        }
    }
}

pub enum ReleaseType {
    Albums,
    EPsAndSingles,
    Live,
    Compilations,
    // Other,
}

impl ReleaseType {
    fn as_str(&self) -> &'static str {
        match self {
            ReleaseType::Albums => "album",
            ReleaseType::EPsAndSingles => "epSingle",
            ReleaseType::Live => "live",
            ReleaseType::Compilations => "compilation",
            // ReleaseType::Other => "other",
        }
    }
}

enum Endpoint {
    Album,
    ArtistPage,
    SimilarArtists,
    ArtistReleases,
    UserPlaylist,
    Track,
    File,
    TrackURL,
    Playlist,
    PlaylistCreate,
    PlaylistDelete,
    PlaylistAddTracks,
    PlaylistDeleteTracks,
    PlaylistUpdatePosition,
    Search,
    SessionStart,
    Favorites,
    FavoriteAdd,
    FavoriteRemove,
    FavoritePlaylistAdd,
    FavoritePlaylistRemove,
    AlbumSuggest,
    GenreList,
    GenrePlaylists,
    DiscoverIndex,
    Suggest,
}

impl Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let endpoint = match self {
            Endpoint::Album => "album/get",
            Endpoint::ArtistPage => "artist/page",
            Endpoint::ArtistReleases => "artist/getReleasesList",
            Endpoint::SimilarArtists => "artist/getSimilarArtists",
            Endpoint::Playlist => "playlist/get",
            Endpoint::PlaylistCreate => "playlist/create",
            Endpoint::PlaylistDelete => "playlist/delete",
            Endpoint::PlaylistAddTracks => "playlist/addTracks",
            Endpoint::PlaylistDeleteTracks => "playlist/deleteTracks",
            Endpoint::PlaylistUpdatePosition => "playlist/updateTracksPosition",
            Endpoint::Search => "catalog/search",
            Endpoint::SessionStart => "session/start",
            Endpoint::Track => "track/get",
            Endpoint::File => "file/url",
            Endpoint::TrackURL => "track/getFileUrl",
            Endpoint::UserPlaylist => "playlist/getUserPlaylists",
            Endpoint::Favorites => "favorite/getUserFavorites",
            Endpoint::FavoriteAdd => "favorite/create",
            Endpoint::FavoriteRemove => "favorite/delete",
            Endpoint::FavoritePlaylistAdd => "playlist/subscribe",
            Endpoint::FavoritePlaylistRemove => "playlist/unsubscribe",
            Endpoint::AlbumSuggest => "album/suggest",
            Endpoint::GenreList => "genre/list",
            Endpoint::GenrePlaylists => "discover/playlists",
            Endpoint::DiscoverIndex => "discover/index",
            Endpoint::Suggest => "dynamic/suggest",
        };

        f.write_str(endpoint)
    }
}

pub async fn browser_oauth_login(headless: bool, app_id: &str) -> Result<OAuthResult> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| Error::Login)?;
    let port = listener.local_addr().map_err(|_| Error::Login)?.port();
    drop(listener);

    let oauth_url = build_oauth_url(app_id, port);

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

    let app = axum::Router::new().route(
        "/",
        get(move |Query(params): Query<HashMap<String, String>>| {
            let tx = tx.clone();
            async move {
                if let Some(code) = params.get("code_autorisation") {
                    let _ = tx.send(code.clone()).await;
                    Html("<html><body><h2>Login successful!</h2><p>You can close this tab and return to the player.</p></body></html>".to_string())
                } else {
                    Html("<html><body><h2>Login failed</h2><p>No authorization code received.</p></body></html>".to_string())
                }
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .map_err(|_| Error::Login)?;

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    println!("Login to Qobuz in browser...");
    if headless {
        let manual_oauth_url = format!(
            "https://www.qobuz.com/signin/oauth?ext_app_id={app_id}&redirect_url=http%3A%2F%2Flocalhost"
        );

        println!("Headless? Open this URL on another device instead:");
        println!();
        println!("  {manual_oauth_url}");
        println!();
        println!(
            "After login, copy the code_autorisation value from the URL bar and paste it here."
        );
        println!("Or if on the same network, the redirect will be captured automatically.");
        println!();
    }
    let _ = open::that(&oauth_url);

    let code: String = if headless {
        let mut stdin_task = Some(tokio::spawn(read_code_from_stdin()));

        tokio::select! {
            result = async {
                tokio::time::timeout(Duration::from_secs(300), rx.recv())
                    .await
                    .ok()
                    .flatten()
            } => {
                if let Some(task) = stdin_task.take() {
                    task.abort();
                }
                result.ok_or(Error::Login)?
            }

            result = stdin_task.as_mut().unwrap() => {
                // stdin path won → task already completed
                result.map_err(|_| Error::Login)??
            }
        }
    } else {
        tokio::time::timeout(Duration::from_secs(300), rx.recv())
            .await
            .ok()
            .flatten()
            .ok_or(Error::Login)?
    };

    server.abort();

    server.abort();

    tracing::debug!("Received authorization code: {}", code);

    let result = exchange_oauth_code(&code, app_id).await.map_err(|e| {
        tracing::error!("OAuth code exchange failed: {:?}", e);
        Error::Login
    })?;

    Ok(result)
}

async fn read_code_from_stdin() -> Result<String, Error> {
    print!("Paste code: ");
    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
    let mut out = tokio::io::stdout();
    out.flush().await.map_err(|_| Error::Login)?;
    let mut input = String::new();

    let _n = reader
        .read_line(&mut input)
        .await
        .map_err(|_| Error::Login)?;

    let input = input.trim();
    // Accept either raw code or full URL containing code_autorisation=
    if let Some(pos) = input.find("code_autorisation=") {
        let code = &input[pos + "code_autorisation=".len()..];
        let code = code.split(['&', ' ', '#']).next().unwrap_or(code);
        Ok(code.to_string())
    } else {
        Ok(input.to_string())
    }
}

impl Client {
    pub async fn new(
        user_auth_token: &str,
        user_id: i64,
        max_audio_quality: AudioQuality,
        file_based_streaming: bool,
    ) -> Result<Client> {
        let http_client = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .expect("infallible");

        let base_url = "https://www.qobuz.com/api.json/0.2/".to_string();

        let (app_id, active_secret) = if file_based_streaming {
            let SecretsForFileBasedStreaming {
                app_id,
                active_secret,
            } = get_secrets_for_file_based_streaming(
                &http_client,
                &base_url,
                user_auth_token,
                max_audio_quality,
            )
            .await?;
            tracing::debug!("Got login secrets + active secret, app_id: {}", app_id);
            (app_id, Some(active_secret))
        } else {
            let Secrets { app_id } = get_secrets(&http_client).await?;
            tracing::debug!("Got login secrets, app_id: {}", app_id);
            (app_id, None)
        };

        let client = Client {
            http_client,
            session: None,
            user_token: user_auth_token.to_string(),
            user_id,
            app_id,
            base_url,
            max_audio_quality,
            active_secret,
        };

        Ok(client)
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    pub fn user_id(&self) -> i64 {
        self.user_id
    }

    pub async fn genres(&self) -> Result<GenreResponse> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::GenreList);
        self.get(&endpoint, None).await
    }

    pub async fn genre_playlists(
        &self,
        genre_id: Option<u32>,
        tag: Option<&str>,
    ) -> Result<GenreFeaturedPlaylists> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::GenrePlaylists);
        let genre_id = genre_id.map(|x| x.to_string()).unwrap_or_default();
        let tag = tag.map(|x| x.to_string()).unwrap_or_default();

        let params = vec![
            ("genre_ids", genre_id.as_str()),
            ("tags", tag.as_str()),
            ("offset", "0"),
            ("limit", "20"),
        ];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn discover_index(&self, genre_id: Option<u32>) -> Result<Discover> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::DiscoverIndex);
        let genre_id = genre_id.map(|x| x.to_string()).unwrap_or_default();

        let params = vec![("genre_ids", genre_id.as_str())];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn user_playlists(&self) -> Result<UserPlaylistsResult> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::UserPlaylist);
        let params = vec![("limit", "500"), ("extra", "tracks"), ("offset", "0")];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn playlist(&self, playlist_id: u32) -> Result<Playlist> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::Playlist);
        let id_string = playlist_id.to_string();
        let params = vec![
            ("limit", "500"),
            ("extra", "tracks"),
            ("playlist_id", id_string.as_str()),
            ("offset", "0"),
        ];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn create_playlist(
        &self,
        name: String,
        is_public: bool,
        description: String,
        is_collaborative: Option<bool>,
    ) -> Result<Playlist> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::PlaylistCreate);

        let mut form_data = HashMap::new();
        form_data.insert("name", name.as_str());

        let is_collaborative = is_collaborative.unwrap_or(false);

        let is_collaborative = if !is_public {
            false.to_string()
        } else {
            is_collaborative.to_string()
        };

        form_data.insert("is_collaborative", is_collaborative.as_str());

        let is_public = is_public.to_string();
        form_data.insert("is_public", is_public.as_str());
        form_data.insert("description", description.as_str());

        self.post(&endpoint, form_data).await
    }

    pub async fn delete_playlist(&self, playlist_id: u32) -> Result<SuccessfulResponse> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::PlaylistDelete);

        let mut form_data = HashMap::new();
        let playlist_id = playlist_id.to_string();
        form_data.insert("playlist_id", playlist_id.as_str());

        self.post(&endpoint, form_data).await
    }

    pub async fn playlist_add_track(
        &self,
        playlist_id: u32,
        playlist_track_ids: &[u32],
    ) -> Result<Playlist> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::PlaylistAddTracks);

        let track_ids = playlist_track_ids
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let playlist_id = playlist_id.to_string();

        let mut form_data = HashMap::new();
        form_data.insert("playlist_id", playlist_id.as_str());
        form_data.insert("track_ids", track_ids.as_str());
        // form_data.insert("no_duplicate", "true");

        self.post(&endpoint, form_data).await
    }

    pub async fn playlist_delete_track(
        &self,
        playlist_id: u32,
        playlist_track_ids: &[u64],
    ) -> Result<Playlist> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::PlaylistDeleteTracks);

        let track_ids = playlist_track_ids
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let playlist_id = playlist_id.to_string();

        let mut form_data = HashMap::new();
        form_data.insert("playlist_id", playlist_id.as_str());
        form_data.insert("playlist_track_ids", track_ids.as_str());

        self.post(&endpoint, form_data).await
    }

    pub async fn update_playlist_track_position(
        &self,
        index: usize,
        playlist_id: u32,
        playlist_track_id: u64,
    ) -> Result<Playlist> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::PlaylistUpdatePosition);

        let index = index.to_string();
        let playlist_id = playlist_id.to_string();
        let track_id = playlist_track_id.to_string();

        let mut form_data = HashMap::new();
        form_data.insert("playlist_id", playlist_id.as_str());
        form_data.insert("playlist_track_ids", track_id.as_str());
        form_data.insert("insert_before", index.as_str());

        self.post(&endpoint, form_data).await
    }

    async fn renew_session(&mut self) -> Result<()> {
        tracing::info!("Renewing session");

        let endpoint = format!("{}{}", &self.base_url, Endpoint::SessionStart);
        let now = format!("{}", time::OffsetDateTime::now_utc().unix_timestamp());

        let mut args = BTreeMap::<&str, String>::new();
        args.insert("profile", "qbz-1".to_string());

        let request_sig = get_request_sig("sessionstart", args, &now);

        let mut form_data = HashMap::new();
        form_data.insert("profile", "qbz-1");
        form_data.insert("request_ts", now.as_str());
        form_data.insert("request_sig", request_sig.as_str());

        let result: StartResponse = self.post(&endpoint, form_data).await?;

        tracing::info!("Session renewed: {}", result.session_id);
        if let Some(infos) = &result.infos {
            tracing::debug!("Session infos: {}", infos);
        }

        self.session = Some(result);
        Ok(())
    }

    async fn ensure_valid_session(&mut self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;

        let need_new_session = match &self.session {
            None => true,
            Some(s) => s.expires_at <= now,
        };

        if need_new_session {
            self.renew_session().await?;
        }

        Ok(())
    }

    fn session_infos(&self) -> Option<&str> {
        self.session.as_ref().and_then(|s| s.infos.as_deref())
    }

    pub async fn stream_track(
        &mut self,
        track_info: TrackInfo,
        cache_path: PathBuf,
    ) -> Result<SeekableStreamReader> {
        let session_infos = self.session_infos().map(|s| s.to_string());

        let content_key = match (&track_info.key, session_infos) {
            (Some(key_str), Some(infos)) => {
                let session_key = crypto::derive_session_key(&infos)?;
                let content_key = crypto::unwrap_content_key(&session_key, key_str)?;
                tracing::debug!("Derived content key for key_id: {:?}", track_info.key_id);
                Some(content_key)
            }
            _ => {
                tracing::warn!("No encryption key available");
                None
            }
        };

        let seg0_url = track_info.url_template.replace("$SEGMENT$", "0");
        let init_bytes = fetch_segment(&seg0_url, 0).await?;
        let init_info = cmaf::parse_init_segment(&init_bytes)?;

        tracing::info!(
            "Init segment: {} bytes, FLAC header: {} bytes",
            init_bytes.len(),
            init_info.flac_header.len(),
        );

        // Segment table may list more audio segments than the API's n_segments-1.
        let audio_segments = init_info.segment_table.len() as u8;
        if audio_segments == 0 {
            return Err(Error::StreamError {
                message: "Track has no audio segments".to_string(),
            });
        }

        let flac_header_len = init_info.flac_header.len() as u64;
        let mut segment_map = Vec::new();
        let mut cumulative_offset: u64 = 0;
        for entry in &init_info.segment_table {
            segment_map.push(SegmentByteInfo {
                byte_offset: cumulative_offset,
                byte_len: entry.byte_len as u64,
            });
            cumulative_offset += entry.byte_len as u64;
        }
        let total_byte_len = flac_header_len + cumulative_offset;

        let n_segments_to_download = audio_segments + 1; // +1 for init segment

        tracing::info!(
            "Segment map: {} audio segments, total FLAC size: {} bytes",
            audio_segments,
            total_byte_len,
        );

        let params = FlacSourceParams {
            url_template: track_info.url_template,
            n_segments: n_segments_to_download,
            content_key,
            flac_header: init_info.flac_header,
            cache_path,
            segment_map: segment_map.clone(),
        };

        let reader = StreamDownload::new::<FlacSourceStream>(
            params,
            TempStorageProvider::default(),
            Settings::default().prefetch_bytes(4096),
        )
        .await
        .map_err(|e| Error::StreamError {
            message: format!("Failed to create stream: {e}"),
        })?;

        Ok(SeekableStreamReader::new(reader, total_byte_len))
    }

    pub async fn get_streaming_info(&mut self, track_id: u32) -> Result<TrackInfo> {
        self.ensure_valid_session().await?;

        let endpoint = format!("{}{}", &self.base_url, Endpoint::File);
        let now = format!("{}", time::OffsetDateTime::now_utc().unix_timestamp());
        let quality_string = self.max_audio_quality.to_string();
        let track_id_str = track_id.to_string();

        let mut args = BTreeMap::<&str, String>::new();
        args.insert("format_id", quality_string.clone());
        args.insert("intent", "stream".to_string());
        args.insert("track_id", track_id_str.clone());

        let request_sig = get_request_sig("fileurl", args, &now);

        let params = vec![
            ("request_ts", now.as_str()),
            ("request_sig", request_sig.as_str()),
            ("track_id", track_id_str.as_str()),
            ("format_id", quality_string.as_str()),
            ("intent", "stream"),
        ];

        let session_id = self.session.as_ref().unwrap().session_id.clone();

        match make_get_call(
            &endpoint,
            Some(&params),
            &self.http_client,
            &self.app_id,
            Some(&self.user_token),
            Some(&session_id),
        )
        .await
        {
            Ok(response) => match serde_json::from_str::<TrackInfo>(response.as_str()) {
                Ok(item) => Ok(item),
                Err(error) => {
                    tracing::debug!("TrackURL deserialize error: {}", error);
                    tracing::debug!("Response was: {}", response);
                    Err(Error::DeserializeJSON {
                        message: error.to_string(),
                    })
                }
            },
            Err(error) => Err(Error::Api {
                message: error.to_string(),
            }),
        }
    }

    pub async fn get_file_based_streaming_info(
        &self,
        track_id: u32,
    ) -> Result<crate::qobuz_models::TrackUrl> {
        let secret = self.active_secret.as_deref().ok_or(Error::ActiveSecret)?;
        track_url(
            track_id,
            secret,
            &self.base_url,
            &self.http_client,
            &self.app_id,
            &self.user_token,
            self.max_audio_quality,
        )
        .await
    }

    pub async fn stream_track_file_based(
        &self,
        url: &str,
        cache_path: &std::path::Path,
    ) -> Result<SeekableStreamReader> {
        use stream_download::http::HttpStream;
        use stream_download::http::reqwest::{Client as SdClient, Url as SdUrl};
        use stream_download::source::SourceStream;

        use crate::stream::passthrough_storage::PassthroughStorageProvider;

        let url_parsed: SdUrl = url
            .parse()
            .map_err(|e: url::ParseError| Error::StreamError {
                message: format!("invalid track URL: {e}"),
            })?;

        let stream = HttpStream::new(SdClient::new(), url_parsed)
            .await
            .map_err(|e| Error::StreamError {
                message: format!("failed to open HTTP stream: {e}"),
            })?;

        let content_length = stream.content_length().unwrap_or(0);

        let partial_path = cache_path.with_extension("partial");
        let provider = PassthroughStorageProvider {
            partial_path: partial_path.clone(),
        };

        let download = StreamDownload::from_stream(
            stream,
            provider,
            Settings::default().prefetch_bytes(64 * 1024),
        )
        .await
        .map_err(|e| Error::StreamError {
            message: format!("failed to create stream-download: {e}"),
        })?;

        if content_length > 0 {
            let handle = download.handle();
            let final_path = cache_path.to_path_buf();
            tokio::spawn(async move {
                handle.wait_for_completion().await;
                finalize_cache(partial_path, final_path, content_length);
            });
        }

        Ok(SeekableStreamReader::new(download, content_length))
    }

    pub async fn favorites(&self, limit: i32) -> Result<Favorites> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::Favorites);

        let limit = limit.to_string();
        let params = vec![("limit", limit.as_str())];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn add_favorite_track(&self, id: u32) -> Result<SuccessfulResponse> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::FavoriteAdd);
        let mut form_data = HashMap::new();
        let id = id.to_string();
        form_data.insert("track_ids", id.as_str());

        self.post(&endpoint, form_data).await
    }

    pub async fn remove_favorite_track(&self, id: u32) -> Result<SuccessfulResponse> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::FavoriteRemove);
        let mut form_data = HashMap::new();
        let id = id.to_string();
        form_data.insert("track_ids", id.as_str());

        self.post(&endpoint, form_data).await
    }

    pub async fn add_favorite_album(&self, id: &str) -> Result<SuccessfulResponse> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::FavoriteAdd);
        let mut form_data = HashMap::new();
        form_data.insert("album_ids", id);

        self.post(&endpoint, form_data).await
    }

    pub async fn remove_favorite_album(&self, id: &str) -> Result<SuccessfulResponse> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::FavoriteRemove);
        let mut form_data = HashMap::new();
        form_data.insert("album_ids", id);

        self.post(&endpoint, form_data).await
    }

    pub async fn add_favorite_artist(&self, id: u32) -> Result<SuccessfulResponse> {
        let id = id.to_string();
        let endpoint = format!("{}{}", self.base_url, Endpoint::FavoriteAdd);
        let mut form_data = HashMap::new();
        form_data.insert("artist_ids", id.as_str());

        self.post(&endpoint, form_data).await
    }

    pub async fn remove_favorite_artist(&self, id: u32) -> Result<SuccessfulResponse> {
        let id = id.to_string();
        let endpoint = format!("{}{}", self.base_url, Endpoint::FavoriteRemove);
        let mut form_data = HashMap::new();
        form_data.insert("artist_ids", id.as_str());

        self.post(&endpoint, form_data).await
    }

    pub async fn add_favorite_playlist(&self, id: u32) -> Result<SuccessfulResponse> {
        let id = id.to_string();
        let endpoint = format!("{}{}", self.base_url, Endpoint::FavoritePlaylistAdd);
        let mut form_data = HashMap::new();
        form_data.insert("playlist_id", id.as_str());

        self.post(&endpoint, form_data).await
    }

    pub async fn remove_favorite_playlist(&self, id: u32) -> Result<SuccessfulResponse> {
        let id = id.to_string();
        let endpoint = format!("{}{}", self.base_url, Endpoint::FavoritePlaylistRemove);
        let mut form_data = HashMap::new();
        form_data.insert("playlist_id", id.as_str());

        self.post(&endpoint, form_data).await
    }

    pub async fn search_all(&self, query: &str, limit: i32) -> Result<SearchAllResults> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::Search);
        let limit = limit.to_string();
        let params = vec![("query", query), ("limit", &limit)];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn album(&self, album_id: &str) -> Result<Album> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::Album);
        let params = vec![
            ("album_id", album_id),
            ("extra", "track_ids"),
            ("offset", "0"),
            ("limit", "500"),
        ];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn track(&self, track_id: u32) -> Result<Track> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::Track);
        let track_id_string = track_id.to_string();
        let params = vec![("track_id", track_id_string.as_str())];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn suggested_albums(&self, album_id: &str) -> Result<AlbumSuggestionResponse> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::AlbumSuggest);
        let params = vec![("album_id", album_id)];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn suggest_track(
        &self,
        queue_track_ids: Vec<u32>,
        genre_id: Option<u32>,
        label_id: Option<u32>,
    ) -> Result<Track> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::Suggest);

        let listened_tracks_ids = queue_track_ids.clone();

        let start_index = queue_track_ids.len().saturating_sub(5);

        let last_track_ids = queue_track_ids[start_index..].to_vec();

        let mut track_to_analysed = Vec::with_capacity(last_track_ids.len());

        for track_id in last_track_ids {
            let track = self.track(track_id).await?;

            track_to_analysed.push(SuggestTrackInput {
                track_id: track.id,
                artist_id: track.performer.map(|x| x.id),
                genre_id,
                label_id,
            });
        }

        let body = SuggestTrackRequest {
            limit: 50,
            listened_tracks_ids,
            track_to_analysed,
        };

        let suggestions: TrackSuggestionResponse = self.post_json(&endpoint, &body).await?;

        suggestions
            .tracks
            .items
            .into_iter()
            .next()
            .ok_or_else(|| Error::Api {
                message: "No track suggestions returned".to_string(),
            })
    }

    pub async fn artist(&self, artist_id: u32) -> Result<ArtistPage> {
        let app_id = &self.app_id;

        let endpoint = format!("{}{}", self.base_url, Endpoint::ArtistPage);

        let artistid_string = artist_id.to_string();

        let params = vec![
            ("artist_id", artistid_string.as_str()),
            ("app_id", app_id),
            ("sort", "relevant"),
        ];

        self.get(&endpoint, Some(&params)).await
    }

    async fn get<T>(&self, endpoint: &str, params: Option<&[(&str, &str)]>) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = self
            .make_get_call(endpoint, params)
            .await
            .map_err(|error| Error::Api {
                message: error.to_string(),
            })?;

        let str = response.as_str();
        let item = match serde_json::from_str::<T>(str) {
            Ok(item) => item,
            Err(err) => {
                tracing::error!("Failed to deserialize: {str}. Error: {err}");
                return Err(Error::DeserializeJSON {
                    message: err.to_string(),
                });
            }
        };

        Ok(item)
    }

    async fn post<T>(&self, endpoint: &str, params: HashMap<&str, &str>) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = self
            .make_post_call(endpoint, params)
            .await
            .map_err(|error| Error::Api {
                message: error.to_string(),
            })?;

        let item = serde_json::from_str::<T>(response.as_str()).map_err(|error| {
            Error::DeserializeJSON {
                message: error.to_string(),
            }
        })?;

        Ok(item)
    }

    async fn post_json<TResponse, TBody>(&self, endpoint: &str, body: &TBody) -> Result<TResponse>
    where
        TResponse: serde::de::DeserializeOwned,
        TBody: serde::Serialize + ?Sized,
    {
        let response = self
            .make_post_json_call(endpoint, body)
            .await
            .map_err(|error| Error::Api {
                message: error.to_string(),
            })?;

        let item = serde_json::from_str::<TResponse>(response.as_str()).map_err(|error| {
            Error::DeserializeJSON {
                message: error.to_string(),
            }
        })?;

        Ok(item)
    }

    pub async fn similar_artists(
        &self,
        artist_id: u32,
        limit: Option<i32>,
    ) -> Result<ArtistsResponse> {
        let limit = limit.unwrap_or(10).to_string();

        let endpoint = format!("{}{}", self.base_url, Endpoint::SimilarArtists);
        let artistid_string = artist_id.to_string();

        let params = vec![
            ("artist_id", artistid_string.as_str()),
            ("limit", &limit),
            ("offset", "0"),
        ];

        self.get(&endpoint, Some(&params)).await
    }

    pub async fn artist_releases(
        &self,
        artist_id: u32,
        release_type: ReleaseType,
        limit: Option<i32>,
    ) -> Result<ReleaseQuery> {
        let endpoint = format!("{}{}", self.base_url, Endpoint::ArtistReleases);
        let limit = limit.unwrap_or(100).to_string();

        let artistid_string = artist_id.to_string();

        let params = vec![
            ("artist_id", artistid_string.as_str()),
            ("limit", &limit),
            ("release_type", release_type.as_str()),
            ("sort", "release_date"),
            ("offset", "0"),
            ("track_size", "1"),
        ];

        self.get(&endpoint, Some(&params)).await
    }

    async fn make_get_call(
        &self,
        endpoint: &str,
        params: Option<&[(&str, &str)]>,
    ) -> Result<String> {
        make_get_call(
            endpoint,
            params,
            &self.http_client,
            &self.app_id,
            Some(&self.user_token),
            None,
        )
        .await
    }

    async fn make_post_call(&self, endpoint: &str, params: HashMap<&str, &str>) -> Result<String> {
        let headers = client_headers(&self.app_id, Some(&self.user_token));

        tracing::debug!("calling {} endpoint, with params {params:?}", endpoint);
        let response = self
            .http_client
            .request(Method::POST, endpoint)
            .headers(headers)
            .form(&params)
            .send()
            .await?;

        handle_response(response).await
    }

    async fn make_post_json_call<TBody>(&self, endpoint: &str, body: &TBody) -> Result<String>
    where
        TBody: serde::Serialize + ?Sized,
    {
        let headers = client_headers(&self.app_id, Some(&self.user_token));

        tracing::debug!("calling {} endpoint, with JSON body", endpoint);

        let response = self
            .http_client
            .request(Method::POST, endpoint)
            .headers(headers)
            .json(body)
            .send()
            .await?;

        handle_response(response).await
    }
}

fn get_request_sig(method: &str, args: BTreeMap<&str, String>, now_string: &str) -> String {
    let mut n = String::new();
    for (k, v) in args.iter() {
        n.push_str(k);
        n.push_str(v);
    }

    let req_id = format!("{method}{n}{now_string}{RNG_INIT}");
    format!("{:x}", md5::compute(req_id.as_bytes()))
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
struct StartResponse {
    session_id: String,
    expires_at: u32,
    #[serde(default)]
    infos: Option<String>,
}

async fn handle_response(response: Response) -> Result<String> {
    if response.status() == StatusCode::OK {
        let res = response.text().await.unwrap_or_default();
        Ok(res)
    } else {
        Err(Error::Api {
            message: response.text().await.unwrap_or_default(),
        })
    }
}

async fn make_get_call(
    endpoint: &str,
    params: Option<&[(&str, &str)]>,
    client: &reqwest::Client,
    app_id: &str,
    user_token: Option<&str>,
    session: Option<&str>,
) -> Result<String> {
    let mut headers = client_headers(app_id, user_token);

    if let Some(session_id) = session {
        headers.insert(
            "X-Session-Id",
            HeaderValue::from_str(session_id).expect("infallible"),
        );
    }

    tracing::debug!("calling {} endpoint, with params {params:?}", endpoint);
    let request = client.request(Method::GET, endpoint).headers(headers);

    if let Some(p) = params {
        let response = request.query(&p).send().await?;
        handle_response(response).await
    } else {
        let response = request.send().await?;
        handle_response(response).await
    }
}

fn client_headers(app_id: &str, user_token: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();

    tracing::debug!("adding app_id to request headers: {}", app_id);
    headers.insert(
        "X-App-Id",
        HeaderValue::from_str(app_id).expect("infallible"),
    );

    if let Some(token) = user_token {
        tracing::debug!("adding token to request headers: {}", token);
        headers.insert(
            "X-User-Auth-Token",
            HeaderValue::from_str(token).expect("infallible"),
        );
    }

    headers.insert(
        "Access-Control-Request-Headers",
        HeaderValue::from_str("x-app-id,x-user-auth-token").expect("infallible"),
    );

    headers.insert(
        "Accept-Language",
        HeaderValue::from_str("en,en-US;q=0.8,ko;q=0.6,zh;q=0.4,zh-CN;q=0.2").expect("infallible"),
    );

    headers
}

const OAUTH_PRIVATE_KEY: &str = "6lz8C03UDIC7";

#[derive(Debug, Clone)]
pub struct OAuthResult {
    pub user_auth_token: String,
    pub user_id: i64,
}

/// Fetch the app_id from the Qobuz web player bundle.
pub async fn get_app_id() -> Result<String> {
    let http_client = reqwest::Client::new();
    let Secrets { app_id } = get_secrets(&http_client).await?;
    Ok(app_id)
}

/// Exchange an OAuth authorization code for a user_auth_token.
pub async fn exchange_oauth_code(code: &str, app_id: &str) -> Result<OAuthResult> {
    let http_client = reqwest::Client::new();
    let base_url = "https://www.qobuz.com/api.json/0.2/";
    let endpoint = format!("{base_url}oauth/callback");
    let params = vec![("code", code), ("private_key", OAUTH_PRIVATE_KEY)];

    let response = make_get_call(&endpoint, Some(&params), &http_client, app_id, None, None).await;

    let response = match response {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("oauth/callback API error: {e}");
            return Err(e);
        }
    };

    tracing::debug!(
        "oauth/callback response: {}",
        &response[..response.len().min(200)]
    );

    let json: Value = serde_json::from_str(response.as_str())
        .or(Err(Error::DeserializeJSON { message: response }))?;

    let user_auth_token = json["token"]
        .as_str()
        .or_else(|| json["user_auth_token"].as_str())
        .ok_or(Error::Login)?
        .to_string();

    let user_id = json["user_id"]
        .as_i64()
        .or_else(|| json["user_id"].as_str().and_then(|s| s.parse().ok()))
        .ok_or(Error::Login)?;

    Ok(OAuthResult {
        user_auth_token,
        user_id,
    })
}

/// Build the OAuth URL that the user should open in their browser.
fn build_oauth_url(app_id: &str, redirect_port: u16) -> String {
    let redirect = format!("http://localhost:{redirect_port}");
    format!("https://www.qobuz.com/signin/oauth?ext_app_id={app_id}&redirect_url={redirect}",)
}

struct Secrets {
    app_id: String,
}

async fn get_secrets(client: &reqwest::Client) -> Result<Secrets> {
    let play_url = "https://play.qobuz.com";

    let login_html = client
        .get(format!("{play_url}/login"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await
        .map_err(|_| Error::Login)?;

    let bundle_regex = Regex::new(
        r#"<script src="(/resources/\d+\.\d+\.\d+-[a-z0-9]\d{3}/bundle\.js)"></script>"#,
    )
    .map_err(|_| Error::Login)?;

    let app_id_regex = Regex::new(
        r#"production:\{api:\{appId:"(?P<app_id>\d{9})",appSecret:"(?P<app_secret>\w{32})""#,
    )
    .map_err(|_| Error::AppID)?;

    let bundle_path = bundle_regex
        .captures(&login_html)
        .and_then(|c| c.get(1))
        .ok_or(Error::AppID)?
        .as_str();

    let bundle_html = client
        .get(format!("{play_url}{bundle_path}"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await
        .map_err(|_| Error::AppID)?;

    let app_captures = app_id_regex.captures(&bundle_html).ok_or(Error::AppID)?;
    let app_id = app_captures
        .name("app_id")
        .ok_or(Error::AppID)?
        .as_str()
        .to_owned();

    Ok(Secrets { app_id })
}

struct SecretsForFileBasedStreaming {
    app_id: String,
    active_secret: String,
}

/// extract the app_id, per-timezone secrets, and probe for an active one
/// tried to mirror 8cd4d7a
async fn get_secrets_for_file_based_streaming(
    client: &reqwest::Client,
    base_url: &str,
    user_token: &str,
    max_audio_quality: AudioQuality,
) -> Result<SecretsForFileBasedStreaming> {
    use base64::{Engine, engine::general_purpose};

    let play_url = "https://play.qobuz.com";

    let login_html = client
        .get(format!("{play_url}/login"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await
        .map_err(|_| Error::Login)?;

    let bundle_regex = Regex::new(
        r#"<script src="(/resources/\d+\.\d+\.\d+-[a-z0-9]\d{3}/bundle\.js)"></script>"#,
    )
    .map_err(|_| Error::Login)?;

    let app_id_regex = Regex::new(
        r#"production:\{api:\{appId:"(?P<app_id>\d{9})",appSecret:"(?P<app_secret>\w{32})""#,
    )
    .map_err(|_| Error::AppID)?;

    let seed_regex = Regex::new(
        r#"[a-z]\.initialSeed\("(?P<seed>[\w=]+)",window\.utimezone\.(?P<timezone>[a-z]+)\)"#,
    )
    .map_err(|_| Error::Login)?;

    let bundle_path = bundle_regex
        .captures(&login_html)
        .and_then(|c| c.get(1))
        .ok_or(Error::AppID)?
        .as_str();

    let bundle_html = client
        .get(format!("{play_url}{bundle_path}"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await
        .map_err(|_| Error::AppID)?;

    let app_captures = app_id_regex.captures(&bundle_html).ok_or(Error::AppID)?;
    let app_id = app_captures
        .name("app_id")
        .ok_or(Error::AppID)?
        .as_str()
        .to_owned();

    let mut secrets = HashMap::new();

    for seed_cap in seed_regex.captures_iter(&bundle_html) {
        let seed = seed_cap.name("seed").ok_or(Error::Login)?.as_str();
        let mut timezone = seed_cap
            .name("timezone")
            .ok_or(Error::Login)?
            .as_str()
            .to_owned();

        capitalize(timezone.as_mut_str());

        let info_re_str = format!(
            r#"name:"\w+/(?P<timezone>{}([a-z]?))",info:"(?P<info>[\w=]+)",extras:"(?P<extras>[\w=]+)""#,
            timezone
        );
        let info_re = Regex::new(&info_re_str).map_err(|_| Error::Login)?;

        for c in info_re.captures_iter(&bundle_html) {
            let tz_full = c.name("timezone").ok_or(Error::Login)?.as_str().to_owned();
            let info = c.name("info").ok_or(Error::Login)?.as_str();
            let extras = c.name("extras").ok_or(Error::Login)?.as_str();

            let chars = format!("{seed}{info}{extras}");

            if chars.len() <= 44 {
                continue;
            }

            let encoded_secret = &chars[..chars.len() - 44];

            let decoded = general_purpose::URL_SAFE
                .decode(encoded_secret)
                .map_err(|_| Error::Login)?;
            let secret = std::str::from_utf8(&decoded)
                .map_err(|_| Error::Login)?
                .to_owned();

            secrets.insert(tz_full, secret);
        }
    }

    let active_secret = find_active_secret(
        secrets,
        base_url,
        client,
        &app_id,
        user_token,
        max_audio_quality,
    )
    .await?;

    Ok(SecretsForFileBasedStreaming {
        app_id,
        active_secret,
    })
}

/// Probe each per-timezone secret with a known-good track id; the first one
/// that returns a valid response is the active secret for our request region
async fn find_active_secret(
    secrets: HashMap<String, String>,
    base_url: &str,
    client: &reqwest::Client,
    app_id: &str,
    user_token: &str,
    max_audio_quality: AudioQuality,
) -> Result<String> {
    tracing::debug!("probing {} timezone secrets", secrets.len());

    for (timezone, secret) in secrets.into_iter() {
        let response = track_url(
            64868955,
            &secret,
            base_url,
            client,
            app_id,
            user_token,
            max_audio_quality,
        )
        .await;

        if response.is_ok() {
            tracing::debug!("found active secret for timezone: {}", timezone);
            return Ok(secret);
        }
    }

    Err(Error::ActiveSecret)
}

/// MD5 signature endpoint + sorted(param k+v concatenated) + ts + secret
async fn track_url(
    track_id: u32,
    secret: &str,
    base_url: &str,
    client: &reqwest::Client,
    app_id: &str,
    user_token: &str,
    max_audio_quality: AudioQuality,
) -> Result<crate::qobuz_models::TrackUrl> {
    let endpoint = format!("{}{}", base_url, Endpoint::TrackURL);
    let now = format!("{}", time::OffsetDateTime::now_utc().unix_timestamp());
    let quality = max_audio_quality.to_string();
    let track_id_str = track_id.to_string();

    let sig =
        format!("trackgetFileUrlformat_id{quality}intentstreamtrack_id{track_id_str}{now}{secret}");
    let request_sig = format!("{:x}", md5::compute(sig.as_str()));

    let params = vec![
        ("request_ts", now.as_str()),
        ("request_sig", request_sig.as_str()),
        ("track_id", track_id_str.as_str()),
        ("format_id", quality.as_str()),
        ("intent", "stream"),
    ];

    let response = make_get_call(
        &endpoint,
        Some(&params),
        client,
        app_id,
        Some(user_token),
        None,
    )
    .await?;
    serde_json::from_str(&response).map_err(|_| Error::DeserializeJSON { message: response })
}

fn capitalize(s: &mut str) {
    if let Some(r) = s.get_mut(0..1) {
        r.make_ascii_uppercase();
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SuccessfulResponse {
    status: String,
}

fn finalize_cache(partial: PathBuf, final_path: PathBuf, expected: u64) {
    match std::fs::metadata(&partial) {
        Ok(meta) if meta.len() == expected => {
            if let Err(e) = std::fs::rename(&partial, &final_path) {
                tracing::warn!("Failed to finalize cache: {e}");
                let _ = std::fs::remove_file(&partial);
            } else {
                tracing::info!("Cached: {} ({} bytes)", final_path.display(), expected);
            }
        }
        Ok(meta) => {
            tracing::debug!(
                "Stream incomplete ({} of {} bytes), discarding partial",
                meta.len(),
                expected
            );
            let _ = std::fs::remove_file(&partial);
        }
        Err(_) => {}
    }
}
