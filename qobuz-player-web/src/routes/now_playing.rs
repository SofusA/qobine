use std::sync::Arc;

use axum::{Router, extract::State, response::Response, routing::get};
use serde_json::json;

use crate::AppState;

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/", get(index))
        .route("/status", get(status_partial))
        .route("/now-playing/content", get(now_playing_content))
}

async fn index(State(state): State<Arc<AppState>>) -> Response {
    let context = now_playing_context(&state).await;
    state.render("now-playing.html", &context)
}

async fn status_partial(State(state): State<Arc<AppState>>) -> Response {
    state.render("play-pause.html", &())
}

async fn now_playing_content(State(state): State<Arc<AppState>>) -> Response {
    let context = now_playing_context(&state).await;
    state.render("now-playing-content.html", &context)
}

async fn now_playing_context(state: &AppState) -> serde_json::Value {
    let tracklist = state.tracklist_receiver.borrow().clone();
    let position_mseconds = state.position_receiver.borrow().as_millis();

    let current_track = tracklist.current_track().cloned();
    let duration_mseconds = current_track
        .as_ref()
        .map(|track| track.duration_seconds * 1000)
        .unwrap_or_default();

    let position_string = mseconds_to_mm_ss(position_mseconds);
    let duration_string = mseconds_to_mm_ss(duration_mseconds);

    json!({
        "position_string": position_string,
        "duration_string": duration_string,
    })
}

fn mseconds_to_mm_ss<T: Into<u128>>(mseconds: T) -> String {
    let seconds = mseconds.into() / 1000;

    let minutes = seconds / 60;
    let seconds = seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}
