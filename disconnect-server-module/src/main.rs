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
use controls_module::{Status, controls::ControlCommand, tracklist::Tracklist};
use disconnect_server_module::{DisconnectServerEvent, DisconnectState};
use futures::{Stream, StreamExt};
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
    listeners: HashSet<String>,
    tx: broadcast::Sender<DisconnectServerEvent>,
    active_device: String,
    tracklist: Tracklist,
    playback_status: Status,
    position: Duration,
    volume: f32,
    auto_play: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum StreamType {
    #[default]
    Device,
    Listener,
}

#[derive(Deserialize)]
struct AuthQuery {
    secret: String,
}

#[derive(Deserialize)]
struct StreamQuery {
    secret: String,
    device_id: String,

    #[serde(default)]
    stream_type: StreamType,
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

    tracing::info!("listening on {}", addr);

    let Ok(listener) = tokio::net::TcpListener::bind(addr).await else {
        tracing::error!("Unable to bind to address: {addr}");
        return;
    };

    if let Err(error) = axum::serve(listener, app).await {
        tracing::error!(?error, "server stopped");
    }
}

fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii)
        .take(MAX_ID_LEN)
        .collect()
}

fn sanitize_secret(value: &str) -> String {
    sanitize_id(value)
}

fn sanitize_device_id(value: &str) -> String {
    sanitize_id(value)
}

const fn validate_non_empty(value: &str) -> Result<(), StatusCode> {
    if value.is_empty() {
        Err(StatusCode::BAD_REQUEST)
    } else {
        Ok(())
    }
}

fn sanitize_auth_query(auth: &AuthQuery) -> Result<String, StatusCode> {
    let secret = sanitize_secret(&auth.secret);
    validate_non_empty(&secret)?;
    Ok(secret)
}

fn sanitize_stream_query(query: &StreamQuery) -> Result<(String, String, StreamType), StatusCode> {
    let secret = sanitize_secret(&query.secret);
    let client_id = sanitize_device_id(&query.device_id);

    validate_non_empty(&secret)?;
    validate_non_empty(&client_id)?;

    Ok((secret, client_id, query.stream_type))
}

fn sanitize_device_request(request: &DeviceRequest) -> Result<String, StatusCode> {
    let device_id = sanitize_device_id(&request.device_id);

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
    request: Request<Body>,
    next: Next,
) -> impl IntoResponse {
    let key = rate_limit_key_from_uri(request.uri());
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

    next.run(request).await
}

async fn is_active_device(state: &AppState, secret: &str, device_id: &str) -> bool {
    let groups = state.groups.read().await;

    groups
        .get(secret)
        .is_some_and(|group| group.active_device == device_id)
}

async fn get_state(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
) -> Result<Json<DisconnectState>, StatusCode> {
    let secret = sanitize_auth_query(&auth)?;

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
    Query(client): Query<DeviceRequest>,
    Json(command): Json<ControlCommand>,
) -> Result<StatusCode, StatusCode> {
    let secret = sanitize_auth_query(&auth)?;
    let client_id = sanitize_device_request(&client)?;

    let groups = state.groups.read().await;
    let group = groups.get(&secret).ok_or(StatusCode::NOT_FOUND)?;

    let is_listener = group.listeners.contains(&client_id);
    let is_inactive_device = group.streams.contains(&client_id) && group.active_device != client_id;

    if !is_listener && !is_inactive_device {
        tracing::info!(
            client_id = %client_id,
            "control request rejected"
        );

        return Err(StatusCode::FORBIDDEN);
    }

    tracing::info!(
        client_id = %client_id,
        "control: {:?}",
        command
    );

    let _ = group.tx.send(DisconnectServerEvent::Control(command));

    Ok(StatusCode::OK)
}

async fn set_active_device(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Json(request): Json<DeviceRequest>,
) -> Result<StatusCode, StatusCode> {
    let secret = sanitize_auth_query(&auth)?;
    let device_id = sanitize_device_request(&request)?;

    let mut groups = state.groups.write().await;
    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

    if !group.streams.contains(&device_id) {
        return Err(StatusCode::BAD_REQUEST);
    }

    if group.active_device == device_id {
        return Ok(StatusCode::OK);
    }

    tracing::info!("new active device: {}", device_id);

    group.active_device.clone_from(&device_id);

    let _ = group
        .tx
        .send(DisconnectServerEvent::ActiveDevice(device_id));

    Ok(StatusCode::OK)
}

async fn set_tracklist(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(tracklist): Json<Tracklist>,
) -> Result<StatusCode, StatusCode> {
    let secret = sanitize_auth_query(&auth)?;
    let device_id = sanitize_device_request(&device)?;

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;
    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

    group.tracklist = tracklist.clone();

    tracing::info!(
        device_id = %device_id,
        "tracklist updated: {:?}",
        tracklist
    );

    let _ = group.tx.send(DisconnectServerEvent::Tracklist(tracklist));

    Ok(StatusCode::OK)
}

async fn set_status(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(status): Json<Status>,
) -> Result<StatusCode, StatusCode> {
    let secret = sanitize_auth_query(&auth)?;
    let device_id = sanitize_device_request(&device)?;

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;
    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

    group.playback_status = status;

    let _ = group.tx.send(DisconnectServerEvent::Status(status));

    tracing::info!(
        device_id = %device_id,
        "status updated: {:?}",
        status
    );

    Ok(StatusCode::OK)
}

async fn set_position(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(position): Json<Duration>,
) -> Result<StatusCode, StatusCode> {
    let secret = sanitize_auth_query(&auth)?;
    let device_id = sanitize_device_request(&device)?;

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;
    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

    group.position = position;

    let _ = group.tx.send(DisconnectServerEvent::Position(position));

    tracing::info!(
        device_id = %device_id,
        "position updated: {:?}",
        position
    );

    Ok(StatusCode::OK)
}

async fn set_volume(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(volume): Json<f32>,
) -> Result<StatusCode, StatusCode> {
    let secret = sanitize_auth_query(&auth)?;
    let device_id = sanitize_device_request(&device)?;

    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err(StatusCode::BAD_REQUEST);
    }

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;
    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

    group.volume = volume;

    let _ = group.tx.send(DisconnectServerEvent::Volume(volume));

    tracing::info!(
        device_id = %device_id,
        "volume updated: {}",
        volume
    );

    Ok(StatusCode::OK)
}

async fn set_auto_play(
    State(state): State<AppState>,
    Query(auth): Query<AuthQuery>,
    Query(device): Query<DeviceRequest>,
    Json(auto_play): Json<bool>,
) -> Result<StatusCode, StatusCode> {
    let secret = sanitize_auth_query(&auth)?;
    let device_id = sanitize_device_request(&device)?;

    if !is_active_device(&state, &secret, &device_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut groups = state.groups.write().await;
    let group = groups.get_mut(&secret).ok_or(StatusCode::NOT_FOUND)?;

    group.auto_play = auto_play;

    let _ = group.tx.send(DisconnectServerEvent::AutoPlay(auto_play));

    tracing::info!(
        device_id = %device_id,
        "autoplay updated: {}",
        auto_play
    );

    Ok(StatusCode::OK)
}

struct Guard {
    secret: String,
    groups: Arc<RwLock<HashMap<String, Group>>>,
    client_id: String,
    stream_type: StreamType,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let groups = self.groups.clone();
        let secret = self.secret.clone();
        let client_id = self.client_id.clone();
        let stream_type = self.stream_type;

        tokio::spawn(async move {
            let mut groups = groups.write().await;

            let should_remove_group = {
                let Some(group) = groups.get_mut(&secret) else {
                    return;
                };

                match stream_type {
                    StreamType::Device => {
                        group.streams.remove(&client_id);

                        tracing::info!(
                            device_id = %client_id,
                            "device stream disconnected"
                        );

                        if group.active_device == client_id {
                            if let Some(new_active) = group.streams.iter().next().cloned() {
                                group.active_device.clone_from(&new_active);

                                let _ = group
                                    .tx
                                    .send(DisconnectServerEvent::ActiveDevice(new_active));
                            } else {
                                group.active_device.clear();
                            }
                        }

                        let available_devices: Vec<String> =
                            group.streams.iter().cloned().collect();

                        let _ = group
                            .tx
                            .send(DisconnectServerEvent::AvailableDevices(available_devices));
                    }

                    StreamType::Listener => {
                        group.listeners.remove(&client_id);

                        tracing::info!(
                            listener_id = %client_id,
                            "listener disconnected"
                        );
                    }
                }

                group.streams.is_empty() && group.listeners.is_empty()
            };

            if should_remove_group {
                groups.remove(&secret);

                tracing::info!(
                    secret = %secret,
                    "removed empty group"
                );
            }
        });
    }
}

async fn stream_handler(
    State(state): State<AppState>,
    Query(query): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let (secret, client_id, stream_type) = sanitize_stream_query(&query)?;

    let rx = {
        let mut groups = state.groups.write().await;

        if !groups.contains_key(&secret) && groups.len() >= MAX_GROUP_COUNT {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }

        let group = groups.entry(secret.clone()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(128);

            Group {
                streams: HashSet::new(),
                listeners: HashSet::new(),
                tx,
                active_device: String::new(),
                tracklist: Tracklist::default(),
                playback_status: Status::default(),
                position: Duration::default(),
                volume: 1.0,
                auto_play: false,
            }
        });

        if group.streams.contains(&client_id) || group.listeners.contains(&client_id) {
            return Err(StatusCode::CONFLICT);
        }

        let rx = group.tx.subscribe();

        match stream_type {
            StreamType::Device => {
                group.streams.insert(client_id.clone());

                if group.active_device.is_empty() {
                    group.active_device.clone_from(&client_id);

                    let _ = group
                        .tx
                        .send(DisconnectServerEvent::ActiveDevice(client_id.clone()));
                }

                let available_devices: Vec<String> = group.streams.iter().cloned().collect();

                let _ = group
                    .tx
                    .send(DisconnectServerEvent::AvailableDevices(available_devices));

                tracing::info!(
                    device_id = %client_id,
                    "device stream connected"
                );
            }

            StreamType::Listener => {
                group.listeners.insert(client_id.clone());

                tracing::info!(
                    listener_id = %client_id,
                    "listener connected"
                );
            }
        }

        rx
    };

    let guard = Guard {
        secret: secret.clone(),
        groups: state.groups.clone(),
        client_id: client_id.clone(),
        stream_type,
    };

    let event_stream = stream! {
        let _guard = guard;
        let mut rx = BroadcastStream::new(rx);

        while let Some(message) = rx.next().await {
            match message {
                Ok(change) => {
                    if let Some(event) = map_event(
                        &state,
                        &secret,
                        &client_id,
                        stream_type,
                        change,
                    ).await {
                        yield Ok(event);
                    }
                }

                Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        client_id = %client_id,
                        skipped,
                        "SSE client lagged behind"
                    );
                }
            }
        }
    };

    Ok(Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(SSE_KEEPALIVE_INTERVAL)
            .text("keepalive"),
    ))
}

async fn map_event(
    state: &AppState,
    secret: &str,
    client_id: &str,
    stream_type: StreamType,
    change: DisconnectServerEvent,
) -> Option<Event> {
    let should_send = match stream_type {
        StreamType::Listener => !matches!(&change, DisconnectServerEvent::Control(_)),

        StreamType::Device => {
            let is_active_device = {
                let groups = state.groups.read().await;

                groups
                    .get(secret)
                    .is_some_and(|group| group.active_device == client_id)
            };

            match &change {
                DisconnectServerEvent::Control(_) => is_active_device,

                DisconnectServerEvent::Tracklist(_)
                | DisconnectServerEvent::Status(_)
                | DisconnectServerEvent::Position(_)
                | DisconnectServerEvent::AutoPlay(_)
                | DisconnectServerEvent::Volume(_) => !is_active_device,

                DisconnectServerEvent::ActiveDevice(_)
                | DisconnectServerEvent::AvailableDevices(_) => true,
            }
        }
    };

    if !should_send {
        return None;
    }

    match serde_json::to_string(&change) {
        Ok(json) => Some(Event::default().data(json)),

        Err(error) => {
            tracing::error!(?error, "failed to serialize SSE event");

            None
        }
    }
}
