use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use apple_cf::cf::CFRunLoop;
use controls_module::{
    ExitSender, PositionReceiver, Status, StatusReceiver, TracklistReceiver, controls::Controls,
    models::Track, tracklist::Tracklist,
};
use dispatch2::DispatchQueue;
use mediaplayer::{
    Artwork,
    now_playing::{NowPlayingInfo, NowPlayingInfoCenter, NowPlayingMediaType, PlaybackState},
    remote_commands::{CommandEvent, CommandToken, HandlerStatus, RemoteCommandCenter},
};
use player_module::player::Player;

pub fn run_with_main_loop<F, Fut>(run: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()>,
{
    let exited = Arc::new(AtomicBool::new(false));

    let thread_exited = exited.clone();
    std::thread::spawn(move || {
        match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime.block_on(run()),
            Err(err) => eprintln!("{err}"),
        }

        thread_exited.store(true, Ordering::Release);
        CFRunLoop::main().stop();
    });

    while !exited.load(Ordering::Acquire) {
        let _ = CFRunLoop::current().run_in_default_mode(Duration::from_secs(1), false);
    }
}

pub fn spawn_now_playing(player: &Player, exit_sender: &ExitSender) {
    let position_receiver = player.position();
    let tracklist_receiver = player.tracklist();
    let status_receiver = player.status();
    let controls = player.controls();
    let exit_sender = exit_sender.clone();
    tokio::spawn(init(
        position_receiver,
        tracklist_receiver,
        status_receiver,
        controls,
        exit_sender,
    ));
}

struct NowPlaying {
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration_seconds: f64,
    elapsed_seconds: f64,
    status: Status,
    artwork_url: Option<String>,
    queue_index: u64,
    queue_count: u64,
}

async fn init(
    position_receiver: PositionReceiver,
    mut tracklist_receiver: TracklistReceiver,
    mut status_receiver: StatusReceiver,
    controls: Controls,
    exit_sender: ExitSender,
) {
    let mut exit_receiver = exit_sender.subscribe();
    let mut position_change_receiver = position_receiver.clone();

    let center = NowPlayingInfoCenter::default_center();

    let (token_sender, token_receiver) = tokio::sync::oneshot::channel();
    {
        let controls = controls.clone();
        DispatchQueue::main().exec_async(move || {
            let _ = token_sender.send(register_commands(&controls));
        });
    }
    let _tokens = token_receiver.await.ok();

    let mut current: Option<NowPlaying> = None;
    let mut artwork_cache: Option<(String, Artwork)> = None;
    let mut last_position = *position_receiver.borrow();

    let now_playing = {
        let tracklist = tracklist_receiver.borrow();
        tracklist
            .current_track()
            .cloned()
            .map(|track| build_now_playing(track, &tracklist, &position_receiver, &status_receiver))
    };
    if let Some(now_playing) = now_playing {
        push(&center, Some(&now_playing), &mut artwork_cache).await;
        current = Some(now_playing);
    }

    loop {
        tokio::select! {
            Ok(()) = tracklist_receiver.changed() => {
                current = {
                    let tracklist = tracklist_receiver.borrow_and_update();
                    tracklist
                        .current_track()
                        .cloned()
                        .map(|track| build_now_playing(track, &tracklist, &position_receiver, &status_receiver))
                };
                push(&center, current.as_ref(), &mut artwork_cache).await;
            },
            Ok(()) = status_receiver.changed() => {
                let status = *status_receiver.borrow_and_update();
                if let Some(now_playing) = current.as_mut() {
                    now_playing.status = status;
                    now_playing.elapsed_seconds = position_receiver.borrow().as_secs_f64();
                    push(&center, Some(now_playing), &mut artwork_cache).await;
                }
            },
            Ok(()) = position_change_receiver.changed() => {
                let position = *position_change_receiver.borrow_and_update();
                let jumped = position.abs_diff(last_position) > Duration::from_secs(3);
                last_position = position;

                if jumped && let Some(now_playing) = current.as_mut() {
                    now_playing.elapsed_seconds = position.as_secs_f64();
                    push(&center, Some(now_playing), &mut artwork_cache).await;
                }
            },
            Ok(exit) = exit_receiver.recv() => {
                if exit {
                    break;
                }
            }
        }
    }
}

fn build_now_playing(
    track: Track,
    tracklist: &Tracklist,
    position_receiver: &PositionReceiver,
    status_receiver: &StatusReceiver,
) -> NowPlaying {
    NowPlaying {
        title: track.title,
        artist: track.artist_name,
        album: track.album_title,
        duration_seconds: f64::from(track.duration_seconds),
        elapsed_seconds: position_receiver.borrow().as_secs_f64(),
        status: *status_receiver.borrow(),
        artwork_url: track.image,
        queue_index: u64::try_from(tracklist.current_position()).unwrap_or_default(),
        queue_count: u64::try_from(tracklist.total()).unwrap_or_default(),
    }
}

async fn push(
    center: &NowPlayingInfoCenter,
    now_playing: Option<&NowPlaying>,
    artwork_cache: &mut Option<(String, Artwork)>,
) {
    let Some(now_playing) = now_playing else {
        center.clear();
        center.set_playback_state(PlaybackState::Stopped);
        return;
    };

    let artwork = fetch_artwork(artwork_cache, now_playing.artwork_url.as_deref()).await;

    let rate = match now_playing.status {
        Status::Playing => 1.0,
        Status::Buffering | Status::Paused => 0.0,
    };

    let mut info = NowPlayingInfo::new()
        .title(&now_playing.title)
        .playback_duration(now_playing.duration_seconds)
        .elapsed_playback_time(now_playing.elapsed_seconds)
        .playback_rate(rate)
        .playback_queue_index(now_playing.queue_index)
        .playback_queue_count(now_playing.queue_count)
        .media_type(NowPlayingMediaType::Audio);

    if let Some(artist) = &now_playing.artist {
        info = info.artist(artist);
    }
    if let Some(album) = &now_playing.album {
        info = info.album_title(album);
    }

    center.set_now_playing_info_with_artwork(&info, artwork);

    let state = match now_playing.status {
        Status::Playing | Status::Buffering => PlaybackState::Playing,
        Status::Paused => PlaybackState::Paused,
    };
    center.set_playback_state(state);
}

async fn fetch_artwork<'a>(
    cache: &'a mut Option<(String, Artwork)>,
    url: Option<&str>,
) -> Option<&'a Artwork> {
    let url = url?;

    let cached = cache
        .as_ref()
        .is_some_and(|(cached_url, _)| cached_url == url);
    if !cached {
        let artwork = download_artwork(url).await?;
        *cache = Some((url.to_string(), artwork));
    }

    cache.as_ref().map(|(_, artwork)| artwork)
}

async fn download_artwork(url: &str) -> Option<Artwork> {
    let response = reqwest::get(url).await.ok()?;
    let bytes = response.bytes().await.ok()?;

    let path = std::env::temp_dir().join(format!("qobine-artwork-{}.jpg", std::process::id()));
    tokio::fs::write(&path, &bytes).await.ok()?;

    let artwork = Artwork::from_path(path.to_str()?).ok();
    let _ = tokio::fs::remove_file(&path).await;
    artwork
}

fn command_handler(
    controls: &Controls,
    action: fn(&Controls),
) -> impl FnMut(CommandEvent) -> HandlerStatus + Send + 'static {
    let controls = controls.clone();
    move |_event| {
        action(&controls);
        HandlerStatus::Success
    }
}

fn register_commands(controls: &Controls) -> Vec<CommandToken> {
    let center = RemoteCommandCenter::shared();

    let seek_controls = controls.clone();

    vec![
        center.on_play(command_handler(controls, Controls::play)),
        center.on_pause(command_handler(controls, Controls::pause)),
        center.on_toggle_play_pause(command_handler(controls, Controls::play_pause)),
        center.on_stop(command_handler(controls, Controls::pause)),
        center.on_next_track(command_handler(controls, Controls::next)),
        center.on_previous_track(command_handler(controls, Controls::previous)),
        center.on_change_playback_position(move |event| {
            if let Some(position) = event.position {
                seek_controls.seek(Duration::from_secs_f64(position.max(0.0)));
            }
            HandlerStatus::Success
        }),
    ]
}
