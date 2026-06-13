use qobuz_client::qobuz_models::playlist::Owner;

pub mod mapper;

#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum TrackStatus {
    Played,
    Playing,
    #[default]
    Unplayed,
    Unplayable,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Track {
    pub id: u32,
    pub title: String,
    pub number: u32,
    pub explicit: bool,
    pub hires_available: bool,
    pub available: bool,
    pub status: TrackStatus,
    pub image: Option<String>,
    pub image_thumbnail: Option<String>,
    pub duration_seconds: u32,
    pub artist_name: Option<String>,
    pub artist_id: Option<u32>,
    pub album_title: Option<String>,
    pub album_id: Option<String>,
    pub playlist_track_id: Option<u64>,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: Artist,
    pub release_year: u32,
    pub hires_available: bool,
    pub explicit: bool,
    pub total_tracks: u32,
    pub tracks: Vec<Track>,
    pub available: bool,
    pub image: String,
    pub image_thumbnail: String,
    pub duration_seconds: u32,
    pub description: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AlbumSimple {
    pub id: String,
    pub title: String,
    pub artist: Artist,
    pub image: String,
    pub available: bool,
    pub hires_available: bool,
    pub release_year: u32,
    pub explicit: bool,
    pub duration_seconds: u32,
}

impl From<Album> for AlbumSimple {
    fn from(value: Album) -> Self {
        Self {
            id: value.id,
            title: value.title,
            artist: value.artist,
            image: value.image,
            available: value.available,
            hires_available: value.hires_available,
            explicit: value.explicit,
            duration_seconds: value.duration_seconds,
            release_year: value.release_year,
        }
    }
}

#[derive(Default, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SearchResults {
    pub query: String,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub tracks: Vec<Track>,
}

#[derive(Default, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Favorites {
    pub albums: Vec<AlbumSimple>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub tracks: Vec<Track>,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Artist {
    pub id: u32,
    pub name: String,
    pub image: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ArtistPage {
    pub id: u32,
    pub name: String,
    pub image: Option<String>,
    pub top_tracks: Vec<Track>,
    pub description: Option<String>,
    pub similar_artists: Vec<Artist>,
    pub albums: Vec<AlbumSimple>,
    pub singles: Vec<AlbumSimple>,
    pub live: Vec<AlbumSimple>,
    pub compilations: Vec<AlbumSimple>,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Playlist {
    pub is_owned: bool,
    pub title: String,
    pub duration_seconds: u32,
    pub id: u32,
    pub image: Option<String>,
    pub tracks: Vec<Track>,
    pub owner: Owner,
}

impl From<Playlist> for PlaylistSimple {
    fn from(value: Playlist) -> Self {
        Self {
            is_owned: value.is_owned,
            title: value.title,
            duration_seconds: value.duration_seconds,
            tracks_count: value.tracks.len() as u32,
            id: value.id,
            image: value.image,
            owner: value.owner,
        }
    }
}
impl From<PlaylistSimple> for Playlist {
    fn from(value: PlaylistSimple) -> Self {
        Self {
            is_owned: value.is_owned,
            title: value.title,
            duration_seconds: value.duration_seconds,
            id: value.id,
            image: value.image,
            tracks: Default::default(),
            owner: value.owner,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PlaylistSimple {
    pub is_owned: bool,
    pub title: String,
    pub duration_seconds: u32,
    pub tracks_count: u32,
    pub id: u32,
    pub image: Option<String>,
    pub owner: Owner,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Genre {
    pub name: String,
    pub id: u32,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct DiscoverPage {
    pub new_releases: Vec<AlbumSimple>,
    pub qobuzissims: Vec<AlbumSimple>,
    pub ideal_discography: Vec<AlbumSimple>,
    pub album_of_the_week: Vec<AlbumSimple>,
    pub most_streamed: Vec<AlbumSimple>,
    pub press_awards: Vec<AlbumSimple>,
    pub playlists: Vec<PlaylistSimple>,
    pub playlists_tags: Vec<PlaylistTag>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct PlaylistTag {
    pub slug: String,
    pub name: String,
}
