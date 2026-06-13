use crate::qobuz_models::{Image, artist::OtherArtists, artist_page::ArtistName};
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumSuggestion {
    pub id: String,
    pub title: String,
    pub artists: Option<Vec<OtherArtists>>,
    pub image: Image,
    pub duration: u32,
    pub dates: Dates,
    pub parental_warning: bool,
    pub rights: Rights,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumSuggestionResponse {
    pub algorithm: String,
    pub albums: AlbumSuggestions,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumSuggestions {
    pub limit: i64,
    pub items: Vec<AlbumSuggestion>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumOfTheWeekQuery {
    has_more: bool,
    pub items: Vec<AlbumSuggestion>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReleaseQuery {
    has_more: bool,
    pub items: Vec<AlbumSuggestion>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalSupport {
    pub media_number: u32,
    pub track_number: u32,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Genre {
    pub path: Vec<i64>,
    pub name: String,
    pub id: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    id: i64,
    name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dates {
    download: String,
    pub original: String,
    stream: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rights {
    purchasable: bool,
    pub streamable: bool,
    downloadable: bool,
    pub hires_streamable: bool,
    hires_purchasable: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artist {
    pub id: u32,
    pub name: ArtistName,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioInfo {
    pub maximum_bit_depth: u32,
    pub maximum_channel_count: f32,
    pub maximum_sampling_rate: f32,
}
