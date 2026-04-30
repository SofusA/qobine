use std::time::Duration;

use mpris_server::{
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property, RootInterface,
    Server, Time, TrackId, Volume,
    zbus::{self, fdo},
};
use qobuz_player_controls::{
    AppResult, ExitSender, PositionReceiver, Status, StatusReceiver, TracklistReceiver,
    VolumeReceiver, controls::Controls, error::Error, models::Track,
};

struct MprisPlayer {
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    volume_receiver: VolumeReceiver,
    status_receiver: StatusReceiver,
    exit_sender: ExitSender,
    mpris_suffix: String,
}

impl RootInterface for MprisPlayer {
    async fn identity(&self) -> fdo::Result<String> {
        Ok(self.mpris_suffix.clone())
    }
    async fn raise(&self) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("Not supported".into()))
    }
    async fn quit(&self) -> fdo::Result<()> {
        match self.exit_sender.send(true) {
            Ok(_) => Ok(()),
            Err(_) => Err(fdo::Error::Failed("Unable to send exit signal".into())),
        }
    }
    async fn can_quit(&self) -> fdo::Result<bool> {
        Ok(true)
    }
    async fn fullscreen(&self) -> fdo::Result<bool> {
        Err(fdo::Error::NotSupported("Not supported".into()))
    }
    async fn set_fullscreen(&self, _fullscreen: bool) -> zbus::Result<()> {
        Err(zbus::Error::Unsupported)
    }
    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn can_raise(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok(self.mpris_suffix.clone())
    }
    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        Ok(vec![])
    }
    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(vec![])
    }
}

impl PlayerInterface for MprisPlayer {
    async fn next(&self) -> fdo::Result<()> {
        self.controls.next();
        Ok(())
    }

    async fn previous(&self) -> fdo::Result<()> {
        self.controls.previous();
        Ok(())
    }

    async fn pause(&self) -> fdo::Result<()> {
        self.controls.pause();
        Ok(())
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        self.controls.play_pause();
        Ok(())
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.controls.pause();
        Ok(())
    }

    async fn play(&self) -> fdo::Result<()> {
        self.controls.play();
        Ok(())
    }

    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        let current_position = *self.position_receiver.borrow();
        let offset_millis = offset.as_millis();

        let new_position = match offset_millis < 0 {
            true => {
                current_position - Duration::from_millis(offset_millis.abs().try_into().unwrap())
            }
            false => {
                current_position + Duration::from_millis(offset_millis.abs().try_into().unwrap())
            }
        };

        self.controls.seek(new_position);
        Ok(())
    }

    async fn set_position(&self, _track_id: TrackId, _position: Time) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("Not supported".into()))
    }

    async fn open_uri(&self, _uri: String) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("Not supported".into()))
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        let status = match *self.status_receiver.borrow() {
            Status::Paused | Status::Buffering => PlaybackStatus::Paused,
            Status::Playing => PlaybackStatus::Playing,
        };

        Ok(status)
    }

    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        Err(fdo::Error::NotSupported("Not supported".into()))
    }

    async fn set_loop_status(&self, _loop_status: LoopStatus) -> zbus::Result<()> {
        Err(zbus::Error::Unsupported)
    }

    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn set_rate(&self, _rate: PlaybackRate) -> zbus::Result<()> {
        Err(zbus::Error::Unsupported)
    }

    async fn shuffle(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn set_shuffle(&self, _shuffle: bool) -> zbus::Result<()> {
        Err(zbus::Error::Unsupported)
    }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        let tracklist = self.tracklist_receiver.borrow();
        let current_track = tracklist.current_track();

        if let Some(current_track) = current_track {
            return Ok(track_to_metadata(current_track));
        };

        Ok(Metadata::new())
    }

    async fn volume(&self) -> fdo::Result<Volume> {
        let volume = self.volume_receiver.borrow();
        Ok(*volume as f64)
    }

    async fn set_volume(&self, volume: Volume) -> zbus::Result<()> {
        self.controls.set_volume(volume as f32);
        Ok(())
    }

    async fn position(&self) -> fdo::Result<Time> {
        let position_millis = self.position_receiver.borrow().as_millis();
        let time = Time::from_millis(position_millis as i64);
        Ok(time)
    }

    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn can_go_next(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_go_previous(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_play(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_pause(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_seek(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_control(&self) -> fdo::Result<bool> {
        Ok(true)
    }
}

pub async fn init(
    position_receiver: PositionReceiver,
    mut tracklist_receiver: TracklistReceiver,
    mut volume_receiver: VolumeReceiver,
    mut status_receiver: StatusReceiver,
    controls: Controls,
    exit_sender: ExitSender,
    mpris_suffix: String,
) -> AppResult<()> {
    let mut exit_receiver = exit_sender.subscribe();

    let Ok(server) = Server::new(
        &mpris_suffix.clone(),
        MprisPlayer {
            controls,
            position_receiver,
            tracklist_receiver: tracklist_receiver.clone(),
            volume_receiver: volume_receiver.clone(),
            status_receiver: status_receiver.clone(),
            exit_sender,
            mpris_suffix,
        },
    )
    .await
    else {
        return Err(Error::MprisInitError);
    };

    loop {
        tokio::select! {
            Ok(_) = tracklist_receiver.changed() => {
                let tracklist = tracklist_receiver.borrow_and_update().clone();
                let current_track = tracklist.current_track();

                if let Some(current_track) = current_track {
                    let metadata = track_to_metadata(current_track);

                    let current_position = tracklist.current_position();
                    let total_tracks = tracklist.total();

                    let can_previous = current_position != 0;
                    let can_next = !(total_tracks != 0 && current_position == total_tracks - 1);

                    let Ok(_) = server
                        .properties_changed([
                            Property::Metadata(metadata),
                            Property::CanGoPrevious(can_previous),
                            Property::CanGoNext(can_next),
                        ])
                        .await else {
                            return Err(Error::MprisPropertyError { property: "Metadata, CanGoPrevious, CanGoNext".into() });
                        };
                }
            },
            Ok(_) = volume_receiver.changed() => {
                let volume = *volume_receiver.borrow_and_update();
                let Ok(_) = server
                    .properties_changed([Property::Volume(volume.into())])
                    .await else {
                        return Err(Error::MprisPropertyError { property: "Volume".into() });
                    };
            },
            Ok(_) = status_receiver.changed() => {
                let status = *status_receiver.borrow_and_update();
                let (can_play, can_pause) = match status {
                    Status::Buffering => (false, false),
                    Status::Paused => (true, true),
                    Status::Playing => (true, true),
                };

                let playback_status = match status {
                    Status::Paused | Status::Buffering => PlaybackStatus::Paused,
                    Status::Playing => PlaybackStatus::Playing,
                };

                    let Ok(_) = server
                    .properties_changed([
                        Property::CanPlay(can_play),
                        Property::CanPause(can_pause),
                        Property::PlaybackStatus(playback_status),
                    ])
                    .await else {
                        return Err(Error::MprisPropertyError { property: "CanPlay, CanPause, PlaybackStatus".into() });
                    };
            },
            Ok(exit) = exit_receiver.recv() => {
                if exit {
                    break Ok(());
                }
            }
        }
    }
}

fn track_to_metadata(track: &Track) -> Metadata {
    let mut metadata = Metadata::new();
    let duration = mpris_server::Time::from_secs(track.duration_seconds as i64);
    metadata.set_length(Some(duration));

    metadata.set_album(track.album_title.clone());
    metadata.set_art_url(track.image.clone());

    // artist
    let artist_name = track.artist_name.clone();

    metadata.set_artist(artist_name.as_ref().map(|a| vec![a]));
    metadata.set_album_artist(artist_name.as_ref().map(|a| vec![a]));

    // track
    metadata.set_title(Some(track.title.clone()));
    metadata.set_track_number(Some(track.number as i32));

    metadata
}
