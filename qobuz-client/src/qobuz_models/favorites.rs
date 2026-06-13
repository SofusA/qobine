use crate::qobuz_models::{album::Albums, artist::Artist, track::Track};
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Favorites {
    pub albums: Albums,
    pub tracks: Tracks,
    pub artists: Artists,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tracks {
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
    pub items: Vec<Track>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artists {
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
    pub items: Vec<Artist>,
}
