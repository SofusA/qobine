use crate::qobuz_models::album::Album;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tracks {
    pub offset: i64,
    pub limit: i64,
    pub total: i64,
    pub items: Vec<Track>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub album: Option<Album>,
    pub duration: u32,
    pub hires_streamable: bool,
    pub id: u32,
    pub performer: Option<Performer>,
    pub streamable: bool,
    pub title: String,
    pub track_number: u32,
    pub parental_warning: bool,
    pub playlist_track_id: Option<u64>,
    #[serde(default)]
    pub favorited_at: Option<i64>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Performer {
    pub id: i64,
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackSuggestionResponse {
    pub algorithm: String,
    pub tracks: TrackSuggestions,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackSuggestions {
    pub limit: i64,
    pub items: Vec<Track>,
}

#[derive(Debug, Serialize)]
pub struct SuggestTrackRequest {
    pub limit: u32,
    pub listened_tracks_ids: Vec<u32>,
    pub track_to_analysed: Vec<SuggestTrackInput>,
}

#[derive(Debug, Serialize)]
pub struct SuggestTrackInput {
    pub artist_id: Option<i64>,
    pub genre_id: Option<u32>,
    pub label_id: Option<u32>,
    pub track_id: u32,
}
