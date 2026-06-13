use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, put},
};
use axum_extra::extract::Form;
use player_module::{database::ReferenceType, notification::Notification};
use serde::Deserialize;
use serde_json::json;

use crate::{AppState, ResponseResult, ok_or_send_error_toast};

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/album/{id}", get(index))
        .route("/album/{id}/content", get(content))
        .route("/album/{id}/tracks", get(album_tracks_partial))
        .route("/album/{id}/set-favorite", put(set_favorite))
        .route("/album/{id}/unset-favorite", put(unset_favorite))
        .route("/album/{id}/play", put(play))
        .route("/album/{id}/play/{track_position}", put(play_track))
        .route("/album/{id}/link", put(link))
        .route("/album/action", put(action))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    AddToQueue,
    PlayNext,
}
#[derive(Deserialize)]
struct ActionParameters {
    id: String,
    action: Action,
}
async fn action(
    State(state): State<Arc<AppState>>,
    Form(req): Form<ActionParameters>,
) -> ResponseResult {
    match req.action {
        Action::AddToQueue => {
            let album_data = ok_or_send_error_toast(&state, state.get_album(&req.id).await)?;
            let track_ids = album_data.album.tracks.into_iter().map(|x| x.id).collect();

            state.controls.add_tracks_to_queue(track_ids);
            Ok(state.send_toast(Notification::Success(format!(
                "{} added to queue",
                album_data.album.title
            ))))
        }
        Action::PlayNext => {
            let album_data = ok_or_send_error_toast(&state, state.get_album(&req.id).await)?;
            let track_ids = album_data.album.tracks.into_iter().map(|x| x.id).collect();

            state.controls.play_tracks_next(track_ids);

            Ok(state.send_toast(Notification::Success(format!(
                "Playing {} next",
                album_data.album.title
            ))))
        }
    }
}

async fn play_track(
    State(state): State<Arc<AppState>>,
    Path((id, track_position)): Path<(String, usize)>,
) -> impl IntoResponse {
    state.controls.play_album(&id, track_position);
}

async fn set_favorite(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ResponseResult {
    ok_or_send_error_toast(&state, state.client.add_favorite_album(&id).await)?;

    Ok(state.render(
        "toggle-favorite.html",
        &json!({"api": "/album", "id": id, "is_favorite": true}),
    ))
}

async fn unset_favorite(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ResponseResult {
    ok_or_send_error_toast(&state, state.client.remove_favorite_album(&id).await)?;

    Ok(state.render(
        "toggle-favorite.html",
        &json!({"api": "/album", "id": id, "is_favorite": false}),
    ))
}

async fn play(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> impl IntoResponse {
    state.controls.play_album(&id, 0);
}

async fn link(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> impl IntoResponse {
    let Some(rfid_state) = state.rfid_state.clone() else {
        return;
    };

    rfid_module::link(
        rfid_state,
        ReferenceType::Album(id),
        state.broadcast.clone(),
    )
    .await;
}

async fn index(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> impl IntoResponse {
    let url = format!("/album/{id}/content");
    state.render("lazy-load-component.html", &json!({"url": url}))
}

async fn content(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> ResponseResult {
    let album_data = ok_or_send_error_toast(&state, state.get_album(&id).await)?;
    let is_favorite = ok_or_send_error_toast(&state, state.is_album_favorite(&id).await)?;

    let duration = album_data.album.duration_seconds / 60;

    let click_string = format!("/album/{}/play/", album_data.album.id);

    Ok(state.render(
        "album.html",
        &json!({
            "album": album_data.album,
            "duration": duration,
            "suggested_albums": album_data.suggested_albums,
            "is_favorite": is_favorite,
            "rfid": state.rfid_state.is_some(),
            "click": click_string
        }),
    ))
}

async fn album_tracks_partial(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ResponseResult {
    let album = ok_or_send_error_toast(&state, state.client.album(&id).await)?;
    let click_string = format!("/album/{}/play/", album.id);

    Ok(state.render(
        "album-tracks.html",
        &json!({
            "album": album,
            "click": click_string
        }),
    ))
}
