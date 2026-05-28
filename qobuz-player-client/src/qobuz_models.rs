use serde::{Deserialize, Serialize};
use snafu::prelude::*;

pub mod album;
pub mod album_suggestion;
pub mod artist;
pub mod artist_page;
pub mod discover;
pub mod favorites;
pub mod featured;
pub mod genre;
pub mod playlist;
pub mod search_results;
pub mod track;

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Composer {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub albums_count: i64,
    pub image: Option<Image>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub small: String,
    pub thumbnail: Option<String>,
    pub large: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackInfo {
    pub url_template: String,
    pub mime_type: String,
    pub n_segments: u8,
    #[serde(default)]
    pub key_id: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub sampling_rate: Option<u32>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub n_samples: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TrackUrl {
    pub url: String,
    pub format_id: i32,
    pub mime_type: String,
    pub sampling_rate: f64,
    pub bit_depth: i32,
}

pub enum UrlType {
    Album { id: String },
    Playlist { id: i64 },
    Track { id: i32 },
}

#[derive(Snafu, Debug)]
pub enum UrlTypeError {
    #[snafu(display("This uri contains an unfamiliar domain."))]
    WrongDomain,
    #[snafu(display("The URL contains an invalid path."))]
    InvalidPath,
    #[snafu(display("The URL is invalid."))]
    InvalidUrl,
    #[snafu(display("An unknown error has occurred."))]
    Unknown,
}

pub type ParseUrlResult<T, E = UrlTypeError> = Result<T, E>;

pub fn parse_url(string_url: &str) -> ParseUrlResult<UrlType> {
    let url = url::Url::parse(string_url).map_err(|_| UrlTypeError::InvalidUrl)?;

    let host = url.host_str().ok_or(UrlTypeError::InvalidUrl)?;
    let mut path = url.path_segments().ok_or(UrlTypeError::InvalidUrl)?;

    if host != "play.qobuz.com" && host != "open.qobuz.com" {
        return Err(UrlTypeError::WrongDomain);
    }

    match path.next() {
        Some("album") => {
            tracing::debug!("this is an album");
            let id = path.next().ok_or(UrlTypeError::InvalidPath)?.to_string();
            Ok(UrlType::Album { id })
        }
        Some("playlist") => {
            tracing::debug!("this is a playlist");
            let id_str = path.next().ok_or(UrlTypeError::InvalidPath)?;
            let id = id_str
                .parse::<i64>()
                .map_err(|_| UrlTypeError::InvalidPath)?;
            Ok(UrlType::Playlist { id })
        }
        Some("track") => {
            tracing::debug!("this is a track");
            let id_str = path.next().ok_or(UrlTypeError::InvalidPath)?;
            let id = id_str
                .parse::<i32>()
                .map_err(|_| UrlTypeError::InvalidPath)?;
            Ok(UrlType::Track { id })
        }
        None => {
            tracing::debug!("no path, cannot use path");
            Err(UrlTypeError::InvalidPath)
        }
        _ => Err(UrlTypeError::Unknown),
    }
}
