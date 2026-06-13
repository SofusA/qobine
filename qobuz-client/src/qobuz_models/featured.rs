use serde::{Deserialize, Serialize};

use super::{Image, artist::Artist, playlist::Playlist};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturedAlbumsResponse {
    pub albums: FeaturedAlbums,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturedAlbums {
    total: u32,
    limit: u32,
    offset: u32,
    pub items: Vec<FeaturedAlbum>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturedAlbum {
    pub id: String,
    pub title: String,
    pub tracks_count: u32,
    pub release_date_original: String,
    pub artist: Artist,
    pub image: Image,
    pub parental_warning: bool,
    pub hires_streamable: bool,
    pub streamable: bool,
    pub duration: u32,
    pub awards: Vec<Award>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturedPlaylistsResponse {
    pub playlists: FeaturedPlaylists,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturedPlaylists {
    total: u32,
    limit: u32,
    offset: u32,
    pub items: Vec<Playlist>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Award {
    name: String,
}
