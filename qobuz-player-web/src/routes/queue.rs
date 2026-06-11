use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;

use crate::app_state::AppState;

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/queue", get(index))
        .route("/queue/partial", get(queue_partial))
        .route("/queue/autoplay", get(auto_play_partial))
}

async fn index(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let tracklist = state.tracklist_receiver.borrow();
    let tracks = tracklist
        .queue()
        .into_iter()
        .map(|x| x.track.clone())
        .collect::<Vec<_>>();
    let currently_playing_position = tracklist.current_position();
    let auto_play = state.auto_play_receiver.borrow();

    state.render(
        "queue.html",
        &json!({
            "tracks": tracks,
            "currently_playing_position": currently_playing_position,
            "autoplay": *auto_play
        }),
    )
}

async fn queue_partial(State(state): State<Arc<AppState>>) -> Response {
    let tracklist = state.tracklist_receiver.borrow();
    let tracks = tracklist
        .queue()
        .into_iter()
        .map(|x| x.track.clone())
        .collect::<Vec<_>>();
    let currently_playing_position = tracklist.current_position();
    let auto_play = state.auto_play_receiver.borrow();

    state.render(
        "queue-list.html",
        &json!({
            "tracks": tracks,
            "currently_playing_position": currently_playing_position,
            "autoplay": *auto_play
        }),
    )
}

async fn auto_play_partial(State(state): State<Arc<AppState>>) -> Response {
    let auto_play = state.auto_play_receiver.borrow();

    state.render(
        "queue-auto-play.html",
        &json!({
            "autoplay": *auto_play
        }),
    )
}
