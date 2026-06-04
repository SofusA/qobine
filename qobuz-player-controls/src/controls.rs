use std::{path::PathBuf, time::Duration};

use qobuz_player_client::client::AudioQuality;

#[derive(Debug)]
pub enum ControlCommand {
    Album {
        id: String,
        index: usize,
    },
    Playlist {
        id: u32,
        index: usize,
        shuffle: bool,
    },
    ArtistTopTracks {
        artist_id: u32,
        index: usize,
    },
    Tracks {
        ids: Vec<u32>,
        shuffle: bool,
        start_index: usize,
    },
    Track {
        id: u32,
    },
    SkipToPosition {
        new_position: usize,
        force: bool,
    },
    Next,
    Previous,
    PlayPause,
    Play,
    Pause,
    JumpForward,
    JumpBackward,
    Seek {
        time: Duration,
    },
    SetVolume {
        volume: f32,
    },
    AddTracksToQueue {
        ids: Vec<u32>,
    },
    RemoveIndexFromQueue {
        index: usize,
    },
    PlayTracksNext {
        ids: Vec<u32>,
    },
    ReorderQueue {
        new_order: Vec<usize>,
    },
    NewQueue {
        items: Vec<NewQueueItem>,
        play: bool,
        start_index: Option<usize>,
    },
    ClearQueue,
    StreamingConfiguration {
        configuration: StreamingConfiguration,
    },
}

#[derive(Debug)]
pub enum StreamingConfiguration {
    SetMaxAudioQuality { new_quality: AudioQuality },
    SetAudioCacheDirectory { new_directory: PathBuf },
    UseFileBasedStreaming { use_file_based_streaming: bool },
}

#[derive(Debug, Clone)]
pub struct Controls {
    tx: tokio::sync::mpsc::UnboundedSender<ControlCommand>,
}

impl Controls {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<ControlCommand>) -> Self {
        Self { tx }
    }

    pub fn next(&self) {
        self.tx.send(ControlCommand::Next).expect("infallible");
    }

    pub fn previous(&self) {
        self.tx.send(ControlCommand::Previous).expect("infallible");
    }

    pub fn play_pause(&self) {
        self.tx.send(ControlCommand::PlayPause).expect("infallible");
    }

    pub fn play(&self) {
        self.tx.send(ControlCommand::Play).expect("infallible");
    }

    pub fn pause(&self) {
        self.tx.send(ControlCommand::Pause).expect("infallible");
    }

    pub fn play_album(&self, id: &str, index: usize) {
        self.tx
            .send(ControlCommand::Album {
                id: id.to_string(),
                index,
            })
            .expect("infallible");
    }

    pub fn play_playlist(&self, id: u32, index: usize, shuffle: bool) {
        self.tx
            .send(ControlCommand::Playlist { id, index, shuffle })
            .expect("infallible");
    }

    pub fn play_track(&self, id: u32) {
        self.tx
            .send(ControlCommand::Track { id })
            .expect("infallible");
    }

    pub fn play_tracks(&self, ids: Vec<u32>, shuffle: bool, start_index: usize) {
        self.tx
            .send(ControlCommand::Tracks {
                ids,
                shuffle,
                start_index,
            })
            .expect("infallible");
    }

    pub fn add_tracks_to_queue(&self, ids: Vec<u32>) {
        self.tx
            .send(ControlCommand::AddTracksToQueue { ids })
            .expect("infallible");
    }

    pub fn remove_index_from_queue(&self, index: usize) {
        self.tx
            .send(ControlCommand::RemoveIndexFromQueue { index })
            .expect("infallible");
    }

    pub fn play_tracks_next(&self, ids: Vec<u32>) {
        self.tx
            .send(ControlCommand::PlayTracksNext { ids })
            .expect("infallible");
    }

    pub fn play_top_tracks(&self, artist_id: u32, index: usize) {
        self.tx
            .send(ControlCommand::ArtistTopTracks { artist_id, index })
            .expect("infallible");
    }

    pub fn skip_to_position(&self, index: usize, force: bool) {
        self.tx
            .send(ControlCommand::SkipToPosition {
                new_position: index,
                force,
            })
            .expect("infallible");
    }

    pub fn set_volume(&self, volume: f32) {
        self.tx
            .send(ControlCommand::SetVolume { volume })
            .expect("infallible");
    }

    pub fn seek(&self, time: Duration) {
        self.tx
            .send(ControlCommand::Seek { time })
            .expect("infallible");
    }

    pub fn jump_forward(&self) {
        self.tx
            .send(ControlCommand::JumpForward)
            .expect("infallible");
    }

    pub fn jump_backward(&self) {
        self.tx
            .send(ControlCommand::JumpBackward)
            .expect("infallible");
    }

    pub fn reorder_queue(&self, new_order: Vec<usize>) {
        self.tx
            .send(ControlCommand::ReorderQueue { new_order })
            .expect("infallible");
    }

    pub fn new_queue(&self, items: Vec<NewQueueItem>, play: bool, start_index: Option<usize>) {
        self.tx
            .send(ControlCommand::NewQueue {
                items,
                play,
                start_index,
            })
            .expect("infallible");
    }

    pub fn clear_queue(&self) {
        self.tx
            .send(ControlCommand::ClearQueue)
            .expect("infallible");
    }

    pub fn set_audio_max_quality(&self, new_quality: AudioQuality) {
        self.tx
            .send(ControlCommand::StreamingConfiguration {
                configuration: StreamingConfiguration::SetMaxAudioQuality { new_quality },
            })
            .expect("infallible");
    }

    pub fn set_use_file_based_streaming(&self, use_file_based_streaming: bool) {
        self.tx
            .send(ControlCommand::StreamingConfiguration {
                configuration: StreamingConfiguration::UseFileBasedStreaming {
                    use_file_based_streaming,
                },
            })
            .expect("infallible");
    }

    pub fn set_audio_cache_directory(&self, new_directory: PathBuf) {
        self.tx
            .send(ControlCommand::StreamingConfiguration {
                configuration: StreamingConfiguration::SetAudioCacheDirectory { new_directory },
            })
            .expect("infallible");
    }
}

#[derive(Debug, Copy, Clone)]
pub struct NewQueueItem {
    pub track_id: u32,
    pub queue_id: u64,
}
