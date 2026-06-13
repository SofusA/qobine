use std::sync::Arc;

use axum::{Router, extract::State, response::IntoResponse, routing::get};

use crate::AppState;

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new().route("/controls", get(controls))
}

async fn controls(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.render("controls.html", &())
}
