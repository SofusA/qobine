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

#[derive(Default, Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
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
            .retain(|item| item.track.status != TrackStatus::Unplayed);
    }

    #[must_use]
    pub const fn total(&self) -> usize {
        self.queue.len()
    }

    /// Returns the queue item that is currently playing.
    #[must_use]
    pub fn current_queue_item(&self) -> Option<&QueueItem> {
        self.queue
            .iter()
            .find(|item| item.track.status == TrackStatus::Playing)
    }

    /// Returns the queue item immediately after the currently playing item.
    ///
    /// Returns `None` if nothing is playing or the current item is the final
    /// item in the queue.
    #[must_use]
    pub fn next_queue_item(&self) -> Option<&QueueItem> {
        let current_queue_id = self.current_queue_item()?.queue_id;

        let current_position = self
            .queue
            .iter()
            .position(|item| item.queue_id == current_queue_id)?;

        let next_position = current_position.checked_add(1)?;

        self.queue.get(next_position)
    }

    #[must_use]
    pub fn currently_playing(&self) -> Option<u32> {
        self.current_queue_item().map(|item| item.track.id)
    }

    #[must_use]
    pub fn current_playing_entity(&self) -> Option<PlayingEntity> {
        let queue_item = self.current_queue_item()?;

        Some(match &self.list_type {
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
        self.next_queue_item().map(|item| item.track.id)
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

    /// Creates a new item with the next free queue ID.
    ///
    /// The item is not added to the queue by this function.
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

    /// Adopts the queue of the Qobuz Connect session, keeping the current track.
    ///
    /// With `keep_unknown`, items the session does not know yet stay after
    /// their nearest surviving predecessor.
    ///
    /// Returns whether the current track is still queued.
    pub fn replace(&mut self, items: Vec<QueueItem>, keep_unknown: bool) -> bool {
        let current_queue_id = self.current_queue_item().map(|item| item.queue_id);

        let mut queue = items;
        let mut previous = None;

        for item in self.queue.drain(..) {
            let present = queue.iter().any(|known| known.queue_id == item.queue_id);

            if !present && keep_unknown && item.connect_id.is_none() {
                let at = previous
                    .and_then(|id| queue.iter().position(|known| known.queue_id == id))
                    .map_or(0, |position| position.saturating_add(1));

                queue.insert(at.min(queue.len()), item.clone());
            }

            if present || (keep_unknown && item.connect_id.is_none()) {
                previous = Some(item.queue_id);
            }
        }

        self.queue = queue;

        let position = current_queue_id
            .and_then(|queue_id| self.queue.iter().position(|item| item.queue_id == queue_id));

        if let Some(position) = position {
            self.skip_to_track(position);
            return true;
        }

        self.reset();
        false
    }

    #[must_use]
    pub fn position_of_connect_id(&self, connect_id: i32) -> Option<usize> {
        self.queue
            .iter()
            .position(|item| item.connect_id == Some(connect_id))
    }

    #[must_use]
    pub fn current_connect_id(&self) -> Option<i32> {
        self.current_queue_item().and_then(|item| item.connect_id)
    }

    #[must_use]
    pub fn next_connect_id(&self) -> Option<i32> {
        self.next_queue_item().and_then(|item| item.connect_id)
    }

    pub fn reorder_queue(&mut self, new_order: &[usize]) {
        if new_order.len() != self.queue.len()
            || new_order
                .iter()
                .enumerate()
                .all(|(index, &value)| index == value)
        {
            return;
        }

        let reordered: Vec<_> = new_order
            .iter()
            .filter_map(|&index| self.queue.get(index).cloned())
            .collect();

        self.queue = reordered;
    }

    /// Returns the current queue position.
    ///
    /// Returns `0` if no item is currently playing.
    #[must_use]
    pub fn current_position(&self) -> usize {
        self.queue
            .iter()
            .position(|item| item.track.status == TrackStatus::Playing)
            .unwrap_or_default()
    }

    #[must_use]
    pub fn current_queue_id(&self) -> Option<u64> {
        self.current_queue_item().map(|item| item.queue_id)
    }

    #[must_use]
    pub fn next_track_queue_id(&self) -> Option<u64> {
        self.next_queue_item().map(|item| item.queue_id)
    }

    #[must_use]
    pub const fn list_type(&self) -> &TracklistType {
        &self.list_type
    }

    pub fn reset(&mut self) {
        for track in self.queue.iter_mut().map(|item| &mut item.track) {
            if track.status == TrackStatus::Played || track.status == TrackStatus::Playing {
                track.status = TrackStatus::Unplayed;
            }
        }

        if let Some(first_item) = self
            .queue
            .iter_mut()
            .find(|item| item.track.status == TrackStatus::Unplayed)
        {
            first_item.track.status = TrackStatus::Playing;
        }
    }

    #[must_use]
    pub fn next_track(&self) -> Option<&Track> {
        self.next_queue_item().map(|item| &item.track)
    }

    #[must_use]
    pub fn current_track(&self) -> Option<&Track> {
        self.current_queue_item().map(|item| &item.track)
    }

    pub fn skip_to_track(&mut self, new_position: usize) -> Option<&QueueItem> {
        let mut new_item = None;

        for (position, queue_item) in self.queue.iter_mut().enumerate() {
            match position.cmp(&new_position) {
                std::cmp::Ordering::Less => {
                    queue_item.track.status = TrackStatus::Played;
                }
                std::cmp::Ordering::Equal => {
                    queue_item.track.status = TrackStatus::Playing;
                    new_item = Some(&*queue_item);
                }
                std::cmp::Ordering::Greater => {
                    queue_item.track.status = TrackStatus::Unplayed;
                }
            }
        }

        new_item
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
        assert!(tracklist.replace(from_session.queue().into_iter().cloned().collect(), true));
        assert_eq!(ids(&tracklist), vec![1, 3, 2, 4]);
        assert_eq!(tracklist.current_position(), 0);
        assert_eq!(tracklist.current_connect_id(), Some(11));
        assert_eq!(tracklist.next_connect_id(), None);
    }

    #[test]
    fn replace_places_an_unknown_item_after_its_nearest_surviving_predecessor() {
        let mut tracklist = connect_queue(&[(0, 10), (1, 11), (2, 12)]);
        tracklist.skip_to_track(0);
        tracklist.insert_track(2, Track::default());
        assert_eq!(ids(&tracklist), vec![0, 1, 3, 2]);

        let from_session = connect_queue(&[(0, 10), (2, 12)]);
        assert!(tracklist.replace(from_session.queue().into_iter().cloned().collect(), true));
        assert_eq!(ids(&tracklist), vec![0, 3, 2]);
    }

    #[test]
    fn replace_can_drop_the_items_the_session_does_not_know() {
        let mut tracklist = connect_queue(&[(0, 10)]);
        tracklist.push_track(Track::default());
        let from_session = connect_queue(&[(0, 10), (2, 11)]);
        assert!(!tracklist.replace(from_session.queue().into_iter().cloned().collect(), false));
        assert_eq!(ids(&tracklist), vec![0, 2]);
        assert_eq!(tracklist.position_of_connect_id(11), Some(1));
    }

    #[test]
    fn replace_reports_a_current_track_that_the_session_dropped() {
        let mut tracklist = connect_queue(&[(0, 10), (1, 11)]);
        tracklist.skip_to_track(0);
        let from_session = connect_queue(&[(1, 11)]);
        assert!(!tracklist.replace(from_session.queue().into_iter().cloned().collect(), true));
        assert_eq!(ids(&tracklist), vec![1]);
    }
}
