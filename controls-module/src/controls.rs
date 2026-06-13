use std::{path::PathBuf, time::Duration};

use qobuz_client::client::AudioQuality;
use tokio::sync::broadcast;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
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
    SetAutoPlay {
        enable: bool,
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

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum StreamingConfiguration {
    SetMaxAudioQuality { new_quality: AudioQuality },
    SetAudioCacheDirectory { new_directory: PathBuf },
    UseFileBasedStreaming { use_file_based_streaming: bool },
}

#[derive(Debug, Clone)]
pub struct Controls {
    tx: broadcast::Sender<ControlCommand>,
}

impl Default for Controls {
    fn default() -> Self {
        Self::new()
    }
}

impl Controls {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(20);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ControlCommand> {
        self.tx.subscribe()
    }

    pub fn send(&self, command: ControlCommand) {
        self.tx.send(command).expect("infallible");
    }

    pub fn next(&self) {
        self.send(ControlCommand::Next);
    }

    pub fn previous(&self) {
        self.send(ControlCommand::Previous);
    }

    pub fn play_pause(&self) {
        self.send(ControlCommand::PlayPause);
    }

    pub fn play(&self) {
        self.send(ControlCommand::Play);
    }

    pub fn pause(&self) {
        self.send(ControlCommand::Pause);
    }

    pub fn play_album(&self, id: &str, index: usize) {
        self.send(ControlCommand::Album {
            id: id.to_string(),
            index,
        });
    }

    pub fn play_playlist(&self, id: u32, index: usize, shuffle: bool) {
        self.send(ControlCommand::Playlist { id, index, shuffle });
    }

    pub fn play_track(&self, id: u32) {
        self.send(ControlCommand::Track { id });
    }

    pub fn play_tracks(&self, ids: Vec<u32>, shuffle: bool) {
        self.send(ControlCommand::Tracks { ids, shuffle });
    }

    pub fn add_tracks_to_queue(&self, ids: Vec<u32>) {
        self.send(ControlCommand::AddTracksToQueue { ids });
    }

    pub fn remove_index_from_queue(&self, index: usize) {
        self.send(ControlCommand::RemoveIndexFromQueue { index });
    }

    pub fn play_tracks_next(&self, ids: Vec<u32>) {
        self.send(ControlCommand::PlayTracksNext { ids });
    }

    pub fn play_top_tracks(&self, artist_id: u32, index: usize) {
        self.send(ControlCommand::ArtistTopTracks { artist_id, index });
    }

    pub fn skip_to_position(&self, index: usize, force: bool) {
        self.send(ControlCommand::SkipToPosition {
            new_position: index,
            force,
        });
    }

    pub fn set_volume(&self, volume: f32) {
        self.send(ControlCommand::SetVolume { volume });
    }

    pub fn set_auto_play(&self, enable: bool) {
        self.send(ControlCommand::SetAutoPlay { enable });
    }

    pub fn seek(&self, time: Duration) {
        self.send(ControlCommand::Seek { time });
    }

    pub fn jump_forward(&self) {
        self.send(ControlCommand::JumpForward);
    }

    pub fn jump_backward(&self) {
        self.send(ControlCommand::JumpBackward);
    }

    pub fn reorder_queue(&self, new_order: Vec<usize>) {
        self.send(ControlCommand::ReorderQueue { new_order });
    }

    pub fn new_queue(&self, items: Vec<NewQueueItem>, play: bool, start_index: Option<usize>) {
        self.send(ControlCommand::NewQueue {
            items,
            play,
            start_index,
        });
    }

    pub fn clear_queue(&self) {
        self.send(ControlCommand::ClearQueue);
    }

    pub fn set_audio_max_quality(&self, new_quality: AudioQuality) {
        self.send(ControlCommand::StreamingConfiguration {
            configuration: StreamingConfiguration::SetMaxAudioQuality { new_quality },
        });
    }

    pub fn set_use_file_based_streaming(&self, use_file_based_streaming: bool) {
        self.send(ControlCommand::StreamingConfiguration {
            configuration: StreamingConfiguration::UseFileBasedStreaming {
                use_file_based_streaming,
            },
        });
    }

    pub fn set_audio_cache_directory(&self, new_directory: PathBuf) {
        self.send(ControlCommand::StreamingConfiguration {
            configuration: StreamingConfiguration::SetAudioCacheDirectory { new_directory },
        });
    }
}

#[derive(Debug, Copy, Clone, serde::Deserialize, serde::Serialize)]
pub struct NewQueueItem {
    pub track_id: u32,
    pub queue_id: u64,
}
