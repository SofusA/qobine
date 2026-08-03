use controls_module::{TracklistReceiver, controls::Controls, tracklist::TracklistType};
use player_module::{
    AppResult,
    database::{Database, ReferenceType},
    error::PlayerError,
    notification::{Notification, NotificationBroadcast},
    player::Player,
};
use reqwest::{RequestBuilder, header::CONTENT_TYPE};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    sync::{
        Mutex,
        mpsc::{self, UnboundedSender},
    },
};

#[derive(Debug, Clone, Default)]
pub struct RfidState {
    link_request: Arc<Mutex<Option<ReferenceType>>>,
}

pub fn spawn_rfid(
    player: &Player,
    database: Arc<Database>,
    broadcast: Arc<NotificationBroadcast>,
    connect_device_name: Option<String>,
    rfid_server_base_address: Option<String>,
    rfid_server_secret: Option<String>,
    state: RfidState,
    set_active_device_tx: Option<UnboundedSender<String>>,
) {
    let controls = player.controls();
    let tracklist_receiver = player.tracklist();

    tokio::spawn(async move {
        if let Err(error) = init(
            state,
            tracklist_receiver,
            controls,
            database,
            broadcast,
            rfid_server_base_address,
            rfid_server_secret,
            connect_device_name,
            set_active_device_tx,
        )
        .await
        {
            eprintln!("{error}");
            std::process::exit(1);
        }
    });
}

async fn init(
    state: RfidState,
    tracklist_receiver: TracklistReceiver,
    controls: Controls,
    database: Arc<Database>,
    broadcast: Arc<NotificationBroadcast>,
    rfid_server_base_address: Option<String>,
    rfid_server_secret: Option<String>,
    connect_device_name: Option<String>,
    connect_set_device: Option<mpsc::UnboundedSender<String>>,
) -> AppResult<()> {
    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
    let mut out = tokio::io::stdout();
    let mut line = String::new();

    loop {
        out.write_all(b"Scan RFID: ")
            .await
            .or(Err(PlayerError::RfidInputPanic))?;
        out.flush().await.or(Err(PlayerError::RfidInputPanic))?;

        line.clear();

        let n = reader
            .read_line(&mut line)
            .await
            .or(Err(PlayerError::RfidInputPanic))?;
        if n == 0 {
            continue;
        }

        let res = line.trim();

        let maybe_request = {
            let guard = state.link_request.lock().await;
            guard.clone()
        };

        match maybe_request {
            Some(ReferenceType::Album(album_id)) => {
                submit_link_album(
                    state.clone(),
                    database.clone(),
                    broadcast.clone(),
                    res,
                    &album_id,
                    rfid_server_base_address.as_deref(),
                    rfid_server_secret.as_deref(),
                )
                .await;
            }
            Some(ReferenceType::Playlist(playlist_id)) => {
                submit_link_playlist(
                    state.clone(),
                    database.clone(),
                    broadcast.clone(),
                    res,
                    playlist_id,
                    rfid_server_base_address.as_deref(),
                    rfid_server_secret.as_deref(),
                )
                .await;
            }
            None => {
                handle_play_scan(
                    &database,
                    &controls,
                    &broadcast,
                    res,
                    &tracklist_receiver,
                    rfid_server_base_address.as_deref(),
                    rfid_server_secret.as_deref(),
                    connect_device_name.as_deref(),
                    connect_set_device.clone(),
                )
                .await;
            }
        }
    }
}

pub async fn handle_play_scan(
    database: &Database,
    controls: &Controls,
    broadcast: &NotificationBroadcast,
    reference_id: &str,
    tracklist_receiver: &TracklistReceiver,
    rfid_server_base_address: Option<&str>,
    rfid_server_secret: Option<&str>,
    connect_device_name: Option<&str>,
    connect_set_device: Option<mpsc::UnboundedSender<String>>,
) {
    let reference = match rfid_server_base_address {
        Some(server) => {
            let client = reqwest::Client::new();
            let url = format!("{server}/api/rfid/reference/{reference_id}");

            let mut request = client.get(&url);
            request = set_secret_header(request, rfid_server_secret);

            let response = match request
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
            {
                Ok(res) => res,
                Err(err) => {
                    broadcast.send_error(err.to_string());
                    return;
                }
            };

            match response.json().await {
                Ok(res) => res,
                Err(err) => {
                    broadcast.send_error(err.to_string());
                    return;
                }
            }
        }
        None => match database.get_reference(reference_id).await {
            Some(reference) => reference,
            None => {
                return;
            }
        },
    };

    let now_playing = {
        let tracklist = tracklist_receiver.borrow();
        tracklist.list_type().clone()
    };

    match reference {
        ReferenceType::Album(id) => {
            if let TracklistType::Album(now_playing_album) = &now_playing
                && now_playing_album.id == id
            {
                controls.play();
                return;
            }

            if let Some(connect_set_device) = connect_set_device
                && let Some(connect_device_name) = connect_device_name
            {
                _ = connect_set_device.send(connect_device_name.to_string());
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            controls.play_album(&id, 0);
        }

        ReferenceType::Playlist(id) => {
            if let TracklistType::Playlist(now_playing_playlist) = &now_playing
                && now_playing_playlist.id == id
            {
                controls.play();
                return;
            }

            if let Some(connect_set_device) = connect_set_device
                && let Some(connect_device_name) = connect_device_name
            {
                _ = connect_set_device.send(connect_device_name.to_string());
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            controls.play_playlist(id, 0, false);
        }
    }
}

pub async fn link(state: RfidState, request: ReferenceType, broadcast: Arc<NotificationBroadcast>) {
    set_state(&state, Some(request.clone())).await;

    let type_string = match request {
        ReferenceType::Album(_) => "album",
        ReferenceType::Playlist(_) => "playlist",
    };

    broadcast.send(Notification::Info(format!(
        "Scan rfid to link {type_string}"
    )));

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let request_ongoing = state.link_request.lock().await.is_some();

        if request_ongoing {
            broadcast.send(Notification::Warning("Scan cancelled".to_string()));
            set_state(&state, None).await;
        }
    });
}

async fn set_state(state: &RfidState, request: Option<ReferenceType>) {
    let mut request_lock = state.link_request.lock().await;
    *request_lock = request;
}

async fn submit_link_album(
    state: RfidState,
    database: Arc<Database>,
    broadcast: Arc<NotificationBroadcast>,
    rfid_id: &str,
    id: &str,
    rfid_server_base_address: Option<&str>,
    rfid_server_secret: Option<&str>,
) {
    let reference = ReferenceType::Album(id.to_owned());
    submit_link(
        state,
        database,
        broadcast,
        rfid_id,
        reference,
        rfid_server_base_address,
        rfid_server_secret,
    )
    .await;
}

async fn submit_link_playlist(
    state: RfidState,
    database: Arc<Database>,
    broadcast: Arc<NotificationBroadcast>,
    rfid_id: &str,
    id: u32,
    rfid_server_base_address: Option<&str>,
    rfid_server_secret: Option<&str>,
) {
    let reference = ReferenceType::Playlist(id);
    submit_link(
        state,
        database,
        broadcast,
        rfid_id,
        reference,
        rfid_server_base_address,
        rfid_server_secret,
    )
    .await;
}

async fn submit_link(
    state: RfidState,
    database: Arc<Database>,
    broadcast: Arc<NotificationBroadcast>,
    rfid_id: &str,
    reference: ReferenceType,
    rfid_server_base_address: Option<&str>,
    rfid_server_secret: Option<&str>,
) {
    if let Some(server) = rfid_server_base_address {
        let client = reqwest::Client::new();

        let mut request = match reference {
            ReferenceType::Album(id) => {
                let reference_payload = LinkAlbumRfid {
                    rfid_id: rfid_id.to_string(),
                    id,
                };

                let reference_payload = match serde_json::to_string(&reference_payload) {
                    Ok(res) => res,
                    Err(err) => {
                        broadcast.send_error(err.to_string());
                        return;
                    }
                };

                let url = format!("{server}/api/rfid/reference/album");
                let request = client.post(url);
                request.body(reference_payload)
            }
            ReferenceType::Playlist(id) => {
                let reference_payload = LinkPlaylistRfid {
                    rfid_id: rfid_id.to_string(),
                    id,
                };

                let reference_payload = match serde_json::to_string(&reference_payload) {
                    Ok(res) => res,
                    Err(err) => {
                        broadcast.send_error(err.to_string());
                        return;
                    }
                };

                let url = format!("{server}/api/rfid/reference/playlist");
                let request = client.post(url);
                request.body(reference_payload)
            }
        };

        request =
            set_secret_header(request, rfid_server_secret).header(CONTENT_TYPE, "application/json");

        match request
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(_) => {
                broadcast.send(Notification::Success("Link completed".to_string()));
                set_state(&state, None).await;
            }
            Err(err) => {
                broadcast.send_error(err.to_string());
                return;
            }
        }

        return;
    }

    let rfid_id = rfid_id.to_owned();

    match database.add_rfid_reference(rfid_id, reference).await {
        Ok(()) => {
            broadcast.send(Notification::Success("Link completed".to_string()));
            set_state(&state, None).await;
        }
        Err(err) => {
            broadcast.send(Notification::Error(err.to_string()));
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LinkAlbumRfid {
    pub rfid_id: String,
    pub id: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LinkPlaylistRfid {
    pub rfid_id: String,
    pub id: u32,
}

fn set_secret_header(mut request: RequestBuilder, secret: Option<&str>) -> RequestBuilder {
    if let Some(secret) = secret {
        request = request.header("Cookie", &format!("secret={secret}"));
    }
    request
}
