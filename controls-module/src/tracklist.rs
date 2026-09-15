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
    #[serde(default)]
    next_queue_id: u64,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PlayingEntity {
    Track(Track),
    Playlist(PlayingPlaylist),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
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
    #[serde(skip)]
    pub connect_id: Option<i32>,
}

impl Tracklist {
    #[must_use]
    pub const fn new(list_type: TracklistType, queue: Vec<QueueItem>) -> Self {
        Self {
            queue,
            list_type,
            next_queue_id: 0,
        }
    }

    pub fn set_list_type(&mut self, list_type: TracklistType) {
        self.list_type = list_type;
    }

    #[must_use]
    pub const fn new_with_id(list_type: TracklistType, items: Vec<QueueItem>) -> Self {
        Self {
            queue: items,
            list_type,
            next_queue_id: 0,
        }
    }

    #[must_use]
    pub fn queue(&self) -> Vec<&QueueItem> {
        self.queue.iter().collect()
    }

    pub fn clear_queue(&mut self) {
        self.queue
            .retain(|x| x.track.status != TrackStatus::Unplayed);
    }

    #[must_use]
    pub const fn total(&self) -> usize {
        self.queue.len()
    }

    #[must_use]
    pub fn currently_playing(&self) -> Option<u32> {
        self.queue
            .iter()
            .find(|t| t.track.status == TrackStatus::Playing)
            .map(|x| x.track.id)
    }

    #[must_use]
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

    #[must_use]
    pub fn next_track_id(&self) -> Option<u32> {
        self.next_track().map(|x| x.id)
    }

    pub fn remove_track(&mut self, index: usize) {
        if index < self.queue.len() {
            self.queue.remove(index);
        }
    }

    pub fn push_track(&mut self, track: Track) {
        let item = self.queue_item(track, None);
        self.queue.push(item);
    }

    pub fn insert_track(&mut self, insert_index: usize, track: Track) {
        let item = self.queue_item(track, None);
        self.queue.insert(insert_index.min(self.queue.len()), item);
    }

    /// A new item with the next free queue id, not yet in the queue.
    pub fn queue_item(&mut self, track: Track, connect_id: Option<i32>) -> QueueItem {
        let index = self.total().checked_add(1).unwrap_or_default();
        let highest = self.queue.iter().map(|item| item.queue_id).max();
        let queue_id = highest
            .map_or(0, |id| id.saturating_add(1))
            .max(self.next_queue_id);
        self.next_queue_id = queue_id.saturating_add(1);
        QueueItem {
            track,
            queue_id,
            index,
            connect_id,
        }
    }

    /// Adopts the queue of the Qobuz Connect session, keeping the current track and the items the session does not know yet in place. Returns whether the current track is still queued.
    pub fn replace(&mut self, items: Vec<QueueItem>) -> bool {
        let current = self.current_queue_id();
        let mut queue = items;
        let mut previous = None;
        for item in self.queue.drain(..) {
            if item.connect_id.is_none() {
                let at = previous
                    .and_then(|id| queue.iter().position(|known| known.queue_id == id))
                    .map_or(0, |position| position.saturating_add(1));
                queue.insert(at.min(queue.len()), item.clone());
            }
            previous = Some(item.queue_id);
        }
        self.queue = queue;
        let position =
            current.and_then(|id| self.queue.iter().position(|item| item.queue_id == id));
        if let Some(position) = position {
            self.skip_to_track(position);
            return true;
        }
        self.reset();
        false
    }

    pub fn set_connect_ids(&mut self, ids: &[(u64, i32)]) {
        for item in &mut self.queue {
            if let Some((_, connect_id)) =
                ids.iter().find(|(queue_id, _)| *queue_id == item.queue_id)
            {
                item.connect_id = Some(*connect_id);
            }
        }
    }

    #[must_use]
    pub fn position_of_connect_id(&self, connect_id: i32) -> Option<usize> {
        self.queue
            .iter()
            .position(|item| item.connect_id == Some(connect_id))
    }

    #[must_use]
    pub fn current_connect_id(&self) -> Option<i32> {
        self.queue
            .iter()
            .find(|item| item.track.status == TrackStatus::Playing)
            .and_then(|item| item.connect_id)
    }

    #[must_use]
    pub fn next_connect_id(&self) -> Option<i32> {
        let next = self.current_position().checked_add(1)?;
        self.queue.get(next).and_then(|item| item.connect_id)
    }

    pub fn reorder_queue(&mut self, new_order: &[usize]) {
        if new_order.len() != self.queue.len() || new_order.iter().enumerate().all(|(i, &v)| i == v)
        {
            return;
        }

        let reordered: Vec<_> = new_order
            .iter()
            .filter_map(|&i| self.queue.get(i).cloned())
            .collect();

        self.queue = reordered;
    }

    #[must_use]
    pub fn current_position(&self) -> usize {
        self.queue
            .iter()
            .enumerate()
            .find(|t| t.1.track.status == TrackStatus::Playing)
            .map_or(0, |x| x.0)
    }

    #[must_use]
    pub fn current_queue_id(&self) -> Option<u64> {
        self.queue
            .iter()
            .find(|t| t.track.status == TrackStatus::Playing)
            .map(|x| x.queue_id)
    }

    #[must_use]
    pub fn next_track_queue_id(&self) -> Option<u64> {
        let current_position = self.current_position();

        if current_position >= self.total() {
            return None;
        }

        let next_position = current_position.checked_add(1)?;

        let next = self.queue.get(next_position);
        next.map(|x| x.queue_id)
    }

    #[must_use]
    pub const fn list_type(&self) -> &TracklistType {
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

    #[must_use]
    pub fn next_track(&self) -> Option<&Track> {
        let current_position = self.current_position();
        let next_position = current_position.checked_add(1)?;

        if self.total() <= next_position {
            return None;
        }

        Some(&self.queue.index(next_position).track)
    }

    #[must_use]
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

                    new_track = Some(queue_item.1);
                }

                std::cmp::Ordering::Greater => {
                    queue_item.1.status = TrackStatus::Unplayed;
                }
            }
        }

        new_track
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracklist(len: usize) -> Tracklist {
        Tracklist::new(TracklistType::Tracks, vec![QueueItem::default(); len])
    }

    #[test]
    fn remove_track_ignores_out_of_range_index() {
        let mut tracklist = tracklist(7);
        tracklist.remove_track(7);
        assert_eq!(tracklist.total(), 7);
    }

    #[test]
    fn insert_track_beyond_end_appends() {
        let mut tracklist = tracklist(0);
        tracklist.insert_track(1, Track::default());
        assert_eq!(tracklist.total(), 1);
    }

    #[test]
    fn reorder_queue_ignores_order_of_wrong_length() {
        let mut tracklist = tracklist(3);
        tracklist.reorder_queue(&[1, 0]);
        assert_eq!(tracklist.total(), 3);
    }

    fn queue_with_ids(ids: &[u64]) -> Tracklist {
        let items = ids
            .iter()
            .map(|&queue_id| QueueItem {
                queue_id,
                ..QueueItem::default()
            })
            .collect();
        Tracklist::new(TracklistType::Tracks, items)
    }

    fn queue_id_at(tracklist: &Tracklist, index: usize) -> Option<u64> {
        tracklist.queue().get(index).map(|item| item.queue_id)
    }

    #[test]
    fn queue_ids_are_never_reused() {
        let mut tracklist = queue_with_ids(&[0, 1, 2, 3, 4, 5, 6, 7]);
        tracklist.insert_track(1, Track::default());
        assert_eq!(queue_id_at(&tracklist, 1), Some(8));
        tracklist.remove_track(1);
        tracklist.insert_track(1, Track::default());
        assert_eq!(queue_id_at(&tracklist, 1), Some(9));
        tracklist.push_track(Track::default());
        assert_eq!(queue_id_at(&tracklist, 9), Some(10));
    }

    #[test]
    fn queue_ids_continue_after_the_highest_id_of_a_received_queue() {
        let mut tracklist = queue_with_ids(&[0, 5, 1, 2, 3, 4]);
        tracklist.push_track(Track::default());
        assert_eq!(queue_id_at(&tracklist, 6), Some(6));
    }

    #[test]
    fn a_stored_tracklist_without_a_counter_still_loads() {
        let stored = r#"{"queue":[],"list_type":"Tracks"}"#;
        assert!(serde_json::from_str::<Tracklist>(stored).is_ok());
    }

    fn connect_queue(ids: &[(u64, i32)]) -> Tracklist {
        let items = ids
            .iter()
            .map(|&(queue_id, connect_id)| QueueItem {
                queue_id,
                connect_id: Some(connect_id),
                ..QueueItem::default()
            })
            .collect();
        Tracklist::new(TracklistType::Tracks, items)
    }

    fn ids(tracklist: &Tracklist) -> Vec<u64> {
        tracklist.queue().iter().map(|item| item.queue_id).collect()
    }

    #[test]
    fn replace_keeps_the_current_track_and_the_items_the_session_does_not_know() {
        let mut tracklist = connect_queue(&[(0, 10), (1, 11), (2, 12)]);
        tracklist.skip_to_track(1);
        tracklist.insert_track(2, Track::default());
        assert_eq!(ids(&tracklist), vec![0, 1, 3, 2]);

        let from_session = connect_queue(&[(1, 11), (2, 12), (4, 13)]);
        assert!(tracklist.replace(from_session.queue().into_iter().cloned().collect()));
        assert_eq!(ids(&tracklist), vec![1, 3, 2, 4]);
        assert_eq!(tracklist.current_position(), 0);
        assert_eq!(tracklist.current_connect_id(), Some(11));
        assert_eq!(tracklist.next_connect_id(), None);
    }

    #[test]
    fn replace_reports_a_current_track_that_the_session_dropped() {
        let mut tracklist = connect_queue(&[(0, 10), (1, 11)]);
        tracklist.skip_to_track(0);
        let from_session = connect_queue(&[(1, 11)]);
        assert!(!tracklist.replace(from_session.queue().into_iter().cloned().collect()));
        assert_eq!(tracklist.current_position(), 0);
        assert_eq!(tracklist.current_connect_id(), Some(11));
    }

    #[test]
    fn connect_ids_are_set_by_queue_id() {
        let mut tracklist = queue_with_ids(&[0, 1, 2]);
        tracklist.set_connect_ids(&[(1, 7), (2, 8)]);
        assert_eq!(tracklist.position_of_connect_id(8), Some(2));
        assert_eq!(tracklist.position_of_connect_id(7), Some(1));
        assert_eq!(tracklist.position_of_connect_id(9), None);
    }
}
