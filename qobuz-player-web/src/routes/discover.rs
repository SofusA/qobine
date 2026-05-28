use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    routing::get,
};
use qobuz_player_controls::{client::GenrePlaylistTag, error::Error};
use serde_json::json;
use tokio::try_join;

use crate::{AppState, Discover, ResponseResult, ok_or_broadcast, ok_or_error_page};

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/discover", get(index))
        .route("/discover/genres", get(genres_tab))
        .route("/discover/genres/{id_or_slug}", get(genre_detail))
}

async fn index(State(state): State<Arc<AppState>>) -> ResponseResult {
    let (albums, playlists) = ok_or_error_page(
        &state,
        try_join!(
            state.client.featured_albums(),
            state.client.featured_playlists(),
        ),
    )?;

    let discover = Discover { albums, playlists };

    Ok(state.render(
        "discover.html",
        &json! ({
            "discover": discover,
            "active_tab": "discover",
            "genres": json!(null),
        }),
    ))
}

async fn genres_tab(State(state): State<Arc<AppState>>) -> ResponseResult {
    let genres = ok_or_error_page(&state, state.client.genres().await)?;

    let (albums, playlists) = ok_or_error_page(
        &state,
        try_join!(
            state.client.featured_albums(),
            state.client.featured_playlists(),
        ),
    )?;

    let discover = Discover { albums, playlists };

    Ok(state.render(
        "discover.html",
        &json! ({
            "discover": discover,
            "active_tab": "genres",
            "genres": genres,
        }),
    ))
}

async fn genre_detail(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> ResponseResult {
    let genres = ok_or_error_page(&state, state.client.genres().await)?;
    let albums = ok_or_error_page(&state, state.client.genre_albums(id).await)?;
    let playlists = ok_or_error_page(
        &state,
        state
            .client
            .genre_playlists(GenrePlaylistTag {
                genre_id: Some(id),
                playlist_tag: None,
            })
            .await,
    )?;

    let genre = genres
        .into_iter()
        .find(|x| x.id == id)
        .ok_or_else(|| Error::Client {
            message: "Unable to find genre".into(),
        });

    let genre = ok_or_broadcast(&state.broadcast, genre)?;

    Ok(state.render(
        "genre-detail.html",
        &json!({
            "genre": genre,
            "albums": albums,
            "playlists": playlists
        }),
    ))
}
