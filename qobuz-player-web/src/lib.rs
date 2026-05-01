use assets::static_handler;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response, Sse, sse::Event},
    routing::get,
};
use futures::stream::Stream;
use qobuz_player_controls::{
    AppResult, PositionReceiver, Status, StatusReceiver, TracklistReceiver, VolumeReceiver,
    client::Client,
    controls::Controls,
    database::Database,
    error::Error,
    models::{Album, AlbumSimple, Playlist},
    notification::{Notification, NotificationBroadcast},
};
use qobuz_player_rfid::RfidState;
use serde_json::json;
use skabelon::Templates;
use std::{convert::Infallible, env, path::PathBuf, sync::Arc};
use tokio::sync::{
    broadcast::{self, Receiver, Sender},
    watch,
};
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    app_state::AppState,
    routes::{
        album, api, artist, auth, controls, discover, favorites, now_playing, playlist, queue,
        search,
    },
    views::templates,
};

mod app_state;
mod assets;
mod routes;
mod views;

#[allow(clippy::too_many_arguments)]
pub async fn init(
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    volume_receiver: VolumeReceiver,
    status_receiver: StatusReceiver,
    port: u16,
    web_secret: Option<String>,
    rfid_state: Option<RfidState>,
    broadcast: Arc<NotificationBroadcast>,
    client: Arc<Client>,
    database: Arc<Database>,
) -> AppResult<()> {
    let interface = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&interface)
        .await
        .or(Err(Error::PortInUse { port }))?;

    let router = create_router(
        controls,
        position_receiver,
        tracklist_receiver,
        volume_receiver,
        status_receiver,
        web_secret,
        rfid_state,
        broadcast,
        client,
        database,
    )
    .await;

    axum::serve(listener, router).await.expect("infallible");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create_router(
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    volume_receiver: VolumeReceiver,
    status_receiver: StatusReceiver,
    web_secret: Option<String>,
    rfid_state: Option<RfidState>,
    broadcast: Arc<NotificationBroadcast>,
    client: Arc<Client>,
    database: Arc<Database>,
) -> Router {
    let (tx, _rx) = broadcast::channel::<ServerSentEvent>(100);
    let broadcast_subscribe = broadcast.subscribe();

    let template_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");

    let templates = templates(&template_path);

    #[allow(unused_variables)]
    let (templates_tx, templates_rx) = watch::channel(templates);

    #[cfg(all(debug_assertions, target_os = "linux"))]
    {
        let templates_clone = templates_rx.clone();
        let watcher_sender = tx.clone();
        let watcher = filesentry::Watcher::new().unwrap();
        watcher.add_root(&template_path, true, |_| ()).unwrap();

        watcher.add_handler(move |events| {
            for event in &*events {
                match event.ty {
                    filesentry::EventType::Modified | filesentry::EventType::Create => {
                        let mut templates = templates_clone.borrow().clone();
                        templates.reload();
                        templates_tx.send(templates).unwrap();

                        let event = ServerSentEvent {
                            event_name: "reload".into(),
                            event_data: "template changed".into(),
                        };

                        _ = watcher_sender.send(event);
                    }
                    _ => (),
                }
            }
            true
        });
        watcher.start();
    }

    let shared_state = Arc::new(AppState {
        controls,
        web_secret,
        rfid_state,
        broadcast,
        client,
        tx: tx.clone(),
        position_receiver: position_receiver.clone(),
        tracklist_receiver: tracklist_receiver.clone(),
        volume_receiver: volume_receiver.clone(),
        status_receiver: status_receiver.clone(),
        templates: templates_rx.clone(),
        database,
    });

    tokio::spawn(background_task(
        tx,
        broadcast_subscribe,
        position_receiver,
        tracklist_receiver,
        volume_receiver,
        status_receiver,
        templates_rx,
    ));

    axum::Router::new()
        .route("/sse", get(sse_handler))
        .merge(now_playing::routes())
        .merge(queue::routes())
        .merge(api::routes())
        .merge(search::routes())
        .merge(album::routes())
        .merge(artist::routes())
        .merge(playlist::routes())
        .merge(favorites::routes())
        .merge(discover::routes())
        .merge(controls::routes())
        .layer(axum::middleware::from_fn_with_state(
            shared_state.clone(),
            auth::auth_middleware,
        ))
        .route("/assets/{*file}", get(static_handler))
        .merge(auth::routes())
        .with_state(shared_state.clone())
}

async fn background_task(
    tx: Sender<ServerSentEvent>,
    mut receiver: Receiver<Notification>,
    mut position: PositionReceiver,
    mut tracklist: TracklistReceiver,
    mut volume: VolumeReceiver,
    mut status: StatusReceiver,
    templates: watch::Receiver<Templates>,
) {
    loop {
        tokio::select! {
            Ok(_) = position.changed() => {
                let position_duration = position.borrow_and_update();
                let event = ServerSentEvent {
                    event_name: "position".into(),
                    event_data: position_duration.as_millis().to_string(),
                };

                _ = tx.send(event);
            },
            Ok(_) = tracklist.changed() => {
                _ = tracklist.borrow_and_update();
                let event = ServerSentEvent {
                    event_name: "tracklist".into(),
                    event_data: "new tracklist".into(),
                };
                _ = tx.send(event);
            },
            Ok(_) = volume.changed() => {
                let volume = *volume.borrow_and_update();
                let volume = (volume * 100.0) as u32;
                let event = ServerSentEvent {
                    event_name: "volume".into(),
                    event_data: volume.to_string(),
                };
                _ = tx.send(event);
            }
            Ok(_) = status.changed() => {
                let status = status.borrow_and_update();
                let message_data = match *status {
                    Status::Paused => "pause",
                    Status::Playing => "play",
                    Status::Buffering => "buffering",
                    Status::Stopped => "stopped",
                };

                let event = ServerSentEvent {
                    event_name: "status".into(),
                    event_data: message_data.into(),
                };
                _ = tx.send(event);
            }
            notification = receiver.recv() => {
                tracing::info!("notification: {:?}", notification);
                if let Ok(message) = notification {
                        let (message_string, severity) = match &message {
                            Notification::Error(message) => (message, 1),
                            Notification::Warning(message) => (message, 2),
                            Notification::Success(message) => (message, 3),
                            Notification::Info(message) => (message, 4),
                        };

                    let toast = templates.borrow().render("toast.html", &json!({"message": message_string, "severity": severity}));
                    let event = match message {
                        Notification::Error(_) => ServerSentEvent {
                            event_name: "error".into(),
                            event_data: toast,
                        },
                        Notification::Warning(_) => {
                            ServerSentEvent {
                                event_name: "warn".into(),
                                event_data: toast,
                            }
                        }
                        Notification::Success(_) => {
                            ServerSentEvent {
                                event_name: "success".into(),
                                event_data: toast,
                            }
                        }
                        Notification::Info(_) => ServerSentEvent {
                            event_name: "info".into(),
                            event_data: toast,
                        },
                    };
                    _ = tx.send(event);
                }
            }
        }
    }
}

async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> (
    axum::http::HeaderMap,
    Sse<impl Stream<Item = AppResult<Event, Infallible>>>,
) {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => Some(Ok(Event::default()
            .event(event.event_name)
            .data(event.event_data))),
        Err(_) => None,
    });

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("X-Accel-Buffering", "no".parse().expect("infallible"));

    (headers, Sse::new(stream))
}

#[derive(Clone)]
pub struct AlbumData {
    pub album: Album,
    pub suggested_albums: Vec<AlbumSimple>,
}

#[derive(Clone)]
pub struct ServerSentEvent {
    event_name: String,
    event_data: String,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct Discover {
    pub albums: Vec<(String, Vec<AlbumSimple>)>,
    pub playlists: Vec<(String, Vec<Playlist>)>,
}

type ResponseResult = std::result::Result<axum::response::Response, axum::response::Response>;

#[allow(clippy::result_large_err)]
fn ok_or_send_error_toast<T>(
    state: &AppState,
    value: AppResult<T, qobuz_player_controls::error::Error>,
) -> AppResult<T, axum::response::Response> {
    match value {
        Ok(value) => Ok(value),
        Err(err) => Err(state.send_toast(Notification::Error(err.to_string()))),
    }
}

#[allow(clippy::result_large_err)]
fn ok_or_error_page<T>(
    state: &AppState,
    value: AppResult<T, qobuz_player_controls::error::Error>,
) -> AppResult<T, axum::response::Response> {
    match value {
        Ok(value) => Ok(value),
        Err(err) => Err(Html(
            state
                .templates
                .borrow()
                .render("error-page.html", &json!({"error": err.to_string()})),
        )
        .into_response()),
    }
}

#[allow(clippy::result_large_err, unused)]
fn ok_or_broadcast<T>(
    broadcast: &NotificationBroadcast,
    value: AppResult<T, qobuz_player_controls::error::Error>,
) -> AppResult<T, axum::response::Response> {
    match value {
        Ok(value) => Ok(value),
        Err(err) => {
            broadcast.send(Notification::Error(err.to_string()));

            let mut response = Html("<div></div>".to_string()).into_response();
            let headers = response.headers_mut();
            headers.insert("HX-Reswap", "none".try_into().expect("infallible"));

            Err(response)
        }
    }
}

pub fn hx_redirect(url: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("HX-Redirect", url.parse().unwrap());
    (StatusCode::OK, headers).into_response()
}
