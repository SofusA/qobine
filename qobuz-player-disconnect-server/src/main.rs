use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    routing::{get, post},
};
use futures::{Stream, StreamExt};
use qobuz_player_controls::{Status, controls::ControlCommand, tracklist::Tracklist};
use qobuz_player_disconnect_server::{DisconnectServerEvent, DisconnectState};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, broadcast};
use tokio_stream::wrappers::BroadcastStream;
use tracing::info;

#[derive(Clone)]
struct AppState {
    groups: Arc<Mutex<HashMap<String, Group>>>,
}

struct Group {
    streams: HashSet<String>,
    tx: broadcast::Sender<DisconnectServerEvent>,
    current_device: Option<String>,
    tracklist: Tracklist,
    playback_status: Status,
    position: Duration,
    volume: f32,
}

#[derive(Deserialize)]
struct AuthQuery {
    secret: String,
}

#[derive(Deserialize)]
struct StreamQuery {
    secret: String,
    device_id: String,
}

#[derive(Deserialize)]
struct DeviceRequest {
    device_id: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let state = AppState {
        groups: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/stream", get(stream_handler))
        .route("/state", get(get_state))
        .route("/current-device", post(set_current_device))
        .route("/tracklist", post(set_tracklist))
        .route("/status", post(set_status))
        .route("/position", post(set_position))
        .route("/volume", post(set_volume))
        .route("/control", post(control))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn ensure_group<'a>(
    state: &'a AppState,
    secret: &str,
) -> tokio::sync::MutexGuard<'a, HashMap<String, Group>> {
    let mut groups = state.groups.lock().await;

    if !groups.contains_key(secret) {
        let (tx, _) = broadcast::channel(128);

        groups.insert(
            secret.to_string(),
            Group {
                streams: Default::default(),
                tx,
                current_device: None,
                tracklist: Default::default(),
                playback_status: Default::default(),
                position: Default::default(),
                volume: 1.0,
            },
        );
    }

    groups
}

async fn is_active_device(state: &AppState, secret: &str, device_id: &str) -> bool {
    let groups = state.groups.lock().await;

    groups
        .get(secret)
        .and_then(|g| g.current_device.as_ref())
        .map(|d| d == device_id)
        .unwrap_or(false)
}

async fn get_state(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
) -> Result<Json<DisconnectState>, StatusCode> {
    let groups = ensure_group(&state, &auth.secret).await;
    let group = groups.get(&auth.secret).unwrap();

    let state = DisconnectState {
        selected_device: group.current_device.clone(),
        available_devices: group.streams.iter().cloned().collect(),
        playback_status: group.playback_status,
        tracklist: group.tracklist.clone(),
        position: group.position,
        volume: group.volume,
    };

    Ok(Json(state))
}

async fn control(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(req): Json<ControlCommand>,
) -> Result<StatusCode, StatusCode> {
    if is_active_device(&state, &auth.secret, &device.device_id).await {
        info!("control blocked. Active device cannot control over Disconnect");
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = ensure_group(&state, &auth.secret).await;
    let group = groups.get_mut(&auth.secret).unwrap();
    info!("control: {:?}", req);

    let _ = group.tx.send(DisconnectServerEvent::Control(req));

    Ok(StatusCode::OK)
}

async fn set_current_device(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Json(req): Json<DeviceRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut groups = ensure_group(&state, &auth.secret).await;
    let group = groups.get_mut(&auth.secret).unwrap();

    info!("new device {}", req.device_id);

    group.current_device = Some(req.device_id.clone());

    let _ = group
        .tx
        .send(DisconnectServerEvent::ActiveDevice(req.device_id));

    Ok(StatusCode::OK)
}

async fn set_tracklist(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(req): Json<Tracklist>,
) -> Result<StatusCode, StatusCode> {
    info!("New set tracklist request");
    if !is_active_device(&state, &auth.secret, &device.device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = ensure_group(&state, &auth.secret).await;
    let group = groups.get_mut(&auth.secret).unwrap();

    group.tracklist = req.clone();
    info!("tracklist {:?}", req);

    let _ = group.tx.send(DisconnectServerEvent::Tracklist(req));

    Ok(StatusCode::OK)
}

async fn set_status(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(req): Json<Status>,
) -> Result<StatusCode, StatusCode> {
    info!("New set status request");
    if !is_active_device(&state, &auth.secret, &device.device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut groups = ensure_group(&state, &auth.secret).await;
    let group = groups.get_mut(&auth.secret).unwrap();

    group.playback_status = req;

    let _ = group.tx.send(DisconnectServerEvent::Status(req));

    info!("Status updated {:?}", req);

    Ok(StatusCode::OK)
}

async fn set_position(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(req): Json<Duration>,
) -> Result<StatusCode, StatusCode> {
    if !is_active_device(&state, &auth.secret, &device.device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut groups = ensure_group(&state, &auth.secret).await;
    let group = groups.get_mut(&auth.secret).unwrap();

    group.position = req;

    let _ = group.tx.send(DisconnectServerEvent::Position(req));

    info!("Position updated {:?}", req);

    Ok(StatusCode::OK)
}

async fn set_volume(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(req): Json<f32>,
) -> Result<StatusCode, StatusCode> {
    info!("New set volume request");
    if !is_active_device(&state, &auth.secret, &device.device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut groups = ensure_group(&state, &auth.secret).await;
    let group = groups.get_mut(&auth.secret).unwrap();

    group.volume = req;

    let _ = group.tx.send(DisconnectServerEvent::Volume(req));

    info!("Position updated {:?}", req);

    Ok(StatusCode::OK)
}

struct Guard {
    secret: String,
    groups: Arc<Mutex<HashMap<String, Group>>>,
    device: String,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let groups = self.groups.clone();
        let secret = self.secret.clone();
        let device = self.device.clone();

        tokio::spawn(async move {
            let mut groups = groups.lock().await;

            if let Some(group) = groups.get_mut(&secret) {
                group.streams.remove(&device);

                if group.streams.is_empty() {
                    groups.remove(&secret);
                }
            }

            tracing::info!("stream disconnected {}", device);
        });
    }
}

async fn stream_handler(
    State(state): State<AppState>,
    Query(query): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let rx = {
        let mut groups = ensure_group(&state, &query.secret).await;
        let group = groups.get_mut(&query.secret).unwrap();

        group.streams.insert(query.device_id.clone());
        group.tx.subscribe()
    };

    let secret = query.secret;
    let device = query.device_id;

    let guard = Guard {
        secret: secret.clone(),
        groups: state.groups.clone(),
        device: device.clone(),
    };

    let s = stream! {
        let _guard = guard;
        let mut rx = BroadcastStream::new(rx);

        while let Some(msg) = rx.next().await {
            if let Ok(change) = msg {
                let is_active_device = {
                    let groups = state.groups.lock().await;

                    groups
                        .get(&secret)
                        .and_then(|g| g.current_device.as_ref())
                        .map(|d| d == &device)
                        .unwrap_or(false)
                };

                let should_send = match &change {
                    // only active device receives controls
                    DisconnectServerEvent::Control(_) => {
                        is_active_device
                    }

                    // active device should NOT receive these
                    DisconnectServerEvent::Tracklist(_)
                    | DisconnectServerEvent::Status(_)
                    | DisconnectServerEvent::Position(_)
                    | DisconnectServerEvent::Volume(_) => {
                        !is_active_device
                    }

                    // everyone receives active device updates
                    DisconnectServerEvent::ActiveDevice(_) => true,
                };

                if !should_send {
                    continue;
                }

                let json = serde_json::to_string(&change).unwrap();

                yield Ok(Event::default().data(json));
            }
        }
    };

    Ok(Sse::new(s))
}
