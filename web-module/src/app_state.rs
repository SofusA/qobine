use axum::response::{Html, IntoResponse, Response};
use controls_module::{
    AutoPlayReceiver, PositionReceiver, Status, StatusReceiver, TracklistReceiver, VolumeReceiver,
    controls::Controls,
    models::{Favorites, Track},
    tracklist::{Tracklist, TracklistType},
};
use futures::try_join;
use player_module::{
    AppResult,
    client::Client,
    database::Database,
    notification::{Notification, NotificationBroadcast},
};
use rfid_module::RfidState;
use serde_json::json;
use skabelon::Templates;
use std::sync::Arc;
use tokio::sync::{broadcast::Sender, mpsc, watch};

use crate::{AlbumData, ServerSentEvent};

pub struct AppState {
    pub tx: Sender<ServerSentEvent>,
    pub web_secret: Option<String>,
    pub rfid_state: Option<RfidState>,
    pub broadcast: Arc<NotificationBroadcast>,
    pub client: Arc<Client>,
    pub controls: Controls,
    pub position_receiver: PositionReceiver,
    pub tracklist_receiver: TracklistReceiver,
    pub status_receiver: StatusReceiver,
    pub volume_receiver: VolumeReceiver,
    pub auto_play_receiver: AutoPlayReceiver,
    pub templates: watch::Receiver<Templates>,
    pub database: Arc<Database>,
    pub connect_device_name: Option<String>,
    pub connect_available_devices: Option<watch::Receiver<Vec<String>>>,
    pub connect_active_device: Option<watch::Receiver<String>>,
    pub set_connect_active_device: Option<mpsc::UnboundedSender<String>>,
}

impl AppState {
    pub fn playing_info(&self) -> PlayingInfo {
        let available_devices = {
            self.connect_available_devices
                .as_ref()
                .map(|x| x.borrow().to_vec())
                .unwrap_or_default()
        };

        let active_device = {
            self.connect_active_device
                .as_ref()
                .map(|x| x.borrow().to_string())
        };

        let current_volume = self.volume_receiver.borrow();
        let current_volume = (*current_volume * 100.0) as u32;

        let tracklist = self.tracklist_receiver.borrow().clone();
        let current_track = tracklist.current_track().cloned();
        let status = *self.status_receiver.borrow();
        let artist_name = current_track
            .as_ref()
            .and_then(|track| track.artist_name.clone());
        let artist_id = current_track.as_ref().and_then(|track| track.artist_id);

        let (title, artist_link, duration_ms, explicit, hires_available) =
            current_track.as_ref().map_or(
                (
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ),
                |track| {
                    (
                        track.title.clone(),
                        artist_id.map(|id| format!("/artist/{id}")),
                        track.duration_seconds * 1000,
                        track.explicit,
                        track.hires_available,
                    )
                },
            );

        let entity = entity_playing(&tracklist, current_track.as_ref());
        let now_playing_id = tracklist.currently_playing();

        let position_ms = self.position_receiver.borrow().as_millis() as u32;

        let number_of_tracks = tracklist.total() as u32;
        let current_position = (tracklist.current_position() + 1) as u32;

        PlayingInfo {
            title,
            now_playing_id,
            artist_link,
            artist_name,
            entity_title: entity.title,
            entity_link: entity.link,
            status,
            cover_image: entity.cover_link,
            number_of_tracks,
            current_position,
            current_volume,
            explicit,
            hires_available,
            duration_ms,
            position_ms,
            available_devices,
            active_device,
        }
    }

    pub fn render<T>(&self, view: &str, context: &T) -> Response
    where
        T: serde::Serialize,
    {
        let playing_info = serde_json::json!({"playing_info": self.playing_info()});

        let context = merge_serialized(&playing_info, context).unwrap();
        let templates = self.templates.borrow();
        let render = templates.render(view, &context);

        Html(render).into_response()
    }

    pub fn send_toast(&self, message: Notification) -> Response {
        let (message_string, severity) = match &message {
            Notification::Error(message) => (message, 1),
            Notification::Warning(message) => (message, 2),
            Notification::Success(message) => (message, 3),
            Notification::Info(message) => (message, 4),
        };

        self.render(
            "send-toast.html",
            &json!({"message": message_string, "severity": severity}),
        )
    }

    pub fn send_sse(&self, event: String, data: String) {
        let event = ServerSentEvent {
            event_name: event,
            event_data: data,
        };

        _ = self.tx.send(event);
    }

    pub async fn get_favorites(&self) -> AppResult<Favorites> {
        self.client.favorites().await
    }

    pub async fn get_album(&self, id: &str) -> AppResult<AlbumData> {
        let (album, suggested_albums) =
            try_join!(self.client.album(id), self.client.suggested_albums(id))?;

        Ok(AlbumData {
            album,
            suggested_albums,
        })
    }

    pub async fn is_album_favorite(&self, id: &str) -> AppResult<bool> {
        let favorites = self.get_favorites().await?;
        Ok(favorites.albums.iter().any(|album| album.id == id))
    }
}

#[derive(Default, Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct Entity {
    title: Option<String>,
    link: Option<String>,
    cover_link: Option<String>,
}

fn entity_playing(tracklist: &Tracklist, current_track: Option<&Track>) -> Entity {
    let track_image = current_track.and_then(|track| track.image.clone());

    match tracklist.list_type() {
        TracklistType::Album(tracklist) => Entity {
            title: Some(tracklist.title.clone()),
            link: Some(format!("/album/{}", tracklist.id)),
            cover_link: tracklist.image.clone().or(track_image),
        },
        TracklistType::Playlist(tracklist) => Entity {
            title: Some(tracklist.title.clone()),
            link: Some(format!("/playlist/{}", tracklist.id)),
            cover_link: track_image,
        },
        TracklistType::TopTracks(tracklist) => Entity {
            title: Some(tracklist.artist_name.clone()),
            link: Some(format!("/artist/{}", tracklist.id)),
            cover_link: track_image,
        },
        TracklistType::Tracks => Entity {
            title: current_track
                .as_ref()
                .and_then(|track| track.album_title.clone()),
            link: current_track
                .as_ref()
                .and_then(|track| track.album_id.as_ref().map(|id| format!("/album/{id}"))),
            cover_link: track_image,
        },
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PlayingInfo {
    title: String,
    now_playing_id: Option<u32>,
    artist_link: Option<String>,
    artist_name: Option<String>,
    entity_title: Option<String>,
    entity_link: Option<String>,
    status: Status,
    cover_image: Option<String>,
    duration_ms: u32,
    position_ms: u32,
    number_of_tracks: u32,
    current_position: u32,
    current_volume: u32,
    explicit: bool,
    hires_available: bool,
    available_devices: Vec<String>,
    active_device: Option<String>,
}

fn merge_serialized<T: serde::Serialize, Y: serde::Serialize>(
    info: &T,
    extra: &Y,
) -> serde_json::Result<serde_json::Value> {
    let mut info_value = serde_json::to_value(info)?;
    let extra_value = serde_json::to_value(extra)?;

    if let (serde_json::Value::Object(info_map), serde_json::Value::Object(extra_map)) =
        (&mut info_value, extra_value)
    {
        for (k, v) in extra_map {
            info_map.insert(k, v);
        }
    }

    Ok(info_value)
}
