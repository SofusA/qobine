use std::ops::Index;

use crate::models::{Track, TrackStatus};

#[derive(Default, Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct AlbumTracklist {
    pub title: String,
    pub id: String,
    pub image: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct PlaylistTracklist {
    pub title: String,
    pub id: u32,
    pub image: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct TopTracklist {
    pub artist_name: String,
    pub id: u32,
    pub image: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum TracklistType {
    Album(AlbumTracklist),
    Playlist(PlaylistTracklist),
    TopTracks(TopTracklist),
    #[default]
    Tracks,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Tracklist {
    queue: Vec<QueueItem>,
    list_type: TracklistType,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PlayingEntity {
    Track(Track),
    Playlist(PlayingPlaylist),
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PlayingPlaylist {
    pub track_id: u32,
    pub queue_id: u64,
    pub index: usize,
    pub playlist_id: u32,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct QueueItem {
    pub track: Track,
    pub queue_id: u64,
    pub index: usize,
}

impl Tracklist {
    pub fn new(list_type: TracklistType, queue: Vec<QueueItem>) -> Self {
        Self { queue, list_type }
    }

    pub fn set_list_type(&mut self, list_type: TracklistType) {
        self.list_type = list_type
    }

    pub fn new_with_id(list_type: TracklistType, items: Vec<QueueItem>) -> Self {
        Self {
            queue: items,
            list_type,
        }
    }

    pub fn queue(&self) -> Vec<&QueueItem> {
        self.queue.iter().collect()
    }

    pub fn total(&self) -> usize {
        self.queue.len()
    }

    pub fn currently_playing(&self) -> Option<u32> {
        self.queue
            .iter()
            .find(|t| t.track.status == TrackStatus::Playing)
            .map(|x| x.track.id)
    }

    pub fn current_playing_entity(&self) -> Option<PlayingEntity> {
        let current_queue_item = self
            .queue
            .iter()
            .find(|q| q.track.status == TrackStatus::Playing);

        current_queue_item.map(|queue_item| match &self.list_type {
            TracklistType::Playlist(playlist_tracklist) => {
                PlayingEntity::Playlist(PlayingPlaylist {
                    track_id: queue_item.track.id,
                    queue_id: queue_item.queue_id,
                    index: queue_item.index,
                    playlist_id: playlist_tracklist.id,
                })
            }
            _ => PlayingEntity::Track(queue_item.track.clone()),
        })
    }

    pub fn next_track_id(&self) -> Option<u32> {
        self.next_track().map(|x| x.id)
    }

    pub fn remove_track(&mut self, index: usize) {
        self.queue.remove(index);
    }

    pub fn push_track(&mut self, track: Track) {
        let id = self.total() + 1;
        let item = QueueItem {
            track,
            queue_id: id as u64,
            index: id,
        };
        self.queue.push(item);
    }

    pub fn insert_track(&mut self, index: usize, track: Track) {
        let id = self.total() + 1;
        let item = QueueItem {
            track,
            queue_id: id as u64,
            index: id,
        };
        self.queue.insert(index, item);
    }

    pub fn reorder_queue(&mut self, new_order: Vec<usize>) {
        if new_order.iter().enumerate().all(|(i, &v)| i == v) {
            return;
        }

        let reordered: Vec<_> = new_order.iter().map(|&i| self.queue[i].clone()).collect();

        self.queue = reordered;
    }

    pub fn current_position(&self) -> usize {
        self.queue
            .iter()
            .enumerate()
            .find(|t| t.1.track.status == TrackStatus::Playing)
            .map(|x| x.0)
            .unwrap_or(0)
    }

    pub fn current_queue_id(&self) -> Option<u64> {
        self.queue
            .iter()
            .find(|t| t.track.status == TrackStatus::Playing)
            .map(|x| x.queue_id)
    }

    pub fn next_track_queue_id(&self) -> Option<u64> {
        let current = self.current_position();

        if current >= self.total() {
            return None;
        }

        let next = self.queue.get(current + 1);
        next.map(|x| x.queue_id)
    }

    pub fn list_type(&self) -> &TracklistType {
        &self.list_type
    }

    pub fn reset(&mut self) {
        for track in self.queue.iter_mut().map(|x| &mut x.track) {
            if track.status == TrackStatus::Played || track.status == TrackStatus::Playing {
                track.status = TrackStatus::Unplayed;
            }
        }

        if let Some(first_item) = self
            .queue
            .iter_mut()
            .find(|t| t.track.status == TrackStatus::Unplayed)
        {
            first_item.track.status = TrackStatus::Playing;
        }
    }

    pub fn next_track(&self) -> Option<&Track> {
        let current_position = self.current_position();
        let next_position = current_position + 1;
        if self.total() <= next_position {
            return None;
        }

        Some(&self.queue.index(next_position).track)
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.queue
            .iter()
            .map(|x| &x.track)
            .find(|t| t.status == TrackStatus::Playing)
    }

    pub fn skip_to_track(&mut self, new_position: usize) -> Option<&Track> {
        let mut new_track: Option<&Track> = None;

        for queue_item in self.queue.iter_mut().map(|x| &mut x.track).enumerate() {
            let queue_item_position = queue_item.0;

            match queue_item_position.cmp(&new_position) {
                std::cmp::Ordering::Less => {
                    queue_item.1.status = TrackStatus::Played;
                }

                std::cmp::Ordering::Equal => {
                    queue_item.1.status = TrackStatus::Playing;

                    new_track = Some(queue_item.1)
                }

                std::cmp::Ordering::Greater => {
                    queue_item.1.status = TrackStatus::Unplayed;
                }
            }
        }

        new_track
    }
}
