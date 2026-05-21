use std::{path::PathBuf, sync::Mutex};

use crate::{
    database::Credentials,
    models::{
        Album, AlbumSimple, ArtistPage, Favorites, Genre, Playlist, PlaylistSimple, SearchResults,
        Track,
        mapper::{
            parse_album, parse_album_simple, parse_artist, parse_artist_page, parse_featured_album,
            parse_genre, parse_playlist, parse_playlist_simple, parse_search_results, parse_track,
        },
    },
};
use futures::future::join_all;
use moka::future::Cache;
use qobuz_player_client::{
    client::{
        AudioQuality, FeaturedAlbumType, FeaturedGenreAlbumType, FeaturedPlaylistType, OAuthResult,
        ReleaseType, browser_oauth_login,
    },
    qobuz_models::{LegacyTrackUrl, TrackInfo},
    stream::flac_source_stream::SeekableStreamReader,
};
use time::Duration;
use tokio::{
    sync::{OnceCell, RwLock},
    try_join,
};

use crate::{AppResult, error::Error, simple_cache::SimpleCache};

pub use qobuz_player_client::client::exchange_oauth_code;
pub use qobuz_player_client::client::get_app_id;

type QobuzClient = qobuz_player_client::client::Client;
type Result<T, E = Error> = std::result::Result<T, E>;

pub struct Client {
    qobuz_client: OnceCell<RwLock<QobuzClient>>,
    credentials: Mutex<Option<Credentials>>,
    max_audio_quality: RwLock<AudioQuality>,
    legacy_streaming: bool,
    favorites_cache: SimpleCache<Favorites>,
    featured_albums_cache: SimpleCache<Vec<(String, Vec<AlbumSimple>)>>,
    featured_playlists_cache: SimpleCache<Vec<(String, Vec<Playlist>)>>,
    genres_cache: SimpleCache<Vec<Genre>>,
    genre_albums_cache: Cache<u32, Vec<(String, Vec<AlbumSimple>)>>,
    genre_playlists_cache: Cache<u32, Vec<PlaylistSimple>>,
    album_cache: Cache<String, Album>,
    artist_cache: Cache<u32, ArtistPage>,
    playlist_cache: Cache<u32, Playlist>,
    suggested_albums_cache: Cache<String, Vec<AlbumSimple>>,
    search_cache: Cache<String, SearchResults>,
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
        legacy_streaming: bool,
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
            legacy_streaming,
        );

        Ok((client, oauth_result))
    }

    pub fn is_legacy_streaming(&self) -> bool {
        self.legacy_streaming
    }

    pub fn new(
        credentials: Option<Credentials>,
        max_audio_quality: AudioQuality,
        legacy_streaming: bool,
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

        let genre_albums_cache = moka::future::CacheBuilder::new(1000)
            .time_to_live(std::time::Duration::from_secs(60 * 60 * 24))
            .build();

        let genre_playlists_cache = moka::future::CacheBuilder::new(1000)
            .time_to_live(std::time::Duration::from_secs(60 * 60 * 24))
            .build();

        let credentials = Mutex::new(credentials);
        let max_audio_quality = RwLock::new(max_audio_quality);

        Self {
            qobuz_client: Default::default(),
            credentials,
            max_audio_quality,
            legacy_streaming,
            favorites_cache: SimpleCache::new(Duration::days(1)),
            featured_albums_cache: SimpleCache::new(Duration::days(1)),
            featured_playlists_cache: SimpleCache::new(Duration::days(1)),
            genres_cache: SimpleCache::new(Duration::days(7)),
            genre_albums_cache,
            genre_playlists_cache,
            album_cache,
            artist_cache,
            playlist_cache,
            suggested_albums_cache,
            search_cache,
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

        let client = QobuzClient::new(
            &credentials.user_auth_token,
            credentials.user_id,
            *max_audio_quality,
            self.legacy_streaming,
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

    pub async fn track_url(&self, track_id: u32) -> Result<TrackInfo> {
        let mut client = self.get_client_mut().await?;
        let info = client.track_url(track_id).await?;
        Ok(info)
    }

    pub async fn track_url_legacy(&self, track_id: u32) -> Result<LegacyTrackUrl> {
        let client = self.get_client().await?;
        let info = client.track_url_legacy(track_id).await?;
        Ok(info)
    }

    pub async fn stream_track_legacy(
        &self,
        url: &str,
        cache_path: &std::path::Path,
    ) -> Result<SeekableStreamReader> {
        let client = self.get_client().await?;
        let stream = client.stream_track_legacy(url, cache_path).await?;
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
                .map(|x| parse_album_simple(x, &audio_quality))
                .collect(),
            live.items
                .into_iter()
                .map(|x| parse_album_simple(x, &audio_quality))
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

    pub async fn featured_albums(&self) -> Result<Vec<(String, Vec<AlbumSimple>)>> {
        if let Some(cache) = self.featured_albums_cache.get().await {
            return Ok(cache);
        }

        let client = self.get_client().await?;

        let (press_awards, most_streamed, new_releases, qobuzissims, ideal_discography) = tokio::try_join!(
            async {
                Ok::<_, Error>(
                    client
                        .featured_albums(FeaturedAlbumType::PressAwards)
                        .await?
                        .albums
                        .items
                        .into_iter()
                        .map(parse_featured_album)
                        .collect(),
                )
            },
            async {
                Ok(client
                    .featured_albums(FeaturedAlbumType::MostStreamed)
                    .await?
                    .albums
                    .items
                    .into_iter()
                    .map(parse_featured_album)
                    .collect())
            },
            async {
                Ok(client
                    .featured_albums(FeaturedAlbumType::NewReleases)
                    .await?
                    .albums
                    .items
                    .into_iter()
                    .map(parse_featured_album)
                    .collect())
            },
            async {
                Ok(client
                    .featured_albums(FeaturedAlbumType::Qobuzissims)
                    .await?
                    .albums
                    .items
                    .into_iter()
                    .map(parse_featured_album)
                    .collect())
            },
            async {
                Ok(client
                    .featured_albums(FeaturedAlbumType::IdealDiscography)
                    .await?
                    .albums
                    .items
                    .into_iter()
                    .map(parse_featured_album)
                    .collect())
            },
        )?;

        let albums = vec![
            ("Press awards".to_string(), press_awards),
            ("Most streamed".to_string(), most_streamed),
            ("New releases".to_string(), new_releases),
            ("Qobuzissims".to_string(), qobuzissims),
            ("Ideal discography".to_string(), ideal_discography),
        ];

        self.featured_albums_cache.set(albums.clone()).await;

        Ok(albums)
    }

    pub async fn featured_playlists(&self) -> Result<Vec<(String, Vec<Playlist>)>> {
        if let Some(cache) = self.featured_playlists_cache.get().await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let audio_quality = self.max_audio_quality.read().await;
        let editor_picks = client
            .featured_playlists(FeaturedPlaylistType::EditorsPick)
            .await?
            .playlists
            .items
            .into_iter()
            .map(|x| parse_playlist(x, client.user_id(), &audio_quality))
            .collect();

        let playlists = vec![("Editor picks".to_string(), editor_picks)];

        self.featured_playlists_cache.set(playlists.clone()).await;

        Ok(playlists)
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
        artists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        let mut playlists: Vec<_> = user_playlists
            .playlists
            .items
            .into_iter()
            .map(|x| parse_playlist(x, client.user_id(), &audio_quality))
            .collect();

        playlists.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

        let mut tracks: Vec<_> = favorites_result
            .tracks
            .items
            .into_iter()
            .map(|x| parse_track(x, &audio_quality))
            .collect();

        tracks.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

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

    pub async fn genre_albums(&self, genre_id: u32) -> Result<Vec<(String, Vec<AlbumSimple>)>> {
        if let Some(cache) = self.genre_albums_cache.get(&genre_id).await {
            return Ok(cache);
        }

        let client = self.get_client().await?;

        let (press_awards, most_streamed, new_releases, qobuzissims, best_sellers) = tokio::try_join!(
            async {
                Ok::<_, Error>(
                    client
                        .genre_albums(genre_id, FeaturedGenreAlbumType::PressAwards)
                        .await?
                        .albums
                        .items
                        .into_iter()
                        .map(parse_featured_album)
                        .collect(),
                )
            },
            async {
                Ok(client
                    .genre_albums(genre_id, FeaturedGenreAlbumType::MostStreamed)
                    .await?
                    .albums
                    .items
                    .into_iter()
                    .map(parse_featured_album)
                    .collect())
            },
            async {
                Ok(client
                    .genre_albums(genre_id, FeaturedGenreAlbumType::NewReleases)
                    .await?
                    .albums
                    .items
                    .into_iter()
                    .map(parse_featured_album)
                    .collect())
            },
            async {
                Ok(client
                    .genre_albums(genre_id, FeaturedGenreAlbumType::Qobuzissims)
                    .await?
                    .albums
                    .items
                    .into_iter()
                    .map(parse_featured_album)
                    .collect())
            },
            async {
                Ok(client
                    .genre_albums(genre_id, FeaturedGenreAlbumType::BestSellers)
                    .await?
                    .albums
                    .items
                    .into_iter()
                    .map(parse_featured_album)
                    .collect())
            },
        )?;

        let albums = vec![
            ("Press awards".to_string(), press_awards),
            ("Most streamed".to_string(), most_streamed),
            ("New releases".to_string(), new_releases),
            ("Qobuzissims".to_string(), qobuzissims),
            ("Best sellers".to_string(), best_sellers),
        ];

        self.genre_albums_cache
            .insert(genre_id, albums.clone())
            .await;

        Ok(albums)
    }

    pub async fn genre_playlists(&self, genre_id: u32) -> Result<Vec<PlaylistSimple>> {
        if let Some(cache) = self.genre_playlists_cache.get(&genre_id).await {
            return Ok(cache);
        }

        let client = self.get_client().await?;
        let playlists: Vec<_> = client
            .genre_playlists(genre_id)
            .await?
            .items
            .into_iter()
            .map(|x| parse_playlist_simple(x, client.user_id()))
            .collect();

        self.genre_playlists_cache
            .insert(genre_id, playlists.clone())
            .await;

        Ok(playlists)
    }
}
