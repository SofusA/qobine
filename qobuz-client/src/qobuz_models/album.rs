use crate::qobuz_models::{Image, artist::Artist, track::Tracks};
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Album {
    pub artist: Artist,
    pub description: Option<String>,
    pub duration: Option<i64>,
    pub hires_streamable: bool,
    pub id: String,
    pub image: Image,
    pub parental_warning: bool,
    pub release_date_original: String,
    pub streamable: bool,
    pub title: String,
    pub tracks: Option<Tracks>,
    pub tracks_count: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumSearchResults {
    pub query: String,
    pub albums: Albums,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Albums {
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
    pub items: Vec<Album>,
}
