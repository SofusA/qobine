use async_stream::stream;
use axum::{
    Json, Router,
    body::Body,
    extract::{Query, Request, State},
    http::{StatusCode, Uri},
    middleware::{Next, from_fn_with_state},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures::{Stream, StreamExt};
use qobuz_player_controls::{Status, controls::ControlCommand, tracklist::Tracklist};
use qobuz_player_disconnect_server::{DisconnectServerEvent, DisconnectState};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, broadcast};
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};
use tower_http::limit::RequestBodyLimitLayer;
use tracing::info;

const MAX_ID_LEN: usize = 20;
const MAX_GROUP_COUNT: usize = 1_000;
const MAX_BODY_SIZE_BYTES: usize = 64 * 1024;

const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);
const RATE_LIMIT_MAX_REQUESTS: usize = 60;

const SSE_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct AppState {
    groups: Arc<RwLock<HashMap<String, Group>>>,
    rate_limits: Arc<RwLock<HashMap<String, VecDeque<Instant>>>>,
}

struct Group {
    streams: HashSet<String>,
    tx: broadcast::Sender<DisconnectServerEvent>,
    active_device: String,
    tracklist: Tracklist,
    playback_status: Status,
    position: Duration,
    volume: f32,
    auto_play: bool,
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
        groups: Arc::new(RwLock::new(HashMap::new())),
        rate_limits: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/stream", get(stream_handler))
        .route("/state", get(get_state))
        .route("/active-device", post(set_active_device))
        .route("/tracklist", post(set_tracklist))
        .route("/status", post(set_status))
        .route("/position", post(set_position))
        .route("/volume", post(set_volume))
        .route("/autoplay", post(set_auto_play))
        .route("/control", post(control))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE_BYTES))
        .layer(from_fn_with_state(state.clone(), rate_limit_middleware))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii())
        .take(MAX_ID_LEN)
        .collect()
}

fn sanitize_secret(value: &str) -> String {
    sanitize_id(value)
}

fn sanitize_device_id(value: &str) -> String {
    sanitize_id(value)
}

fn validate_non_empty(value: &str) -> Result<(), StatusCode> {
    if value.is_empty() {
        Err(StatusCode::BAD_REQUEST)
    } else {
        Ok(())
    }
}

fn sanitize_auth_query(auth: AuthQuery) -> Result<String, StatusCode> {
    let secret = sanitize_secret(&auth.secret);
    validate_non_empty(&secret)?;
    Ok(secret)
}

fn sanitize_stream_query(query: StreamQuery) -> Result<(String, String), StatusCode> {
    let secret = sanitize_secret(&query.secret);
    let device_id = sanitize_device_id(&query.device_id);

    validate_non_empty(&secret)?;
    validate_non_empty(&device_id)?;

    Ok((secret, device_id))
}

fn sanitize_device_request(req: DeviceRequest) -> Result<String, StatusCode> {
    let device_id = sanitize_device_id(&req.device_id);
    validate_non_empty(&device_id)?;
    Ok(device_id)
}

fn rate_limit_key_from_uri(uri: &Uri) -> String {
    if let Ok(Query(auth)) = Query::<AuthQuery>::try_from_uri(uri) {
        let secret = sanitize_secret(&auth.secret);

        if !secret.is_empty() {
            return format!("secret:{secret}");
        }
    }

    "global".to_string()
}

async fn rate_limit_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> impl IntoResponse {
    let key = rate_limit_key_from_uri(req.uri());
    let now = Instant::now();

    {
        let mut rate_limits = state.rate_limits.write().await;
        let timestamps = rate_limits.entry(key).or_default();

        while let Some(oldest) = timestamps.front() {
            if now.duration_since(*oldest) > RATE_LIMIT_WINDOW {
                timestamps.pop_front();
            } else {
                break;
            }
        }

        if timestamps.len() >= RATE_LIMIT_MAX_REQUESTS {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }

        timestamps.push_back(now);
    }

    next.run(req).await
}

async fn is_active_device(state: &AppState, secret: &str, device_id: &str) -> bool {
    let groups = state.groups.read().await;

    groups
        .get(secret)
        .map(|g| g.active_device == device_id)
        .unwrap_or(false)
}

async fn get_state(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
) -> Result<Json<DisconnectState>, StatusCode> {
    let secret = sanitize_auth_query(auth)?;

    let groups = state.groups.read().await;

    let group = groups.get(&secret).ok_or(StatusCode::NOT_FOUND)?;

    let state = DisconnectState {
        active_device: group.active_device.clone(),
        available_devices: group.streams.iter().cloned().collect(),
        playback_status: group.playback_status,
        tracklist: group.tracklist.clone(),
        position: group.position,
        volume: group.volume,
        auto_play: group.auto_play,
    };

    Ok(Json(state))
}

async fn control(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(req): Json<ControlCommand>,
) -> Result<StatusCode, StatusCode> {
    let secret = sanitize_auth_query(auth)?;
    let device_id = sanitize_device_request(device)?;

    if is_active_device(&state, &secret, &device_id).await {
        info!("control blocked. Active device cannot control over Disconnect");
        return Err(StatusCode::FORBIDDEN);
    }

    let groups = state.groups.read().await;

    let group = groups.get(&secret).ok_or(StatusCode::NOT_FOUND)?;

    info!("control: {:?}", req);

    let _ = group.tx.send(DisconnectServerEvent::Control(req));

    Ok(StatusCode::OK)
}

async fn set_active_device(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Json(req): Json<DeviceRequest>,
) -> Result<StatusCode, StatusCode> {
    let secret = sanitize_auth_query(auth)?;
    let device_id = sanitize_device_request(req)?;

    let mut groups = state.groups.write().await;

    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

    if !group.streams.contains(&device_id) {
        return Err(StatusCode::BAD_REQUEST);
    }

    if group.active_device == device_id {
        return Ok(StatusCode::OK);
    }

    info!("new active device {}", device_id);

    group.active_device = device_id.clone();

    let _ = group
        .tx
        .send(DisconnectServerEvent::ActiveDevice(device_id));

    Ok(StatusCode::OK)
}

async fn set_tracklist(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(req): Json<Tracklist>,
) -> Result<StatusCode, StatusCode> {
    info!("New set tracklist request");

    let secret = sanitize_auth_query(auth)?;
    let device_id = sanitize_device_request(device)?;

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;

    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

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

    let secret = sanitize_auth_query(auth)?;
    let device_id = sanitize_device_request(device)?;

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;

    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

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
    let secret = sanitize_auth_query(auth)?;
    let device_id = sanitize_device_request(device)?;

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;

    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

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

    let secret = sanitize_auth_query(auth)?;
    let device_id = sanitize_device_request(device)?;

    if !req.is_finite() || !(0.0..=1.0).contains(&req) {
        return Err(StatusCode::BAD_REQUEST);
    }

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;

    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

    group.volume = req;

    let _ = group.tx.send(DisconnectServerEvent::Volume(req));

    info!("Volume updated {:?}", req);

    Ok(StatusCode::OK)
}

async fn set_auto_play(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(req): Json<bool>,
) -> Result<StatusCode, StatusCode> {
    info!("New set autoplay request");

    let secret = sanitize_auth_query(auth)?;
    let device_id = sanitize_device_request(device)?;

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;

    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

    group.auto_play = req;

    let _ = group.tx.send(DisconnectServerEvent::AutoPlay(req));

    info!("auto_play updated {:?}", req);

    Ok(StatusCode::OK)
}

struct Guard {
    secret: String,
    groups: Arc<RwLock<HashMap<String, Group>>>,
    device: String,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let groups = self.groups.clone();
        let secret = self.secret.clone();
        let device = self.device.clone();

        tokio::spawn(async move {
            let mut groups = groups.write().await;

            if let Some(group) = groups.get_mut(&secret) {
                group.streams.remove(&device);

                tracing::info!("stream disconnected {}", device);

                if group.streams.is_empty() {
                    groups.remove(&secret);
                } else {
                    if group.active_device == device
                        && let Some(new_active) = group.streams.iter().next().cloned()
                    {
                        group.active_device = new_active.clone();

                        let _ = group
                            .tx
                            .send(DisconnectServerEvent::ActiveDevice(new_active));
                    }

                    let devices: Vec<String> = group.streams.iter().cloned().collect();

                    let _ = group
                        .tx
                        .send(DisconnectServerEvent::AvailableDevices(devices));
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
    let (secret, device_id) = sanitize_stream_query(query)?;

    let rx = {
        let mut groups = state.groups.write().await;

        if !groups.contains_key(&secret) && groups.len() >= MAX_GROUP_COUNT {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }

        let group = groups.entry(secret.clone()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(128);

            let streams = HashSet::new();

            Group {
                streams,
                tx,
                active_device: device_id.clone(),
                tracklist: Default::default(),
                playback_status: Default::default(),
                position: Default::default(),
                volume: 1.0,
                auto_play: false,
            }
        });

        if group.streams.contains(&device_id) {
            return Err(StatusCode::BAD_REQUEST);
        }

        group.streams.insert(device_id.clone());

        if group.active_device.is_empty() {
            group.active_device = device_id.clone();
        }

        let rx = group.tx.subscribe();

        let devices: Vec<String> = group.streams.iter().cloned().collect();

        let _ = group
            .tx
            .send(DisconnectServerEvent::AvailableDevices(devices));

        rx
    };

    let guard = Guard {
        secret: secret.clone(),
        groups: state.groups.clone(),
        device: device_id.clone(),
    };

    let s = stream! {
        let _guard = guard;
        let mut rx = BroadcastStream::new(rx);

        while let Some(msg) = rx.next().await {
            if let Some(event) =
                map_event(&state, &secret, &device_id, msg).await
            {
                yield event;
            }
        }
    };

    Ok(Sse::new(s).keep_alive(
        KeepAlive::new()
            .interval(SSE_KEEPALIVE_INTERVAL)
            .text("keepalive"),
    ))
}

async fn map_event(
    state: &AppState,
    secret: &str,
    device: &str,
    msg: Result<DisconnectServerEvent, BroadcastStreamRecvError>,
) -> Option<Result<Event, Infallible>> {
    let change = match msg {
        Ok(change) => change,
        Err(_) => return None,
    };

    let is_active_device = {
        let groups = state.groups.read().await;

        groups
            .get(secret)
            .map(|g| g.active_device == device)
            .unwrap_or(false)
    };

    let should_send = match &change {
        DisconnectServerEvent::Control(_) => is_active_device,

        DisconnectServerEvent::Tracklist(_)
        | DisconnectServerEvent::Status(_)
        | DisconnectServerEvent::Position(_)
        | DisconnectServerEvent::AutoPlay(_)
        | DisconnectServerEvent::Volume(_) => !is_active_device,

        DisconnectServerEvent::ActiveDevice(_) | DisconnectServerEvent::AvailableDevices(_) => true,
    };

    if !should_send {
        return None;
    }

    let json = serde_json::to_string(&change).unwrap();

    Some(Ok(Event::default().data(json)))
}
