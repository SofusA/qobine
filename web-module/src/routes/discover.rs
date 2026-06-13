use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State},
    routing::get,
};
use player_module::client::GenrePlaylistSlug;
use serde::Deserialize;
use serde_json::json;
use tokio::try_join;

use crate::{AppState, ResponseResult, ok_or_error_page};

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/discover", get(genre_page))
        .route("/discover/playlist", get(playlist_partial))
}

#[derive(Debug, Deserialize)]
struct GenreQuery {
    genre_id: Option<String>,
}

async fn genre_page(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GenreQuery>,
) -> ResponseResult {
    let genre_id = query
        .genre_id
        .as_deref()
        .and_then(|s| s.parse::<u32>().ok());

    let (discover, genres) = ok_or_error_page(
        &state,
        try_join!(state.client.discover_page(genre_id), state.client.genres(),),
    )?;

    Ok(state.render(
        "discover.html",
        &json! ({
            "discover": discover,
            "active_tab": "discover",
            "genres": genres,
            "playlists": discover.playlists
        }),
    ))
}

#[derive(Deserialize)]
struct PlaylistQuery {
    genre_id: Option<String>,
    playlist_tag_slug: Option<String>,
}

async fn playlist_partial(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PlaylistQuery>,
) -> ResponseResult {
    let genre_id = query
        .genre_id
        .as_deref()
        .and_then(|s| s.parse::<u32>().ok());

    let (discover, playlists) = ok_or_error_page(
        &state,
        try_join!(
            state.client.discover_page(genre_id),
            state.client.genre_playlists(GenrePlaylistSlug {
                genre_id,
                playlist_slug: query.playlist_tag_slug,
            })
        ),
    )?;

    let tags = discover.playlists_tags;

    Ok(state.render(
        "discover-playlists.html",
        &json! ({
            "tags": tags,
            "playlists": playlists,
        }),
    ))
}
