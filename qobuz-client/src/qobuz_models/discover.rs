use serde::{Deserialize, Serialize};

use crate::qobuz_models::{album_suggestion::AlbumSuggestion, playlist::PlaylistSimple};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Discover {
    pub containers: Containers,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Containers {
    pub new_releases: AlbumContainer,
    pub qobuzissims: AlbumContainer,
    pub ideal_discography: AlbumContainer,
    pub album_of_the_week: AlbumContainer,
    pub most_streamed: AlbumContainer,
    pub press_awards: AlbumContainer,
    pub playlists: PlaylistContainer,
    pub playlists_tags: PlaylistTagsContainer,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AlbumContainer {
    pub data: AlbumData,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AlbumData {
    pub items: Vec<AlbumSuggestion>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistContainer {
    pub data: PlaylistData,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistData {
    pub items: Vec<PlaylistSimple>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTagsContainer {
    pub data: PlaylistTagData,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTagData {
    pub items: Vec<PlaylistTag>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTag {
    pub slug: String,
    pub name: String,
}
