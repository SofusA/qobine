use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, put},
};
use serde_json::json;

use crate::{AppState, ResponseResult, ok_or_error_page, ok_or_send_error_toast};

#[derive(Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum Tab {
    Albums,
    Artists,
    Playlists,
    Tracks,
}

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/favorites/{tab}", get(index))
        .route("/favorites/tracks/partial", get(tracks_partial))
        .route("/favorites/tracks/shuffle", put(shuffle_favorite_tracks))
}

async fn index(State(state): State<Arc<AppState>>, Path(tab): Path<Tab>) -> ResponseResult {
    let favorites = ok_or_error_page(&state, state.get_favorites().await)?;

    Ok(state.render(
        "favorites.html",
        &json!({"favorites": favorites, "tab": tab}),
    ))
}

async fn tracks_partial(State(state): State<Arc<AppState>>) -> ResponseResult {
    let favorites = ok_or_send_error_toast(&state, state.get_favorites().await)?;

    Ok(state.render(
        "favorites-tracks.html",
        &json!({"tracks": favorites.tracks}),
    ))
}

async fn shuffle_favorite_tracks(State(state): State<Arc<AppState>>) -> ResponseResult {
    let favorites = ok_or_send_error_toast(&state, state.get_favorites().await)?;
    let track_ids = favorites.tracks.into_iter().map(|x| x.id).collect();

    state.controls.play_tracks(track_ids, true);

    Ok(().into_response())
}
