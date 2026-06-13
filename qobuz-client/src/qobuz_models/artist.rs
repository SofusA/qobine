use serde::{Deserialize, Serialize};

use crate::qobuz_models::{Image, album::Albums};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtistSearchResults {
    pub query: String,
    pub artists: Artists,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtistsResponse {
    pub artists: Artists,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artists {
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
    pub items: Vec<Artist>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artist {
    pub image: Option<Image>,
    pub name: String,
    pub id: u32,
    pub albums: Option<Albums>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtherArtists {
    pub id: i64,
    pub name: Option<String>,
}
