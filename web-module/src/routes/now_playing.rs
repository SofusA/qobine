use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    extract::State,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use controls_module::tracklist::Tracklist;
use serde_json::json;

use crate::AppState;

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/", get(index))
        .route("/status", get(status_partial))
        .route("/now-playing/content", get(now_playing_content))
}

async fn index(State(state): State<Arc<AppState>>) -> Response {
    let tracklist = state.tracklist_receiver.borrow().clone();

    if tracklist.current_track().is_none() {
        return Redirect::to("/favorites").into_response();
    }

    let position = *state.position_receiver.borrow();

    let context = now_playing_context(&tracklist, &position);
    state.render("now-playing.html", &context)
}

async fn status_partial(State(state): State<Arc<AppState>>) -> Response {
    state.render("play-pause.html", &())
}

async fn now_playing_content(State(state): State<Arc<AppState>>) -> Response {
    let tracklist = state.tracklist_receiver.borrow().clone();
    let position = *state.position_receiver.borrow();

    let context = now_playing_context(&tracklist, &position);
    state.render("now-playing-content.html", &context)
}

fn now_playing_context(tracklist: &Tracklist, position: &Duration) -> serde_json::Value {
    let position_mseconds = position.as_millis();

    let current_track = tracklist.current_track().cloned();
    let duration_mseconds = current_track
        .as_ref()
        .map(|track| track.duration_seconds.saturating_mul(1000))
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
