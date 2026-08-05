use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post, put},
};
use axum_extra::extract::Form;
use controls_module::tracklist::PlayingEntity;
use player_module::{database::ReferenceType, error::PlayerError, notification::Notification};
use serde::Deserialize;
use serde_json::json;

use crate::{AppState, ResponseResult, hx_redirect, ok_or_send_error_toast};

pub fn routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/playlist/create", get(create).post(create_form))
        .route("/playlist/{id}", get(index).delete(delete))
        .route("/playlist/{id}/content", get(content))
        .route("/playlist/{id}/tracks", get(tracks_partial))
        .route("/playlist/{id}/tracks/edit", get(edit_tracks_partial))
        .route("/playlist/{id}/set-favorite", put(set_favorite))
        .route("/playlist/{id}/unset-favorite", put(unset_favorite))
        .route("/playlist/{id}/play", put(play))
        .route("/playlist/{id}/play/shuffle", put(shuffle))
        .route("/playlist/{id}/play/{track_position}", put(play_track))
        .route("/playlist/{id}/link", put(link))
        .route("/playlist/add-track/{id}", get(add_track_to_playlist_page))
        .route(
            "/playlist/remove-track",
            post(remove_track_from_playlist_action),
        )
        .route("/playlist/add-track", post(add_track_to_playlist_action))
        .route("/playlist/reorder", post(reorder_tracks))
        .route("/playlist/action", put(action))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    AddToQueue,
    PlayNext,
}
#[derive(Deserialize)]
struct ActionParameters {
    id: u32,
    action: Action,
}
async fn action(
    State(state): State<Arc<AppState>>,
    Form(req): Form<ActionParameters>,
) -> ResponseResult {
    match req.action {
        Action::AddToQueue => {
            let playlist = ok_or_send_error_toast(&state, state.client.playlist(req.id).await)?;
            let tracks = playlist.tracks;

            state.controls.add_tracks_to_queue(tracks);
            Ok(state.send_toast(&Notification::Success(format!(
                "{} added to queue",
                playlist.title
            ))))
        }
        Action::PlayNext => {
            let playlist = ok_or_send_error_toast(&state, state.client.playlist(req.id).await)?;
            let tracks = playlist.tracks;

            state.controls.play_tracks_next(tracks);
            Ok(state.send_toast(&Notification::Success(format!(
                "Playing {} next",
                playlist.title
            ))))
        }
    }
}

#[derive(Deserialize)]
struct AddTrackParameters {
    track_id: u32,
    playlist_id: u32,
}

async fn add_track_to_playlist_action(
    State(state): State<Arc<AppState>>,
    Form(req): Form<AddTrackParameters>,
) -> ResponseResult {
    let res = state
        .client
        .playlist_add_track(req.playlist_id, &[req.track_id])
        .await;
    let res = ok_or_send_error_toast(&state, res)?;

    Ok(state.send_toast(&Notification::Success(format!("Added to {}", res.title))))
}

#[derive(Deserialize)]
struct RemoveTrackParameters {
    track_id: u64,
    playlist_id: u32,
}

async fn remove_track_from_playlist_action(
    State(state): State<Arc<AppState>>,
    Form(req): Form<RemoveTrackParameters>,
) -> ResponseResult {
    let res = state
        .client
        .playlist_delete_track(req.playlist_id, &[req.track_id])
        .await;
    let res = ok_or_send_error_toast(&state, res)?;

    Ok(state.render("playlist-edit-tracks.html", &json!({"playlist": res,})))
}

#[derive(Deserialize)]
struct ReorderPlaylistParameters {
    new_order: Vec<usize>,
    playlist_id: u32,
}

async fn reorder_tracks(
    State(state): State<Arc<AppState>>,
    Form(req): Form<ReorderPlaylistParameters>,
) -> ResponseResult {
    let playlist = state.client.playlist(req.playlist_id).await;
    let playlist = ok_or_send_error_toast(&state, playlist)?;

    let Some(moved_output) = moved_index(&req.new_order) else {
        return Ok(([("HX-Reswap", "none")], "").into_response());
    };

    let moved_track = playlist
        .tracks
        .get(moved_output.moved_index)
        .ok_or(PlayerError::PlaylistReorderError);
    let moved_track = ok_or_send_error_toast(&state, moved_track)?;

    let moved_track_playlist_id = moved_track
        .playlist_track_id
        .ok_or(PlayerError::PlaylistReorderError);
    let moved_track_playlist_id = ok_or_send_error_toast(&state, moved_track_playlist_id)?;

    let res = state
        .client
        .update_playlist_track_position(
            moved_output.insert_before,
            req.playlist_id,
            moved_track_playlist_id,
        )
        .await;

    let res = ok_or_send_error_toast(&state, res)?;

    Ok(state.render("playlist-edit-tracks.html", &json!({"playlist": res,})))
}

async fn add_track_to_playlist_page(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u32>,
) -> ResponseResult {
    let track = state.client.track(id).await;
    let track = ok_or_send_error_toast(&state, track)?;

    let playlists = state.get_favorites().await;
    let playlists = ok_or_send_error_toast(&state, playlists)?;
    let playlists: Vec<_> = playlists
        .playlists
        .into_iter()
        .filter(|x| x.is_owned)
        .collect();

    Ok(state.render(
        "add-track-to-playlist.html",
        &json!({"track": track, "playlists": playlists}),
    ))
}

async fn create(State(state): State<Arc<AppState>>) -> ResponseResult {
    Ok(state.render("create-playlist.html", &json!({})))
}

#[derive(Deserialize)]
struct CreatePlaylist {
    name: String,
    description: String,
    is_public: Option<bool>,
    is_collaborative: Option<bool>,
}

async fn create_form(
    State(state): State<Arc<AppState>>,
    Form(req): Form<CreatePlaylist>,
) -> ResponseResult {
    let is_public = req.is_public.unwrap_or(false);

    let res = state
        .client
        .create_playlist(req.name, is_public, req.description, req.is_collaborative)
        .await;
    let res = ok_or_send_error_toast(&state, res)?;

    Ok(hx_redirect(&format!("/playlist/{}", res.id)))
}

async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> ResponseResult {
    let res = state.client.delete_playlist(id).await;
    ok_or_send_error_toast(&state, res)?;

    Ok(hx_redirect("/favorites/playlists"))
}

async fn play_track(
    State(state): State<Arc<AppState>>,
    Path((id, track_position)): Path<(u32, usize)>,
) -> impl IntoResponse {
    state.controls.play_playlist(id, track_position, false);
}

async fn play(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> impl IntoResponse {
    state.controls.play_playlist(id, 0, false);
}

async fn link(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> impl IntoResponse {
    let Some(rfid_state) = state.rfid_state.clone() else {
        return;
    };
    rfid_module::link(
        rfid_state,
        ReferenceType::Playlist(id),
        state.broadcast.clone(),
    )
    .await;
}

async fn shuffle(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> impl IntoResponse {
    state.controls.play_playlist(id, 0, true);
}

async fn set_favorite(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> ResponseResult {
    ok_or_send_error_toast(&state, state.client.add_favorite_playlist(id).await)?;

    Ok(state.render(
        "toggle-favorite.html",
        &json!({"api": "/playlist", "id": id, "is_favorite": true}),
    ))
}

async fn unset_favorite(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> ResponseResult {
    ok_or_send_error_toast(&state, state.client.remove_favorite_playlist(id).await)?;

    Ok(state.render(
        "toggle-favorite.html",
        &json!({"api": "/playlist", "id": id, "is_favorite": false}),
    ))
}

async fn index(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> impl IntoResponse {
    let url = format!("/playlist/{id}/content");
    state.render("lazy-load-component.html", &json!({"url": url}))
}

async fn content(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> ResponseResult {
    let playlist = ok_or_send_error_toast(&state, state.client.playlist(id).await)?;
    let favorites = ok_or_send_error_toast(&state, state.get_favorites().await)?;
    let is_favorite = favorites.playlists.iter().any(|playlist| playlist.id == id);
    let duration = playlist.duration_seconds / 60;
    let click_string = format!("/playlist/{}/play/", playlist.id);

    let playing_entity = &state.tracklist_receiver.borrow().current_playing_entity();
    let playing_index = index_if_playlist(playing_entity.as_ref(), id);

    Ok(state.render(
        "playlist.html",
        &json!({
            "playlist": playlist,
            "duration": duration,
            "is_favorite": is_favorite,
            "rfid": state.rfid_state.is_some(),
            "click": click_string,
            "use_playing_index": playing_index.is_some(),
            "playing_index": playing_index
        }),
    ))
}

async fn tracks_partial(State(state): State<Arc<AppState>>, Path(id): Path<u32>) -> ResponseResult {
    let playlist = ok_or_send_error_toast(&state, state.client.playlist(id).await)?;
    let click_string = format!("/playlist/{}/play/", playlist.id);

    let playing_entity = &state.tracklist_receiver.borrow().current_playing_entity();
    let playing_index = index_if_playlist(playing_entity.as_ref(), id);

    Ok(state.render(
        "playlist-tracks.html",
        &json!({
            "playlist": playlist,
            "click": click_string,
            "use_playing_index": playing_index.is_some(),
            "playing_index": playing_index
        }),
    ))
}

fn index_if_playlist(playing_entity: Option<&PlayingEntity>, playlist_id: u32) -> Option<usize> {
    playing_entity.as_ref().and_then(|x| match x {
        PlayingEntity::Playlist(playing_playlist) => {
            if playing_playlist.playlist_id == playlist_id {
                Some(playing_playlist.index)
            } else {
                None
            }
        }
        PlayingEntity::Track(_) => None,
    })
}

async fn edit_tracks_partial(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u32>,
) -> ResponseResult {
    let playlist = ok_or_send_error_toast(&state, state.client.playlist(id).await)?;

    Ok(state.render(
        "playlist-edit-tracks.html",
        &json!({
            "playlist": playlist,
        }),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MovedIndexOutput {
    moved_index: usize,
    insert_before: usize,
}

fn moved_index(perm: &[usize]) -> Option<MovedIndexOutput> {
    let mut moved: Option<(usize, usize)> = None;

    for (new_pos, old_idx) in perm.iter().copied().enumerate() {
        if new_pos == old_idx {
            continue;
        }

        let displacement = new_pos.abs_diff(old_idx);
        let moved_right = new_pos > old_idx;

        let better = moved.is_none_or(|(current_pos, current_idx)| {
            let current_displacement = current_pos.abs_diff(current_idx);
            let current_moved_right = current_pos > current_idx;

            displacement > current_displacement
                || (displacement == current_displacement && moved_right && !current_moved_right)
        });

        if better {
            moved = Some((new_pos, old_idx));
        }
    }

    let (new_pos, moved_index) = moved?;

    let insert_before = new_pos
        .checked_add(1)
        .and_then(|position| perm.get(position))
        .copied()
        .unwrap_or(perm.len());

    Some(MovedIndexOutput {
        moved_index,
        insert_before,
    })
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_1() {
        let perm = &[0, 1, 2, 3, 4, 5];
        let output = moved_index(perm);

        assert_eq!(output, None);
    }

    #[test]
    fn test_2() {
        let perm = &[1, 2, 3, 4, 5, 0];
        let output = moved_index(perm).unwrap();

        assert_eq!(output.moved_index, 0);
        assert_eq!(output.insert_before, 6);
    }

    #[test]
    fn test_3() {
        let perm = &[2, 0, 1, 3, 4, 5];
        let output = moved_index(perm).unwrap();

        assert_eq!(output.moved_index, 2);
        assert_eq!(output.insert_before, 0);
    }

    #[test]
    fn test_4() {
        let perm = &[0, 1, 2, 4, 3, 5];
        let output = moved_index(perm).unwrap();

        assert_eq!(output.moved_index, 3);
        assert_eq!(output.insert_before, 5);
    }
}
