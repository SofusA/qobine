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
use serde::{Deserialize, Serialize};
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

#[derive(Deserialize, Serialize)]
struct DeviceRequest {
    device_id: String,
}

fn create_state() -> AppState {
    AppState {
        groups: Arc::new(RwLock::new(HashMap::new())),
        rate_limits: Arc::new(RwLock::new(HashMap::new())),
    }
}

fn create_app(state: AppState) -> Router {
    Router::new()
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
        .with_state(state)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let app = create_app(create_state());

    let address = SocketAddr::from(([0, 0, 0, 0], 3000));

    tracing::info!("listening on {}", address);

    let listener = match tokio::net::TcpListener::bind(address).await {
        Ok(listener) => listener,

        Err(error) => {
            tracing::error!(?error, "unable to bind to address: {address}");

            return;
        }
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

#[cfg(test)]
#[allow(clippy::expect_used)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::panic)]
#[allow(clippy::never_loop)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    use reqwest::Client;
    use reqwest_eventsource::{Event as EventSourceEvent, EventSource, RequestBuilderExt};
    use serde_json::Value;
    use tokio::{
        net::TcpListener,
        task::JoinHandle,
        time::{sleep, timeout},
    };

    const TEST_TIMEOUT: Duration = Duration::from_secs(2);

    struct TestServer {
        base_url: String,
        state: AppState,
        task: JoinHandle<()>,
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn spawn_server() -> TestServer {
        let state = create_state();
        let app = create_app(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind test server");

        let address = listener
            .local_addr()
            .expect("failed to get test server address");

        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("test server stopped unexpectedly");
        });

        TestServer {
            base_url: format!("http://{address}"),
            state,
            task,
        }
    }

    fn connect_stream(
        client: &Client,
        server: &TestServer,
        secret: &str,
        client_id: &str,
        stream_type: StreamType,
    ) -> EventSource {
        let stream_type = match stream_type {
            StreamType::Device => "device",
            StreamType::Listener => "listener",
        };

        client
            .get(format!("{}/stream", server.base_url))
            .query(&[
                ("secret", secret),
                ("device_id", client_id),
                ("stream_type", stream_type),
            ])
            .eventsource()
            .expect("failed to create event source")
    }

    async fn wait_until_open(event_source: &mut EventSource) {
        timeout(TEST_TIMEOUT, async {
            while let Some(event) = event_source.next().await {
                match event {
                    Ok(EventSourceEvent::Open) => return,
                    Ok(EventSourceEvent::Message(_)) => {
                        // A message also proves that the stream is connected.
                        return;
                    }
                    Err(error) => panic!("SSE connection failed: {error:?}"),
                }
            }

            panic!("SSE stream ended before opening");
        })
        .await
        .expect("timed out waiting for SSE stream to open");
    }

    async fn wait_for_json_event(event_source: &mut EventSource, expected: &Value) {
        timeout(TEST_TIMEOUT, async {
            while let Some(event) = event_source.next().await {
                match event {
                    Ok(EventSourceEvent::Open) => {}

                    Ok(EventSourceEvent::Message(message)) => {
                        let value: Value = serde_json::from_str(&message.data)
                            .expect("SSE message was not valid JSON");

                        if &value == expected {
                            return;
                        }
                    }

                    Err(error) => {
                        panic!("error while waiting for SSE event: {error:?}");
                    }
                }
            }

            panic!("SSE stream ended before expected event arrived");
        })
        .await
        .unwrap_or_else(|_| {
            panic!("timed out waiting for SSE event: {expected}");
        });
    }

    async fn assert_event_not_received(
        event_source: &mut EventSource,
        unexpected: &Value,
        duration: Duration,
    ) {
        let result = timeout(duration, async {
            while let Some(event) = event_source.next().await {
                match event {
                    Ok(EventSourceEvent::Open) => {}

                    Ok(EventSourceEvent::Message(message)) => {
                        let value: Value = serde_json::from_str(&message.data)
                            .expect("SSE message was not valid JSON");

                        assert!(
                            &value != unexpected,
                            "unexpected SSE event received: {unexpected}"
                        );
                    }

                    Err(error) => {
                        panic!("SSE stream failed unexpectedly: {error:?}");
                    }
                }
            }

            panic!("SSE stream ended unexpectedly");
        })
        .await;

        // A timeout is the expected result because no matching event arrived.
        assert!(result.is_err(), "event absence check finished unexpectedly");
    }

    async fn wait_for_group_removal(state: &AppState, secret: &str) {
        timeout(TEST_TIMEOUT, async {
            loop {
                if !state.groups.read().await.contains_key(secret) {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("group was not removed");
    }

    async fn get_disconnect_state(
        client: &Client,
        server: &TestServer,
        secret: &str,
    ) -> DisconnectState {
        client
            .get(format!("{}/state", server.base_url))
            .query(&[("secret", secret)])
            .send()
            .await
            .expect("state request failed")
            .error_for_status()
            .expect("state request returned an error")
            .json()
            .await
            .expect("failed to deserialize state")
    }

    fn disconnect(mut event_source: EventSource) {
        event_source.close();
        drop(event_source);
    }

    async fn wait_for_device_count(state: &AppState, secret: &str, expected: usize) {
        timeout(TEST_TIMEOUT, async {
            loop {
                let count = state
                    .groups
                    .read()
                    .await
                    .get(secret)
                    .map_or(0, |group| group.streams.len());

                if count == expected {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!("timed out waiting for {expected} devices in group {secret}");
        });
    }

    async fn wait_for_listener_count(state: &AppState, secret: &str, expected: usize) {
        timeout(TEST_TIMEOUT, async {
            loop {
                let count = state
                    .groups
                    .read()
                    .await
                    .get(secret)
                    .map_or(0, |group| group.listeners.len());

                if count == expected {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!("timed out waiting for {expected} listeners in group {secret}");
        });
    }

    async fn connect_device(
        client: &Client,
        server: &TestServer,
        secret: &str,
        device_id: &str,
    ) -> EventSource {
        let mut stream = connect_stream(client, server, secret, device_id, StreamType::Device);

        wait_until_open(&mut stream).await;
        stream
    }

    async fn connect_listener(
        client: &Client,
        server: &TestServer,
        secret: &str,
        listener_id: &str,
    ) -> EventSource {
        let mut stream = connect_stream(client, server, secret, listener_id, StreamType::Listener);

        wait_until_open(&mut stream).await;
        stream
    }

    async fn post_json(
        client: &Client,
        url: String,
        query: &[(&str, &str)],
        body: Value,
    ) -> reqwest::Response {
        client
            .post(url)
            .query(query)
            .json(&body)
            .send()
            .await
            .expect("request failed")
    }

    #[tokio::test]
    async fn devices_can_change_disconnect_and_rejoin_while_listener_survives() {
        let server = spawn_server().await;
        let client = Client::new();

        let secret = "happy-path-group";

        /*
         * Connect the first device.
         *
         * Waiting for ActiveDevice also ensures the server has fully processed
         * this connection before the second device connects.
         */
        let mut device_1 = connect_stream(&client, &server, secret, "device-1", StreamType::Device);

        let device_1_active =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-1".to_string()))
                .unwrap();

        wait_for_json_event(&mut device_1, &device_1_active).await;

        let mut device_2 = connect_stream(&client, &server, secret, "device-2", StreamType::Device);

        wait_until_open(&mut device_2).await;

        let mut listener =
            connect_stream(&client, &server, secret, "listener-1", StreamType::Listener);

        wait_until_open(&mut listener).await;

        let state = get_disconnect_state(&client, &server, secret).await;

        assert_eq!(state.active_device, "device-1");
        assert_eq!(state.available_devices.len(), 2);
        assert!(state.available_devices.contains(&"device-1".to_string()));
        assert!(state.available_devices.contains(&"device-2".to_string()));

        /*
         * Change the active device and assert that both devices and the
         * listener receive the ActiveDevice event.
         */
        let response = client
            .post(format!("{}/active-device", server.base_url))
            .query(&[("secret", secret)])
            .json(&DeviceRequest {
                device_id: "device-2".to_string(),
            })
            .send()
            .await
            .expect("active-device request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let device_2_active =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-2".to_string()))
                .unwrap();

        wait_for_json_event(&mut device_1, &device_2_active).await;
        wait_for_json_event(&mut device_2, &device_2_active).await;
        wait_for_json_event(&mut listener, &device_2_active).await;

        /*
         * Disconnect both devices. The group must remain because the listener
         * is still connected.
         */
        disconnect(device_1);
        disconnect(device_2);

        timeout(TEST_TIMEOUT, async {
            loop {
                let groups = server.state.groups.read().await;

                let devices_are_gone = groups.get(secret).is_some_and(|group| {
                    group.streams.is_empty() && group.listeners.contains("listener-1")
                });

                drop(groups);

                if devices_are_gone {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("devices did not disconnect correctly");

        /*
         * Connecting device-3 proves that the listener stream remained active.
         * Device-3 becomes active because no active device remains.
         */
        let mut device_3 = connect_stream(&client, &server, secret, "device-3", StreamType::Device);

        let device_3_active =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-3".to_string()))
                .unwrap();

        wait_for_json_event(&mut device_3, &device_3_active).await;
        wait_for_json_event(&mut listener, &device_3_active).await;

        let state = get_disconnect_state(&client, &server, secret).await;

        assert_eq!(state.active_device, "device-3");
        assert_eq!(state.available_devices, vec!["device-3".to_string()]);

        disconnect(device_3);
        disconnect(listener);

        wait_for_group_removal(&server.state, secret).await;

        let response = client
            .get(format!("{}/state", server.base_url))
            .query(&[("secret", secret)])
            .send()
            .await
            .expect("state request failed");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn listener_control_command_is_delivered_only_to_active_device() {
        let server = spawn_server().await;
        let client = Client::new();

        let secret = "control-group";

        let mut active_device =
            connect_stream(&client, &server, secret, "device-1", StreamType::Device);

        let initial_active_event =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-1".to_string()))
                .unwrap();

        wait_for_json_event(&mut active_device, &initial_active_event).await;

        let mut inactive_device =
            connect_stream(&client, &server, secret, "device-2", StreamType::Device);

        wait_until_open(&mut inactive_device).await;

        let mut listener =
            connect_stream(&client, &server, secret, "listener-1", StreamType::Listener);

        wait_until_open(&mut listener).await;

        let command = ControlCommand::Play;

        let expected_event = serde_json::to_value(DisconnectServerEvent::Control(command.clone()))
            .expect("failed to serialize expected control event");

        let response = client
            .post(format!("{}/control", server.base_url))
            .query(&[("secret", secret), ("device_id", "listener-1")])
            .json(&command)
            .send()
            .await
            .expect("control request failed");

        assert_eq!(response.status(), StatusCode::OK);

        wait_for_json_event(&mut active_device, &expected_event).await;

        assert_event_not_received(
            &mut inactive_device,
            &expected_event,
            Duration::from_millis(300),
        )
        .await;

        assert_event_not_received(&mut listener, &expected_event, Duration::from_millis(300)).await;

        disconnect(active_device);
        disconnect(inactive_device);
        disconnect(listener);

        wait_for_group_removal(&server.state, secret).await;
    }

    #[tokio::test]
    async fn active_device_disconnect_causes_failover() {
        let server = spawn_server().await;
        let client = Client::new();
        let secret = "failover-group";

        let mut device_1 = connect_stream(&client, &server, secret, "device-1", StreamType::Device);

        let device_1_active =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-1".to_string()))
                .unwrap();

        wait_for_json_event(&mut device_1, &device_1_active).await;

        let mut device_2 = connect_device(&client, &server, secret, "device-2").await;

        let mut device_3 = connect_device(&client, &server, secret, "device-3").await;

        let mut listener = connect_listener(&client, &server, secret, "listener-1").await;

        wait_for_device_count(&server.state, secret, 3).await;
        wait_for_listener_count(&server.state, secret, 1).await;

        let response = client
            .post(format!("{}/active-device", server.base_url))
            .query(&[("secret", secret)])
            .json(&DeviceRequest {
                device_id: "device-2".to_string(),
            })
            .send()
            .await
            .expect("active-device request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let device_2_active =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-2".to_string()))
                .unwrap();

        wait_for_json_event(&mut device_1, &device_2_active).await;
        wait_for_json_event(&mut device_2, &device_2_active).await;
        wait_for_json_event(&mut device_3, &device_2_active).await;
        wait_for_json_event(&mut listener, &device_2_active).await;

        disconnect(device_2);

        wait_for_device_count(&server.state, secret, 2).await;

        let state = get_disconnect_state(&client, &server, secret).await;

        assert_ne!(state.active_device, "device-2");

        assert!(
            state.active_device == "device-1" || state.active_device == "device-3",
            "unexpected active device: {}",
            state.active_device
        );

        assert_eq!(state.available_devices.len(), 2);
        assert!(state.available_devices.contains(&"device-1".to_string()));
        assert!(state.available_devices.contains(&"device-3".to_string()));
        assert!(!state.available_devices.contains(&"device-2".to_string()));

        let expected_active_event = serde_json::to_value(DisconnectServerEvent::ActiveDevice(
            state.active_device.clone(),
        ))
        .unwrap();

        wait_for_json_event(&mut device_1, &expected_active_event).await;

        wait_for_json_event(&mut device_3, &expected_active_event).await;

        wait_for_json_event(&mut listener, &expected_active_event).await;

        disconnect(device_1);
        disconnect(device_3);
        disconnect(listener);

        wait_for_group_removal(&server.state, secret).await;
    }

    #[tokio::test]
    async fn only_active_device_can_update_playback_state() {
        struct EndpointCase {
            path: &'static str,
            body: Value,
        }

        let server = spawn_server().await;
        let client = Client::new();
        let secret = "state-authorization";

        let mut active_device =
            connect_stream(&client, &server, secret, "device-1", StreamType::Device);

        let active_event =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-1".to_string()))
                .unwrap();

        wait_for_json_event(&mut active_device, &active_event).await;

        let inactive_device = connect_device(&client, &server, secret, "device-2").await;

        let listener = connect_listener(&client, &server, secret, "listener-1").await;

        let cases = vec![
            EndpointCase {
                path: "/tracklist",
                body: serde_json::to_value(Tracklist::default()).unwrap(),
            },
            EndpointCase {
                path: "/status",
                body: serde_json::to_value(Status::default()).unwrap(),
            },
            EndpointCase {
                path: "/position",
                body: serde_json::to_value(Duration::from_secs(42)).unwrap(),
            },
            EndpointCase {
                path: "/volume",
                body: serde_json::json!(0.5),
            },
            EndpointCase {
                path: "/autoplay",
                body: serde_json::json!(true),
            },
        ];

        for test_case in cases {
            let url = format!("{}{}", server.base_url, test_case.path);

            let response = post_json(
                &client,
                url.clone(),
                &[("secret", secret), ("device_id", "device-1")],
                test_case.body.clone(),
            )
            .await;

            assert_eq!(
                response.status(),
                StatusCode::OK,
                "active device should be allowed to call {}",
                test_case.path
            );

            let response = post_json(
                &client,
                url.clone(),
                &[("secret", secret), ("device_id", "device-2")],
                test_case.body.clone(),
            )
            .await;

            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "inactive device should be rejected by {}",
                test_case.path
            );

            let response = post_json(
                &client,
                url.clone(),
                &[("secret", secret), ("device_id", "listener-1")],
                test_case.body.clone(),
            )
            .await;

            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "listener should be rejected by {}",
                test_case.path
            );

            let response = post_json(
                &client,
                url,
                &[("secret", secret), ("device_id", "unknown-client")],
                test_case.body,
            )
            .await;

            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "unknown client should be rejected by {}",
                test_case.path
            );
        }

        let state = get_disconnect_state(&client, &server, secret).await;

        assert_eq!(state.position, Duration::from_secs(42));
        assert_eq!(state.volume, 0.5);
        assert!(state.auto_play);

        disconnect(active_device);
        disconnect(inactive_device);
        disconnect(listener);

        wait_for_group_removal(&server.state, secret).await;
    }

    #[tokio::test]
    async fn control_commands_have_correct_sender_authorization() {
        let server = spawn_server().await;
        let client = Client::new();
        let secret = "control-authorization";

        let mut active_device =
            connect_stream(&client, &server, secret, "device-1", StreamType::Device);

        let active_event =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-1".to_string()))
                .unwrap();

        wait_for_json_event(&mut active_device, &active_event).await;

        let mut inactive_device = connect_device(&client, &server, secret, "device-2").await;

        let listener = connect_listener(&client, &server, secret, "listener-1").await;

        let command = ControlCommand::Play;

        let expected_event =
            serde_json::to_value(DisconnectServerEvent::Control(command.clone())).unwrap();

        let response = client
            .post(format!("{}/control", server.base_url))
            .query(&[("secret", secret), ("device_id", "listener-1")])
            .json(&command)
            .send()
            .await
            .expect("listener control request failed");

        assert_eq!(response.status(), StatusCode::OK);

        wait_for_json_event(&mut active_device, &expected_event).await;

        let response = client
            .post(format!("{}/control", server.base_url))
            .query(&[("secret", secret), ("device_id", "device-2")])
            .json(&command)
            .send()
            .await
            .expect("inactive-device control request failed");

        assert_eq!(response.status(), StatusCode::OK);

        wait_for_json_event(&mut active_device, &expected_event).await;

        let response = client
            .post(format!("{}/control", server.base_url))
            .query(&[("secret", secret), ("device_id", "device-1")])
            .json(&command)
            .send()
            .await
            .expect("active-device control request failed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = client
            .post(format!("{}/control", server.base_url))
            .query(&[("secret", secret), ("device_id", "unknown-client")])
            .json(&command)
            .send()
            .await
            .expect("unknown-client control request failed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        assert_event_not_received(
            &mut inactive_device,
            &expected_event,
            Duration::from_millis(300),
        )
        .await;

        disconnect(active_device);
        disconnect(inactive_device);
        disconnect(listener);

        wait_for_group_removal(&server.state, secret).await;
    }

    #[tokio::test]
    async fn duplicate_and_cross_type_client_ids_are_rejected() {
        let server = spawn_server().await;
        let client = Client::new();
        let secret = "duplicate-group";

        let mut device = connect_stream(
            &client,
            &server,
            secret,
            "shared-device",
            StreamType::Device,
        );

        let active_event = serde_json::to_value(DisconnectServerEvent::ActiveDevice(
            "shared-device".to_string(),
        ))
        .unwrap();

        wait_for_json_event(&mut device, &active_event).await;

        let listener = connect_listener(&client, &server, secret, "shared-listener").await;

        let duplicate_device_response = client
            .get(format!("{}/stream", server.base_url))
            .query(&[
                ("secret", secret),
                ("device_id", "shared-device"),
                ("stream_type", "device"),
            ])
            .send()
            .await
            .expect("duplicate-device request failed");

        assert_eq!(duplicate_device_response.status(), StatusCode::CONFLICT);

        let duplicate_listener_response = client
            .get(format!("{}/stream", server.base_url))
            .query(&[
                ("secret", secret),
                ("device_id", "shared-listener"),
                ("stream_type", "listener"),
            ])
            .send()
            .await
            .expect("duplicate-listener request failed");

        assert_eq!(duplicate_listener_response.status(), StatusCode::CONFLICT);

        let device_as_listener_response = client
            .get(format!("{}/stream", server.base_url))
            .query(&[
                ("secret", secret),
                ("device_id", "shared-device"),
                ("stream_type", "listener"),
            ])
            .send()
            .await
            .expect("device-as-listener request failed");

        assert_eq!(device_as_listener_response.status(), StatusCode::CONFLICT);

        let listener_as_device_response = client
            .get(format!("{}/stream", server.base_url))
            .query(&[
                ("secret", secret),
                ("device_id", "shared-listener"),
                ("stream_type", "device"),
            ])
            .send()
            .await
            .expect("listener-as-device request failed");

        assert_eq!(listener_as_device_response.status(), StatusCode::CONFLICT);

        let state = get_disconnect_state(&client, &server, secret).await;

        assert_eq!(state.active_device, "shared-device");
        assert_eq!(state.available_devices, vec!["shared-device".to_string()]);

        {
            let groups = server.state.groups.read().await;
            let group = groups.get(secret).expect("group should exist");

            assert_eq!(group.streams.len(), 1);
            assert_eq!(group.listeners.len(), 1);
            assert!(group.streams.contains("shared-device"));
            assert!(group.listeners.contains("shared-listener"));
        }

        /*
         * Verify that the original connection remains operational after all
         * rejected duplicate attempts.
         */
        let command = ControlCommand::Play;

        let expected_event =
            serde_json::to_value(DisconnectServerEvent::Control(command.clone())).unwrap();

        let response = client
            .post(format!("{}/control", server.base_url))
            .query(&[("secret", secret), ("device_id", "shared-listener")])
            .json(&command)
            .send()
            .await
            .expect("control request failed");

        assert_eq!(response.status(), StatusCode::OK);

        wait_for_json_event(&mut device, &expected_event).await;

        disconnect(device);
        disconnect(listener);

        wait_for_group_removal(&server.state, secret).await;
    }

    #[tokio::test]
    async fn newly_connected_listener_can_fetch_complete_current_state() {
        let server = spawn_server().await;
        let client = Client::new();
        let secret = "initial-state";

        let mut device = connect_stream(&client, &server, secret, "device-1", StreamType::Device);

        let active_event =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-1".to_string()))
                .unwrap();

        wait_for_json_event(&mut device, &active_event).await;

        let tracklist = Tracklist::default();
        let status = Status::default();
        let position = Duration::from_secs(123);
        let volume = 0.35_f32;
        let auto_play = true;

        let response = client
            .post(format!("{}/tracklist", server.base_url))
            .query(&[("secret", secret), ("device_id", "device-1")])
            .json(&tracklist)
            .send()
            .await
            .expect("tracklist request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let response = client
            .post(format!("{}/status", server.base_url))
            .query(&[("secret", secret), ("device_id", "device-1")])
            .json(&status)
            .send()
            .await
            .expect("status request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let response = client
            .post(format!("{}/position", server.base_url))
            .query(&[("secret", secret), ("device_id", "device-1")])
            .json(&position)
            .send()
            .await
            .expect("position request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let response = client
            .post(format!("{}/volume", server.base_url))
            .query(&[("secret", secret), ("device_id", "device-1")])
            .json(&volume)
            .send()
            .await
            .expect("volume request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let response = client
            .post(format!("{}/autoplay", server.base_url))
            .query(&[("secret", secret), ("device_id", "device-1")])
            .json(&auto_play)
            .send()
            .await
            .expect("autoplay request failed");

        assert_eq!(response.status(), StatusCode::OK);

        /*
         * The listener connects after all updates have already occurred.
         */
        let listener = connect_listener(&client, &server, secret, "listener-1").await;

        let state = get_disconnect_state(&client, &server, secret).await;

        assert_eq!(state.active_device, "device-1");
        assert_eq!(state.available_devices, vec!["device-1".to_string()]);
        assert_eq!(state.position, position);
        assert_eq!(state.volume, volume);
        assert_eq!(state.auto_play, auto_play);

        /*
         * These JSON comparisons avoid requiring PartialEq on Status and
         * Tracklist.
         */
        assert_eq!(
            serde_json::to_value(state.playback_status).unwrap(),
            serde_json::to_value(status).unwrap()
        );

        assert_eq!(
            serde_json::to_value(&state.tracklist).unwrap(),
            serde_json::to_value(&tracklist).unwrap()
        );

        disconnect(device);
        disconnect(listener);

        wait_for_group_removal(&server.state, secret).await;
    }

    #[tokio::test]
    async fn groups_with_different_secrets_are_fully_isolated() {
        let server = spawn_server().await;
        let client = Client::new();

        let secret_a = "group-a";
        let secret_b = "group-b";

        /*
         * Deliberately use identical client IDs in both groups.
         */
        let mut device_a =
            connect_stream(&client, &server, secret_a, "device-1", StreamType::Device);

        let active_a =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-1".to_string()))
                .unwrap();

        wait_for_json_event(&mut device_a, &active_a).await;

        let listener_a = connect_listener(&client, &server, secret_a, "listener-1").await;

        let mut device_b =
            connect_stream(&client, &server, secret_b, "device-1", StreamType::Device);

        let active_b =
            serde_json::to_value(DisconnectServerEvent::ActiveDevice("device-1".to_string()))
                .unwrap();

        wait_for_json_event(&mut device_b, &active_b).await;

        let listener_b = connect_listener(&client, &server, secret_b, "listener-1").await;

        let response = client
            .post(format!("{}/volume", server.base_url))
            .query(&[("secret", secret_a), ("device_id", "device-1")])
            .json(&0.25_f32)
            .send()
            .await
            .expect("group A volume request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let volume_event = serde_json::to_value(DisconnectServerEvent::Volume(0.25)).unwrap();

        /*
         * The active device does not receive its own state update, but the
         * listener in group A does. Group B must not receive it.
         */
        let mut listener_a = listener_a;
        let mut listener_b = listener_b;

        wait_for_json_event(&mut listener_a, &volume_event).await;

        assert_event_not_received(&mut device_b, &volume_event, Duration::from_millis(300)).await;

        assert_event_not_received(&mut listener_b, &volume_event, Duration::from_millis(300)).await;

        let command = ControlCommand::Play;

        let control_event =
            serde_json::to_value(DisconnectServerEvent::Control(command.clone())).unwrap();

        let response = client
            .post(format!("{}/control", server.base_url))
            .query(&[("secret", secret_a), ("device_id", "listener-1")])
            .json(&command)
            .send()
            .await
            .expect("group A control request failed");

        assert_eq!(response.status(), StatusCode::OK);

        wait_for_json_event(&mut device_a, &control_event).await;

        assert_event_not_received(&mut device_b, &control_event, Duration::from_millis(300)).await;

        let state_a = get_disconnect_state(&client, &server, secret_a).await;

        let state_b = get_disconnect_state(&client, &server, secret_b).await;

        assert_eq!(state_a.volume, 0.25);
        assert_eq!(state_b.volume, 1.0);

        /*
         * Remove group A and verify group B remains alive and functional.
         */
        disconnect(device_a);
        disconnect(listener_a);

        wait_for_group_removal(&server.state, secret_a).await;

        {
            let groups = server.state.groups.read().await;

            assert!(
                !groups.contains_key(secret_a),
                "group A should have been removed"
            );

            assert!(groups.contains_key(secret_b), "group B should remain");
        }

        let response = client
            .get(format!("{}/state", server.base_url))
            .query(&[("secret", secret_b)])
            .send()
            .await
            .expect("group B state request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let response = client
            .post(format!("{}/autoplay", server.base_url))
            .query(&[("secret", secret_b), ("device_id", "device-1")])
            .json(&true)
            .send()
            .await
            .expect("group B autoplay request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let state_b = get_disconnect_state(&client, &server, secret_b).await;

        assert!(state_b.auto_play);
        assert_eq!(state_b.volume, 1.0);

        disconnect(device_b);
        disconnect(listener_b);

        wait_for_group_removal(&server.state, secret_b).await;
    }
}
