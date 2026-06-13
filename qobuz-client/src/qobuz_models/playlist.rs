use crate::qobuz_models::track::Tracks;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct User {
    pub id: i64,
    pub login: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPlaylistsResult {
    user: User,
    pub playlists: Playlists,
}

impl From<UserPlaylistsResult> for Vec<Vec<String>> {
    fn from(playlist: UserPlaylistsResult) -> Self {
        vec![playlist.into()]
    }
}

impl From<UserPlaylistsResult> for Vec<String> {
    fn from(playlist: UserPlaylistsResult) -> Self {
        playlist
            .playlists
            .items
            .iter()
            .map(|i| i.name.to_string())
            .collect::<Vec<String>>()
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub id: i64,
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub owner: Owner,
    pub users_count: i64,
    pub images150: Option<Vec<String>>,
    pub images: Option<Vec<String>>,
    pub is_collaborative: bool,
    pub is_published: Option<bool>,
    pub description: String,
    pub created_at: i64,
    pub images300: Option<Vec<String>>,
    pub duration: i64,
    pub updated_at: i64,
    pub published_to: Option<i64>,
    pub tracks_count: i64,
    pub name: String,
    pub is_public: bool,
    pub published_from: Option<i64>,
    pub id: i64,
    pub is_featured: Option<bool>,
    pub position: Option<i64>,
    #[serde(default)]
    pub image_rectangle_mini: Vec<String>,
    pub timestamp_position: Option<i64>,
    #[serde(default)]
    pub image_rectangle: Vec<String>,
    pub slug: Option<String>,
    #[serde(default)]
    pub stores: Vec<String>,
    pub tracks: Option<Tracks>,
}

impl Playlist {
    pub fn set_tracks(&mut self, tracks: &Tracks) {
        self.tracks = Some(tracks.clone());
    }

    pub fn reverse(&mut self) {
        if let Some(tracks) = &mut self.tracks {
            tracks.items.reverse();
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playlists {
    pub offset: i64,
    pub limit: i64,
    pub total: i64,
    pub items: Vec<Playlist>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaylistSimple {
    pub owner: Owner,
    pub image: PlaylistSimpleImage,
    pub description: String,
    pub duration: i64,
    pub tracks_count: i64,
    pub name: String,
    pub id: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaylistSimpleImage {
    pub rectangle: String,
    pub covers: Vec<String>,
}
