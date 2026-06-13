use std::{path::PathBuf, sync::Mutex};

use crate::database::Credentials;
use controls_module::models::{
    Album, AlbumSimple, Artist, ArtistPage, DiscoverPage, Favorites, Genre, Playlist,
    PlaylistSimple, SearchResults, Track,
    mapper::{
        extract_year, hifi_available, parse_album, parse_album_simple, parse_artist,
        parse_artist_page, parse_discover, parse_genre, parse_playlist, parse_playlist_simple,
        parse_search_results, parse_track,
    },
};
use futures::future::join_all;
use moka::future::Cache;
use qobuz_client::{
    client::{AudioQuality, OAuthResult, ReleaseType, browser_oauth_login},
    qobuz_models::{TrackInfo, TrackUrl},
    stream::flac_source_stream::SeekableStreamReader,
};
use time::Duration;
use tokio::{
    sync::{OnceCell, RwLock},
    try_join,
};

use crate::{AppResult, error::Error, simple_cache::SimpleCache};

pub use qobuz_client::client::exchange_oauth_code;
pub use qobuz_client::client::get_app_id;

type QobuzClient = qobuz_client::client::Client;
type Result<T, E = Error> = std::result::Result<T, E>;

pub struct Client {
    qobuz_client: OnceCell<RwLock<QobuzClient>>,
    credentials: Mutex<Option<Credentials>>,
    max_audio_quality: RwLock<AudioQuality>,
    file_based_streaming: RwLock<bool>,
    favorites_cache: SimpleCache<Favorites>,
    genres_cache: SimpleCache<Vec<Genre>>,
    genre_playlists_cache: Cache<GenrePlaylistSlug, Vec<PlaylistSimple>>,
    album_cache: Cache<String, Album>,
    artist_cache: Cache<u32, ArtistPage>,
    playlist_cache: Cache<u32, Playlist>,
    suggested_albums_cache: Cache<String, Vec<AlbumSimple>>,
    search_cache: Cache<String, SearchResults>,
    discover_cache: Cache<Option<u32>, DiscoverPage>,
}

impl Client {
    pub fn credentials_is_set(&self) -> AppResult<bool> {
        Ok(self.credentials.lock()?.is_some())
    }

    pub fn set_credentials(&self, credentials: Credentials) -> AppResult<()> {
        let mut lock = self.credentials.lock()?;
        *lock = Some(credentials);
        Ok(())
    }

    pub async fn app_id(&self) -> AppResult<String> {
        let client = self.get_client().await?;
        Ok(client.app_id().to_string())
    }

    pub async fn new_with_oauth_login(
        max_audio_quality: AudioQuality,
        file_based_streaming: bool,
        headless: bool,
    ) -> Result<(Self, OAuthResult)> {
        let app_id = get_app_id().await?;
        let oauth_result = browser_oauth_login(headless, &app_id).await?;
        let client = Self::new(
            Some(Credentials {
                user_auth_token: oauth_result.user_auth_token.clone(),
                user_id: oauth_result.user_id,
            }),
            max_audio_quality,
            file_based_streaming,
        );

        Ok((client, oauth_result))
    }

    pub async fn file_based_streaming(&self) -> bool {
        *self.file_based_streaming.read().await
    }

    pub fn new(
        credentials: Option<Credentials>,
        max_audio_quality: AudioQuality,
        file_based_streaming: bool,
    ) -> Self {
        let album_cache = moka::future::CacheBuilder::new(1000)
            .time_to_live(std::time::Duration::from_secs(60 * 60 * 24 * 7))
            .build();

        let artist_cache = moka::future::CacheBuilder::new(1000)
            .time_to_live(std::time::Duration::from_secs(60 * 60 * 24))
            .build();

        let playlist_cache = moka::future::CacheBuilder::new(1000)
            .time_to_live(std::time::Duration::from_secs(60 * 60 * 24))
            .build();

        let suggested_albums_cache = moka::future::CacheBuilder::new(1000)
            .time_to_live(std::time::Duration::from_secs(60 * 60 * 24 * 7))
            .build();

        let search_cache = moka::future::CacheBuilder::new(1000)
            .time_to_live(std::time::Duration::from_secs(60 * 60 * 24))
            .build();

        let genre_playlists_cache = moka::future::CacheBuilder::new(1000)
            .time_to_live(std::time::Duration::from_secs(60 * 60 * 24))
            .build();

        let discover_cache = moka::future::CacheBuilder::new(1000)
            .time_to_live(std::time::Duration::from_secs(60 * 60 * 24))
            .build();

        let credentials = Mutex::new(credentials);
        let max_audio_quality = RwLock::new(max_audio_quality);
        let file_based_streaming = RwLock::new(file_based_streaming);

        Self {
            qobuz_client: Default::default(),
            credentials,
            max_audio_quality,
            file_based_streaming,
            favorites_cache: SimpleCache::new(Duration::days(1)),
            genres_cache: SimpleCache::new(Duration::days(7)),
            genre_playlists_cache,
            album_cache,
            artist_cache,
            playlist_cache,
            suggested_albums_cache,
            search_cache,
            discover_cache,
        }
    }

    async fn init_client(&self) -> Result<QobuzClient> {
        let credentials = self.credentials.lock()?.clone();

        let Some(credentials) = credentials else {
            return Err(Error::Login {
                message: "Login credentials not set before logging in".to_string(),
            });
        };

        let max_audio_quality = self.max_audio_quality.read().await;
        let file_based_streaming = self.file_based_streaming.read().await;

        let client = QobuzClient::new(
            &credentials.user_auth_token,
            credentials.user_id,
            *max_audio_quality,
            *file_based_streaming,
        )
        .await?;

        Ok(client)
    }

    async fn get_client(&self) -> Result<tokio::sync::RwLockReadGuard<'_, QobuzClient>> {
        let cell = self
            .qobuz_client
            .get_or_try_init(|| async {
                let client = self.init_client().await?;
                Ok::<_, Error>(RwLock::new(client))
            })
            .await?;

        Ok(cell.read().await)
    }

    async fn get_client_mut(&self) -> Result<tokio::sync::RwLockWriteGuard<'_, QobuzClient>> {
        let cell = self
            .qobuz_client
            .get_or_try_init(|| async {
                let client = self.init_client().await?;
                Ok::<_, Error>(RwLock::new(client))
            })
            .await?;

        Ok(cell.write().await)
    }

    pub async fn set_max_audio_quality(&self, new_quality: AudioQuality) {
        let mut max_audio_quality = self.max_audio_quality.write().await;
        *max_audio_quality = new_quality;
    }

    pub async fn use_file_based_streaming(&self, use_file_based_streaming: bool) {
        let mut file_based_streaming = self.file_based_streaming.write().await;
        *file_based_streaming = use_file_based_streaming;
    }

    pub async fn get_streaming_info(&self, track_id: u32) -> Result<TrackInfo> {
        let mut client = self.get_client_mut().await?;
        let info = client.get_streaming_info(track_id).await?;
        Ok(info)
    }

    pub async fn get_file_based_streaming_info(&self, track_id: u32) -> Result<TrackUrl> {
        let client = self.get_client().await?;
        let info = client.get_file_based_streaming_info(track_id).await?;
        Ok(info)
    }

    pub async fn stream_track_file_based(
        &self,
        url: &str,
        cache_path: &std::path::Path,
    ) -> Result<SeekableStreamReader> {
        let client = self.get_client().await?;
        let stream = client.stream_track_file_based(url, cache_path).await?;
        Ok(stream)
    }

    pub async fn stream_track(
        &self,
        cache_path: PathBuf,
        track_info: TrackInfo,
    ) -> Result<SeekableStreamReader> {
        let mut client = self.get_client_mut().await?;
        let stream = client.stream_track(track_info, cache_path).await?;
        Ok(stream)
    }

    pub async fn album(&self, id: &str) -> Result<Album> {
        if let Some(cache) = self.album_cache.get(id).await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let album = client.album(id).await?;
        let album = parse_album(album, &*self.max_audio_quality.read().await);

        self.album_cache.insert(id.to_string(), album.clone()).await;

        Ok(album)
    }

    pub async fn search(&self, query: String) -> Result<SearchResults> {
        if let Some(cache) = self.search_cache.get(&query).await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let results = client.search_all(&query, 20).await?;
        let user_id = self.get_client().await?.user_id();

        let out = parse_search_results(results, user_id, &*self.max_audio_quality.read().await);

        self.search_cache.insert(query, out.clone()).await;
        Ok(out)
    }

    pub async fn artist_page(&self, id: u32) -> Result<ArtistPage> {
        if let Some(cache) = self.artist_cache.get(&id).await {
            return Ok(cache);
        }

        let client = self.get_client().await?;

        let (artist, albums, singles, live, compilations, similar_artists) = try_join!(
            client.artist(id),
            client.artist_releases(id, ReleaseType::Albums, None),
            client.artist_releases(id, ReleaseType::EPsAndSingles, None),
            client.artist_releases(id, ReleaseType::Live, None),
            client.artist_releases(id, ReleaseType::Compilations, None),
            client.similar_artists(id, None),
        )?;

        let audio_quality = self.max_audio_quality.read().await;

        let artist = parse_artist_page(
            artist,
            albums
                .items
                .into_iter()
                .map(|x| parse_album_simple(x, &audio_quality))
                .collect(),
            singles
                .items
                .into_iter()
                .map(|x| {
                    let max_audio_quality: &AudioQuality = &audio_quality;
                    let artist = x.artists.and_then(|vec| vec.into_iter().next());
                    let (artist_id, artist_name) = artist.map_or((0, "Unknown".into()), |artist| {
                        (artist.id as u32, artist.name.unwrap_or("Unknown".into()))
                    });

                    AlbumSimple {
                        id: x.id,
                        title: x.title,
                        artist: Artist {
                            id: artist_id,
                            name: artist_name,
                            ..Default::default()
                        },
                        hires_available: hifi_available(
                            x.rights.hires_streamable,
                            max_audio_quality,
                        ),
                        explicit: x.parental_warning,
                        available: x.rights.streamable,
                        image: x.image.large,
                        duration_seconds: x.duration,
                        release_year: extract_year(&x.dates.original),
                    }
                })
                .collect(),
            live.items
                .into_iter()
                .map(|x| {
                    let max_audio_quality: &AudioQuality = &audio_quality;
                    let artist = x.artists.and_then(|vec| vec.into_iter().next());
                    let (artist_id, artist_name) = artist.map_or((0, "Unknown".into()), |artist| {
                        (artist.id as u32, artist.name.unwrap_or("Unknown".into()))
                    });

                    AlbumSimple {
                        id: x.id,
                        title: x.title,
                        artist: Artist {
                            id: artist_id,
                            name: artist_name,
                            ..Default::default()
                        },
                        hires_available: hifi_available(
                            x.rights.hires_streamable,
                            max_audio_quality,
                        ),
                        explicit: x.parental_warning,
                        available: x.rights.streamable,
                        image: x.image.large,
                        duration_seconds: x.duration,
                        release_year: extract_year(&x.dates.original),
                    }
                })
                .collect(),
            compilations
                .items
                .into_iter()
                .map(|x| parse_album_simple(x, &audio_quality))
                .collect(),
            similar_artists
                .artists
                .items
                .into_iter()
                .map(parse_artist)
                .collect(),
        );

        self.artist_cache.insert(id, artist.clone()).await;
        Ok(artist)
    }

    pub async fn tracks(&self, ids: Vec<u32>) -> Result<Vec<Track>> {
        let futures = ids.into_iter().map(|id| self.track(id));
        let results = join_all(futures).await;
        results.into_iter().collect()
    }

    pub async fn track(&self, id: u32) -> Result<Track> {
        let client = self.get_client().await?;
        let track = client.track(id).await?;
        let track = parse_track(track, &*self.max_audio_quality.read().await);
        Ok(track)
    }

    pub async fn suggested_albums(&self, id: &str) -> Result<Vec<AlbumSimple>> {
        if let Some(cache) = self.suggested_albums_cache.get(id).await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let audio_quality = self.max_audio_quality.read().await;
        let suggested_albums = client.suggested_albums(id).await?;
        let suggested_albums: Vec<_> = suggested_albums
            .albums
            .items
            .into_iter()
            .map(|x| parse_album_simple(x, &audio_quality))
            .collect();

        self.suggested_albums_cache
            .insert(id.to_string(), suggested_albums.clone())
            .await;

        Ok(suggested_albums)
    }

    pub async fn suggest_track(&self, queue_track_ids: Vec<u32>) -> Result<Track> {
        let client = self.get_client().await?;
        let suggestion = client.suggest_track(queue_track_ids, None, None).await?;
        let audio_quality = self.max_audio_quality.read().await;

        Ok(parse_track(suggestion, &audio_quality))
    }

    pub async fn playlist(&self, id: u32) -> Result<Playlist> {
        if let Some(cache) = self.playlist_cache.get(&id).await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let playlist = client.playlist(id).await?;
        let playlist = parse_playlist(
            playlist,
            client.user_id(),
            &*self.max_audio_quality.read().await,
        );

        self.playlist_cache.insert(id, playlist.clone()).await;
        Ok(playlist)
    }

    pub async fn add_favorite_track(&self, id: u32) -> Result<()> {
        let client = self.get_client().await?;
        client.add_favorite_track(id).await?;
        self.favorites_cache.clear().await;
        Ok(())
    }

    pub async fn remove_favorite_track(&self, id: u32) -> Result<()> {
        let client = self.get_client().await?;
        client.remove_favorite_track(id).await?;
        self.favorites_cache.clear().await;
        Ok(())
    }

    pub async fn add_favorite_album(&self, id: &str) -> Result<()> {
        let client = self.get_client().await?;
        client.add_favorite_album(id).await?;
        self.favorites_cache.clear().await;
        Ok(())
    }

    pub async fn remove_favorite_album(&self, id: &str) -> Result<()> {
        let client = self.get_client().await?;
        client.remove_favorite_album(id).await?;
        self.favorites_cache.clear().await;
        Ok(())
    }

    pub async fn add_favorite_artist(&self, id: u32) -> Result<()> {
        let client = self.get_client().await?;
        client.add_favorite_artist(id).await?;
        self.favorites_cache.clear().await;
        Ok(())
    }

    pub async fn remove_favorite_artist(&self, id: u32) -> Result<()> {
        let client = self.get_client().await?;
        client.remove_favorite_artist(id).await?;
        self.favorites_cache.clear().await;
        Ok(())
    }

    pub async fn add_favorite_playlist(&self, id: u32) -> Result<()> {
        let client = self.get_client().await?;
        client.add_favorite_playlist(id).await?;
        self.favorites_cache.clear().await;
        Ok(())
    }

    pub async fn remove_favorite_playlist(&self, id: u32) -> Result<()> {
        let client = self.get_client().await?;
        client.remove_favorite_playlist(id).await?;
        self.favorites_cache.clear().await;
        Ok(())
    }

    pub async fn favorites(&self) -> Result<Favorites> {
        if let Some(cache) = self.favorites_cache.get().await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let audio_quality = self.max_audio_quality.read().await;

        let favorites_result = client.favorites(1000).await?;
        let user_playlists = client.user_playlists().await?;

        let mut albums: Vec<_> = favorites_result
            .albums
            .items
            .into_iter()
            .map(|x| parse_album(x, &audio_quality).into())
            .collect();

        albums.sort_by(|a: &AlbumSimple, b| {
            a.artist
                .name
                .to_lowercase()
                .cmp(&b.artist.name.to_lowercase())
        });

        let mut artists: Vec<_> = favorites_result
            .artists
            .items
            .into_iter()
            .map(parse_artist)
            .collect();
        artists.sort_by_key(|a| a.name.to_lowercase());

        let mut playlists: Vec<_> = user_playlists
            .playlists
            .items
            .into_iter()
            .map(|x| parse_playlist(x, client.user_id(), &audio_quality))
            .collect();

        playlists.sort_by_key(|a| a.title.to_lowercase());

        let mut track_items = favorites_result.tracks.items;
        track_items.sort_by_key(|t| std::cmp::Reverse(t.favorited_at));

        let tracks: Vec<_> = track_items
            .into_iter()
            .map(|x| parse_track(x, &audio_quality))
            .collect();

        let favorites = Favorites {
            albums,
            artists,
            playlists,
            tracks,
        };

        self.favorites_cache.set(favorites.clone()).await;
        Ok(favorites)
    }

    pub async fn create_playlist(
        &self,
        name: String,
        is_public: bool,
        description: String,
        is_collaborative: Option<bool>,
    ) -> Result<Playlist> {
        let client = self.get_client().await?;
        let playlist = client
            .create_playlist(name, is_public, description, is_collaborative)
            .await?;
        let playlist = parse_playlist(
            playlist,
            client.user_id(),
            &*self.max_audio_quality.read().await,
        );
        let cache = self.favorites_cache.get().await;

        if let Some(mut cache) = cache {
            cache.playlists.push(playlist.clone());
            cache.playlists.sort_by(|a, b| a.title.cmp(&b.title));
            self.favorites_cache.set(cache).await;
        }

        Ok(playlist)
    }

    pub async fn delete_playlist(&self, playlist_id: u32) -> Result<()> {
        let client = self.get_client().await?;
        client.delete_playlist(playlist_id).await?;
        let cache = self.favorites_cache.get().await;

        if let Some(mut cache) = cache {
            cache
                .playlists
                .retain(|playlist| playlist.id != playlist_id);

            self.favorites_cache.set(cache).await;
        }

        Ok(())
    }

    pub async fn playlist_add_track(
        &self,
        playlist_id: u32,
        track_ids: &[u32],
    ) -> Result<Playlist> {
        let client = self.get_client().await?;
        client.playlist_add_track(playlist_id, track_ids).await?;
        self.playlist_cache.invalidate(&playlist_id).await;
        self.playlist(playlist_id).await
    }

    pub async fn playlist_delete_track(
        &self,
        playlist_id: u32,
        playlist_track_ids: &[u64],
    ) -> Result<Playlist> {
        let client = self.get_client().await?;
        client
            .playlist_delete_track(playlist_id, playlist_track_ids)
            .await?;
        self.playlist_cache.invalidate(&playlist_id).await;
        self.playlist(playlist_id).await
    }

    pub async fn update_playlist_track_position(
        &self,
        index: usize,
        playlist_id: u32,
        playlist_track_id: u64,
    ) -> Result<Playlist> {
        let client = self.get_client().await?;
        client
            .update_playlist_track_position(index + 1, playlist_id, playlist_track_id)
            .await?;
        self.playlist_cache.invalidate(&playlist_id).await;
        self.playlist(playlist_id).await
    }

    // TODO: To be removed
    pub async fn genres(&self) -> Result<Vec<Genre>> {
        if let Some(cache) = self.genres_cache.get().await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let genres = client.genres().await?;
        let genres: Vec<_> = genres.genres.items.into_iter().map(parse_genre).collect();

        self.genres_cache.set(genres.clone()).await;
        Ok(genres)
    }

    pub async fn genre_playlists(&self, tag: GenrePlaylistSlug) -> Result<Vec<PlaylistSimple>> {
        if let Some(cache) = self.genre_playlists_cache.get(&tag).await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let playlists: Vec<_> = client
            .genre_playlists(tag.genre_id, tag.playlist_slug.as_deref())
            .await?
            .items
            .into_iter()
            .map(|x| parse_playlist_simple(x, client.user_id()))
            .collect();

        self.genre_playlists_cache
            .insert(tag.clone(), playlists.clone())
            .await;

        Ok(playlists)
    }

    pub async fn discover_page(&self, genre_id: Option<u32>) -> Result<DiscoverPage> {
        if let Some(cache) = self.discover_cache.get(&genre_id).await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let result = client.discover_index(genre_id).await?;

        let audio_quality = &*self.max_audio_quality.read().await;

        let parsed = parse_discover(result, audio_quality, client.user_id());
        self.discover_cache.insert(genre_id, parsed.clone()).await;

        Ok(parsed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GenrePlaylistSlug {
    pub genre_id: Option<u32>,
    pub playlist_slug: Option<String>,
}
