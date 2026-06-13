use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{Path, Query, State},
    routing::get,
};
use controls_module::models::SearchResults;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    Albums,
    Artists,
    Playlists,
    Tracks,
}

use crate::{AppState, ResponseResult, ok_or_error_page, ok_or_send_error_toast};

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new().route("/search/{tab}", get(index).post(search))
}

#[derive(Deserialize)]
struct SearchParameters {
    query: Option<String>,
}

async fn index(
    State(state): State<Arc<AppState>>,
    Path(tab): Path<Tab>,
    Query(parameters): Query<SearchParameters>,
) -> ResponseResult {
    let query = parameters
        .query
        .and_then(|s| if s.is_empty() { None } else { Some(s) });
    let search_results = match query {
        Some(query) => ok_or_error_page(&state, state.client.search(query).await)?,
        None => SearchResults::default(),
    };

    Ok(state.render(
        "search.html",
        &json!({"search_results": search_results, "tab": tab}),
    ))
}

async fn search(
    State(state): State<Arc<AppState>>,
    Path(tab): Path<Tab>,
    Form(parameters): Form<SearchParameters>,
) -> ResponseResult {
    let query = parameters
        .query
        .and_then(|s| if s.is_empty() { None } else { Some(s) });
    let search_results = match query {
        Some(query) => ok_or_send_error_toast(&state, state.client.search(query).await)?,
        None => SearchResults::default(),
    };

    Ok(state.render(
        "search-content.html",
        &json!({"search_results": search_results, "tab": tab }),
    ))
}
