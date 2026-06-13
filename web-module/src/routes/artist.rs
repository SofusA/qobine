use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, put},
};
use serde_json::json;

use crate::{AppState, ResponseResult, ok_or_send_error_toast};

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/artist/{id}", get(index))
        .route("/artist/{id}/content", get(content))
        .route("/artist/{id}/top-tracks", get(top_tracks_partial))
        .route("/artist/{id}/top-tracks/page", get(top_tracks_page))
        .route(
            "/artist/{id}/top-tracks/page/partial",
            get(top_tracks_page_partial),
        )
        .route("/artist/{id}/set-favorite", put(set_favorite))
        .route("/artist/{id}/unset-favorite", put(unset_favorite))
        .route(
            "/artist/{artist_id}/play-top-track/{track_index}",
            put(play_top_track),
        )
}

async fn top_tracks_partial(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u32>,
) -> ResponseResult {
    let artist = ok_or_send_error_toast(&state, state.client.artist_page(id).await)?;
    let click_string = format!("/artist/{}/play-top-track/", artist.id);
    let now_playing_id = state.tracklist_receiver.borrow().currently_playing();

    let top_tracks: Vec<_> = artist.top_tracks.iter().take(5).collect();

    Ok(state.render(
        "list-tracks.html",
        &json!({
            "click": click_string,
            "tracks": top_tracks ,
            "show_artist": false,
            "show_track_cover": true,
            "now_playing_id": now_playing_id
        }),
    ))
}

async fn top_tracks_page(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u32>,
) -> ResponseResult {
    let artist = ok_or_send_error_toast(&state, state.client.artist_page(id).await)?;
    let click_string = format!("/artist/{}/play-top-track/", artist.id);

    Ok(state.render(
        "artist-top-tracks.html",
        &json!({
            "click": click_string,
            "artist": artist,
        }),
    ))
}

async fn top_tracks_page_partial(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u32>,
) -> ResponseResult {
    let artist = ok_or_send_error_toast(&state, state.client.artist_page(id).await)?;
    let click_string = format!("/artist/{}/play-top-track/", artist.id);
    let now_playing_id = state.tracklist_receiver.borrow().currently_playing();

    Ok(state.render(
        "list-tracks.html",
        &json!({
            "click": click_string,
            "tracks": artist.top_tracks,
            "show_artist": false,
            "show_track_cover": true,
            "now_playing_id": now_playing_id
        }),
    ))
}

async fn play_top_track(
    State(state): State<Arc<AppState>>,
    Path((artist_id, track_index)): Path<(u32, usize)>,
) -> impl IntoResponse {
    state.controls.play_top_tracks(artist_id, track_index);
}

async fn set_favorite(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> ResponseResult {
    ok_or_send_error_toast(&state, state.client.add_favorite_artist(id).await)?;

    Ok(state.render(
        "toggle-favorite.html",
        &json!({"api": "/artist", "id": id, "is_favorite": true}),
    ))
}

async fn unset_favorite(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> ResponseResult {
    ok_or_send_error_toast(&state, state.client.remove_favorite_artist(id).await)?;

    Ok(state.render(
        "toggle-favorite.html",
        &json!({"api": "/artist", "id": id, "is_favorite": false}),
    ))
}

async fn index(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> impl IntoResponse {
    let url = format!("/artist/{id}/content");
    state.render("lazy-load-component.html", &json!({"url": url}))
}

async fn content(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> ResponseResult {
    let artist = ok_or_send_error_toast(&state, state.client.artist_page(id).await)?;

    let favorites = ok_or_send_error_toast(&state, state.get_favorites().await)?;
    let is_favorite = favorites.artists.iter().any(|artist| artist.id == id);
    let click_string = format!("/artist/{}/play-top-track/", artist.id);
    let top_tracks: Vec<_> = artist.top_tracks.iter().take(5).collect();

    Ok(state.render(
        "artist.html",
        &json!({
            "artist": artist,
            "top_tracks": top_tracks,
            "is_favorite": is_favorite,
            "click": click_string
        }),
    ))
}
