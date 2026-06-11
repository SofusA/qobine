use std::{
    ops::{Add, Sub},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use axum_extra::extract::Form;
use qobuz_player_controls::models::{AlbumSimple, Artist, Playlist, Track};
use qobuz_player_player::{
    AppResult, client::Client, database::ReferenceType, notification::Notification,
};
use qobuz_player_rfid::{LinkAlbumRfid, LinkPlaylistRfid, handle_play_scan};
use serde::Deserialize;

use crate::{AppState, ResponseResult, hx_redirect, ok_or_send_error_toast};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/play-info", get(playing_info))
        .route("/api/play", put(play))
        .route("/api/play-pause", put(play_pause))
        .route("/api/pause", put(pause))
        .route("/api/previous", put(previous))
        .route("/api/next", put(next))
        .route("/api/volume", post(set_volume))
        .route("/api/enable-auto-play", post(enable_auto_play))
        .route("/api/disable-auto-play", post(disable_auto_play))
        .route("/api/volume/up", put(set_volume_up))
        .route("/api/volume/down", put(set_volume_down))
        .route("/api/position", post(set_position))
        .route("/api/skip-to/{track_number}", put(skip_to))
        .route(
            "/api/remove-queue-item/{index}",
            put(remove_index_from_queue),
        )
        .route("/api/track/play/{track_id}", put(play_track))
        .route("/api/track/action", put(track_action))
        .route("/api/queue/reorder", put(reorder_queue))
        .route("/api/favorites/albums", get(favorite_albums))
        .route("/api/favorites/artists", get(favorite_artists))
        .route("/api/favorites/playlists", get(favorite_playlists))
        .route("/api/favorites/tracks", get(favorite_tracks))
        .route(
            "/api/rfid/reference/{reference}",
            get(rfid_reference).put(play_rfid_reference),
        )
        .route("/api/rfid/reference/album", post(link_album_rfid_reference))
        .route(
            "/api/rfid/reference/playlist",
            post(link_playlist_rfid_reference),
        )
        .route("/api/active-device/{device_id}", post(set_active_device))
        .route("/api/available-devices", get(get_available_devices))
}

async fn playing_info(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let playing_info = state.playing_info();
    Json(playing_info)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TrackAction {
    AddFavorite,
    RemoveFavorite,
    AddToQueue,
    PlayNext,
    AddToPlaylist,
}
#[derive(Deserialize)]
struct TrackActionParameters {
    track_id: u32,
    action: TrackAction,
}
async fn track_action(
    State(state): State<Arc<AppState>>,
    Form(req): Form<TrackActionParameters>,
) -> ResponseResult {
    match req.action {
        TrackAction::AddFavorite => {
            ok_or_send_error_toast(&state, state.client.add_favorite_track(req.track_id).await)?;
            state.send_sse("tracklist".into(), "New favorite track".into());
            Ok(state.send_toast(Notification::Info("Track added to favorites".into())))
        }
        TrackAction::RemoveFavorite => {
            ok_or_send_error_toast(
                &state,
                state.client.remove_favorite_track(req.track_id).await,
            )?;
            state.send_sse("tracklist".into(), "Removed favorite track".into());
            Ok(state.send_toast(Notification::Info("Track removed from favorites".into())))
        }
        TrackAction::AddToQueue => {
            state.controls.add_tracks_to_queue(vec![req.track_id]);
            state.send_sse("tracklist".into(), "Track added to queue".into());
            Ok(state.send_toast(Notification::Info("Track added to queue".into())))
        }
        TrackAction::PlayNext => {
            state.controls.play_tracks_next(vec![req.track_id]);
            state.send_sse("tracklist".into(), "Track queued next".into());
            Ok(state.send_toast(Notification::Info("Track queued next".into())))
        }
        TrackAction::AddToPlaylist => Ok(hx_redirect(&format!(
            "/playlist/add-track/{}",
            req.track_id
        ))),
    }
}

#[derive(Deserialize)]
struct ReorderQueueParameters {
    new_order: Vec<usize>,
}

async fn reorder_queue(
    State(state): State<Arc<AppState>>,
    Form(req): Form<ReorderQueueParameters>,
) -> impl IntoResponse {
    state.controls.reorder_queue(req.new_order);
}

async fn remove_index_from_queue(
    State(state): State<Arc<AppState>>,
    Path(index): Path<usize>,
) -> impl IntoResponse {
    state.controls.remove_index_from_queue(index);
}

async fn play_track(
    State(state): State<Arc<AppState>>,
    Path(track_id): Path<u32>,
) -> impl IntoResponse {
    state.controls.play_track(track_id);
}

async fn play(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.controls.play();
}

async fn play_pause(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.controls.play_pause();
}

async fn pause(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.controls.pause();
}

async fn previous(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.controls.previous();
}

async fn next(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.controls.next();
}

async fn skip_to(
    State(state): State<Arc<AppState>>,
    Path(track_number): Path<usize>,
) -> impl IntoResponse {
    state.controls.skip_to_position(track_number, true);
}

#[derive(serde::Deserialize, Clone, Copy)]
struct SliderParameters {
    value: i32,
}
async fn set_volume(
    State(state): State<Arc<AppState>>,
    axum::Form(parameters): axum::Form<SliderParameters>,
) -> impl IntoResponse {
    let mut volume = parameters.value;

    if volume < 0 {
        volume = 0;
    };

    if volume > 100 {
        volume = 100;
    };

    let formatted_volume = volume as f32 / 100.0;

    state.controls.set_volume(formatted_volume);
}

async fn enable_auto_play(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.controls.set_auto_play(true);
}

async fn disable_auto_play(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.controls.set_auto_play(false);
}

async fn set_volume_up(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let current_volume = state.volume_receiver.borrow();
    let mut new_volume = current_volume.add(0.05);

    if new_volume > 1.0 {
        new_volume = 1.0
    }

    state.controls.set_volume(new_volume);
}

async fn set_volume_down(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let current_volume = state.volume_receiver.borrow();
    let mut new_volume = current_volume.sub(0.05);

    if new_volume < 0.0 {
        new_volume = 0.0
    }

    state.controls.set_volume(new_volume);
}

async fn set_position(
    State(state): State<Arc<AppState>>,
    axum::Form(parameters): axum::Form<SliderParameters>,
) -> impl IntoResponse {
    let time = Duration::from_millis(parameters.value as u64);
    state.controls.seek(time);
}

async fn favorite_albums(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match get_favorite_albums(&state.client).await {
        Ok(albums) => Json(albums).into_response(),
        Err(err) => err.to_string().into_response(),
    }
}

async fn get_favorite_albums(client: &Client) -> AppResult<Vec<AlbumSimple>> {
    let favorites = client.favorites().await?;
    Ok(favorites.albums)
}

async fn favorite_artists(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match get_favorite_artists(&state.client).await {
        Ok(artists) => Json(artists).into_response(),
        Err(err) => err.to_string().into_response(),
    }
}

async fn get_favorite_artists(client: &Client) -> AppResult<Vec<Artist>> {
    let favorites = client.favorites().await?;
    Ok(favorites.artists)
}

async fn favorite_playlists(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match get_favorite_playlists(&state.client).await {
        Ok(playlists) => Json(playlists).into_response(),
        Err(err) => err.to_string().into_response(),
    }
}

async fn get_favorite_playlists(client: &Client) -> AppResult<Vec<Playlist>> {
    let favorites = client.favorites().await?;
    Ok(favorites.playlists)
}

async fn favorite_tracks(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match get_favorite_tracks(&state.client).await {
        Ok(tracks) => Json(tracks).into_response(),
        Err(err) => err.to_string().into_response(),
    }
}

async fn get_favorite_tracks(client: &Client) -> AppResult<Vec<Track>> {
    let favorites = client.favorites().await?;
    Ok(favorites.tracks)
}

async fn rfid_reference(
    State(state): State<Arc<AppState>>,
    Path(reference): Path<String>,
) -> Json<Option<ReferenceType>> {
    Json(state.database.get_reference(&reference).await)
}

async fn play_rfid_reference(State(state): State<Arc<AppState>>, Path(reference): Path<String>) {
    handle_play_scan(
        &state.database,
        &state.controls,
        &state.broadcast,
        &reference,
        &state.tracklist_receiver,
        None,
        None,
        state.connect_device_name.as_deref(),
        state.set_connect_active_device.clone(),
    )
    .await;
}

async fn link_album_rfid_reference(
    State(state): State<Arc<AppState>>,
    Json(link): Json<LinkAlbumRfid>,
) -> ResponseResult {
    let reference = ReferenceType::Album(link.id);

    ok_or_send_error_toast(
        &state,
        state
            .database
            .add_rfid_reference(link.rfid_id, reference)
            .await,
    )?;

    Ok(state.send_toast(Notification::Success("Link complete".into())))
}

async fn link_playlist_rfid_reference(
    State(state): State<Arc<AppState>>,
    Json(link): Json<LinkPlaylistRfid>,
) -> ResponseResult {
    let reference = ReferenceType::Playlist(link.id);

    ok_or_send_error_toast(
        &state,
        state
            .database
            .add_rfid_reference(link.rfid_id, reference)
            .await,
    )?;

    Ok(state.send_toast(Notification::Success("Link complete".into())))
}

async fn set_active_device(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
) -> impl IntoResponse {
    if let Some(sender) = &state.set_connect_active_device {
        sender.send(device_id).unwrap();
    }
}

async fn get_available_devices(State(state): State<Arc<AppState>>) -> Response {
    state.render("connect-content.html", &{})
}
