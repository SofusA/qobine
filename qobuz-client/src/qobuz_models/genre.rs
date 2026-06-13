use serde::{Deserialize, Serialize};

use crate::qobuz_models::playlist::PlaylistSimple;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreResponse {
    pub genres: GenreResponseInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreResponseInner {
    pub items: Vec<Genre>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Genre {
    pub id: u32,
    pub name: String,
    pub slug: String,
    pub color: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenreFeaturedResponse {
    pub albums: GenreFeaturedAlbums,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenreFeaturedAlbums {
    pub items: Vec<super::album_suggestion::AlbumSuggestion>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenreFeaturedPlaylists {
    pub items: Vec<PlaylistSimple>,
}
