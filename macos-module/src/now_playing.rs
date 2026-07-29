use std::{
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use block2::RcBlock;
use controls_module::{
    ExitSender, PositionReceiver, Status, StatusReceiver, TracklistReceiver, controls::Controls,
    models::Track,
};
use dispatch2::DispatchQueue;
use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
use objc2_app_kit::NSImage;
use objc2_core_foundation::{CFRunLoop, CGSize};
use objc2_foundation::{NSData, NSMutableDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle,
    MPMediaItemPropertyAlbumTrackNumber, MPMediaItemPropertyArtist, MPMediaItemPropertyArtwork,
    MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle, MPNowPlayingInfoCenter,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
    MPNowPlayingPlaybackState, MPRemoteCommand, MPRemoteCommandCenter, MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
};
use player_module::player::Player;

static EXITED: AtomicBool = AtomicBool::new(false);

pub fn run_main_loop() {
    while !EXITED.load(Ordering::Acquire) {
        CFRunLoop::run();
    }
}

pub fn stop_main_loop() {
    EXITED.store(true, Ordering::Release);
    DispatchQueue::main().exec_async(|| {
        if let Some(run_loop) = CFRunLoop::current() {
            run_loop.stop();
        }
    });
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

#[derive(Clone)]
struct NowPlaying {
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration_seconds: f64,
    track_number: u32,
    elapsed_seconds: f64,
    status: Status,
    artwork: Option<Arc<Vec<u8>>>,
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

    {
        let controls = controls.clone();
        DispatchQueue::main().exec_async(move || register_commands(controls));
    }

    let mut current: Option<NowPlaying> = None;
    let mut artwork_cache: Option<(String, Arc<Vec<u8>>)> = None;
    let mut last_position = *position_receiver.borrow();

    let initial_track = tracklist_receiver.borrow().current_track().cloned();
    if let Some(track) = initial_track {
        let now_playing = build_now_playing(
            track,
            &mut artwork_cache,
            &position_receiver,
            &status_receiver,
        )
        .await;
        current = Some(now_playing.clone());
        push(Some(now_playing));
    }

    loop {
        tokio::select! {
            Ok(_) = tracklist_receiver.changed() => {
                let current_track = tracklist_receiver.borrow_and_update().current_track().cloned();

                match current_track {
                    Some(track) => {
                        let now_playing = build_now_playing(
                            track,
                            &mut artwork_cache,
                            &position_receiver,
                            &status_receiver,
                        )
                        .await;
                        current = Some(now_playing.clone());
                        push(Some(now_playing));
                    }
                    None => {
                        current = None;
                        push(None);
                    }
                }
            },
            Ok(_) = status_receiver.changed() => {
                let status = *status_receiver.borrow_and_update();
                if let Some(now_playing) = current.as_mut() {
                    now_playing.status = status;
                    now_playing.elapsed_seconds = position_receiver.borrow().as_secs_f64();
                    push(Some(now_playing.clone()));
                }
            },
            Ok(_) = position_change_receiver.changed() => {
                let position = *position_change_receiver.borrow_and_update();
                let jumped = position.abs_diff(last_position) > Duration::from_secs(3);
                last_position = position;

                if jumped && let Some(now_playing) = current.as_mut() {
                    now_playing.elapsed_seconds = position.as_secs_f64();
                    push(Some(now_playing.clone()));
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

async fn build_now_playing(
    track: Track,
    artwork_cache: &mut Option<(String, Arc<Vec<u8>>)>,
    position_receiver: &PositionReceiver,
    status_receiver: &StatusReceiver,
) -> NowPlaying {
    let artwork = fetch_artwork(artwork_cache, track.image.as_deref()).await;
    NowPlaying {
        title: track.title,
        artist: track.artist_name,
        album: track.album_title,
        duration_seconds: track.duration_seconds as f64,
        track_number: track.number,
        elapsed_seconds: position_receiver.borrow().as_secs_f64(),
        status: *status_receiver.borrow(),
        artwork,
    }
}

async fn fetch_artwork(
    cache: &mut Option<(String, Arc<Vec<u8>>)>,
    url: Option<&str>,
) -> Option<Arc<Vec<u8>>> {
    let url = url?;

    if let Some((cached_url, bytes)) = cache
        && cached_url == url
    {
        return Some(bytes.clone());
    }

    let response = reqwest::get(url).await.ok()?;
    let bytes = Arc::new(response.bytes().await.ok()?.to_vec());
    *cache = Some((url.to_string(), bytes.clone()));
    Some(bytes)
}

fn push(now_playing: Option<NowPlaying>) {
    DispatchQueue::main().exec_async(move || set_now_playing(now_playing));
}

fn set_now_playing(now_playing: Option<NowPlaying>) {
    let center = unsafe { MPNowPlayingInfoCenter::defaultCenter() };

    let Some(now_playing) = now_playing else {
        unsafe {
            center.setNowPlayingInfo(None);
            center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
        }
        return;
    };

    let info = NSMutableDictionary::<NSString, AnyObject>::new();

    insert_string(
        &info,
        unsafe { MPMediaItemPropertyTitle },
        &now_playing.title,
    );
    if let Some(artist) = &now_playing.artist {
        insert_string(&info, unsafe { MPMediaItemPropertyArtist }, artist);
    }
    if let Some(album) = &now_playing.album {
        insert_string(&info, unsafe { MPMediaItemPropertyAlbumTitle }, album);
    }

    let rate = match now_playing.status {
        Status::Playing => 1.0,
        Status::Buffering | Status::Paused => 0.0,
    };

    insert_number(
        &info,
        unsafe { MPMediaItemPropertyPlaybackDuration },
        now_playing.duration_seconds,
    );
    insert_number(
        &info,
        unsafe { MPMediaItemPropertyAlbumTrackNumber },
        f64::from(now_playing.track_number),
    );
    insert_number(
        &info,
        unsafe { MPNowPlayingInfoPropertyElapsedPlaybackTime },
        now_playing.elapsed_seconds,
    );
    insert_number(&info, unsafe { MPNowPlayingInfoPropertyPlaybackRate }, rate);

    if let Some(artwork) = now_playing.artwork.as_ref().and_then(make_artwork) {
        info.insert(unsafe { MPMediaItemPropertyArtwork }, &artwork);
    }

    let state = match now_playing.status {
        Status::Playing | Status::Buffering => MPNowPlayingPlaybackState::Playing,
        Status::Paused => MPNowPlayingPlaybackState::Paused,
    };

    unsafe {
        center.setNowPlayingInfo(Some(&info));
        center.setPlaybackState(state);
    }
}

fn insert_string(info: &NSMutableDictionary<NSString, AnyObject>, key: &NSString, value: &str) {
    info.insert(key, &NSString::from_str(value));
}

fn insert_number(info: &NSMutableDictionary<NSString, AnyObject>, key: &NSString, value: f64) {
    info.insert(key, &NSNumber::new_f64(value));
}

fn make_artwork(bytes: &Arc<Vec<u8>>) -> Option<Retained<MPMediaItemArtwork>> {
    let data = NSData::with_bytes(bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    let size = image.size();

    let handler = RcBlock::new(move |_size: CGSize| NonNull::from(&*image));

    Some(unsafe {
        MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            size,
            &handler,
        )
    })
}

fn register_commands(controls: Controls) {
    let center = unsafe { MPRemoteCommandCenter::sharedCommandCenter() };

    let play = unsafe { center.playCommand() };
    let pause = unsafe { center.pauseCommand() };
    let toggle = unsafe { center.togglePlayPauseCommand() };
    let stop = unsafe { center.stopCommand() };
    let next = unsafe { center.nextTrackCommand() };
    let previous = unsafe { center.previousTrackCommand() };

    add_command(&play, &controls, Controls::play);
    add_command(&pause, &controls, Controls::pause);
    add_command(&toggle, &controls, Controls::play_pause);
    add_command(&stop, &controls, Controls::pause);
    add_command(&next, &controls, Controls::next);
    add_command(&previous, &controls, Controls::previous);

    let handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
        let event = unsafe { event.as_ref() };
        let Some(event) = event.downcast_ref::<MPChangePlaybackPositionCommandEvent>() else {
            return MPRemoteCommandHandlerStatus::CommandFailed;
        };
        let position = unsafe { event.positionTime() }.max(0.0);
        controls.seek(Duration::from_secs_f64(position));
        MPRemoteCommandHandlerStatus::Success
    });

    unsafe {
        center
            .changePlaybackPositionCommand()
            .addTargetWithHandler(&handler);

        center.skipForwardCommand().setEnabled(false);
        center.skipBackwardCommand().setEnabled(false);
        center.seekForwardCommand().setEnabled(false);
        center.seekBackwardCommand().setEnabled(false);
        center.changePlaybackRateCommand().setEnabled(false);
        center.changeRepeatModeCommand().setEnabled(false);
        center.changeShuffleModeCommand().setEnabled(false);
    }
}

fn add_command(command: &MPRemoteCommand, controls: &Controls, action: fn(&Controls)) {
    let controls = controls.clone();
    let handler = RcBlock::new(move |_event: NonNull<MPRemoteCommandEvent>| {
        action(&controls);
        MPRemoteCommandHandlerStatus::Success
    });
    unsafe { command.addTargetWithHandler(&handler) };
}
