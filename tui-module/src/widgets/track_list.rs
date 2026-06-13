use std::collections::HashSet;

use controls_module::{controls::Controls, models::Track};
use player_module::{AppResult, client::Client, notification::Notification};
use ratatui::{
    buffer::Buffer,
    crossterm::event::KeyCode,
    layout::{Constraint, Rect},
    style::{Modifier, Stylize},
    text::Line,
    widgets::{Row, StatefulWidget, Table},
};

use crate::{
    app::{FavoriteAdd, FavoriteRemove, FilteredListState, NotificationList, Output},
    popup::Popup,
    ui::{
        COLUMN_SPACING, HIGHLIGHT_STYLE, SELECTED_STYLE, fetch_image, format_duration,
        mark_explicit_and_hifi, mark_favorite,
    },
};

#[derive(Default)]
pub struct TrackList {
    items: FilteredListState<Track>,
}

pub enum TrackListEvent {
    Track,
    Album(String),
    Playlist(u32, bool),
    Artist(u32),
}

impl TrackList {
    pub fn new(tracks: Vec<Track>) -> Self {
        let is_empty = tracks.is_empty();

        let mut tracks = FilteredListState::new(tracks);

        if !is_empty {
            tracks.state.select(Some(0));
        }

        Self { items: tracks }
    }

    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        show_album: bool,
        focus: bool,
        favorite_tracks: &HashSet<u32>,
    ) {
        let table = track_table(self.items.filter(), show_album, focus, favorite_tracks);
        table.render(area, buf, &mut self.items.state);
    }

    pub fn all_items(&self) -> &Vec<Track> {
        self.items.all_items()
    }

    pub fn set_filter(&mut self, items: Vec<Track>) {
        self.items.set_filter(items);
    }

    pub fn select_index(&mut self, index: usize) {
        self.items.state.select(Some(index));
    }

    pub fn set_all_items(&mut self, items: Vec<Track>) {
        self.items.set_all_items(items);
    }

    pub fn selected(&self) -> Option<usize> {
        self.items.state.selected()
    }

    pub fn get(&self, index: usize) -> Option<&Track> {
        self.items.filter().get(index)
    }

    pub fn remove_at_index(&mut self, index: usize) {
        self.items.remove_at_index(index);
    }

    pub fn move_index_to_new_index(&mut self, index: usize, new_index: usize) {
        self.items.move_index_to_new_index(index, new_index);
    }

    pub fn filter(&self) -> &Vec<Track> {
        self.items.filter()
    }

    pub async fn handle_events(
        &mut self,
        event: KeyCode,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
        event_type: TrackListEvent,
    ) -> AppResult<Output> {
        match event {
            KeyCode::Down | KeyCode::Char('j') => {
                self.items.state.select_next();
                Ok(Output::Consumed)
            }

            KeyCode::Up | KeyCode::Char('k') => {
                self.items.state.select_previous();
                Ok(Output::Consumed)
            }

            KeyCode::Char('a') => {
                let index = self.items.state.selected();

                let track = index.and_then(|index| self.items.filter().get(index));

                if let Some(id) = track {
                    return Ok(Output::AddTrackToPlaylistPopup(id.clone()));
                }
                Ok(Output::Consumed)
            }

            KeyCode::Char('N') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                let Some(selected) = selected else {
                    return Ok(Output::Consumed);
                };

                controls.play_tracks_next(vec![selected.id]);
                Ok(Output::Consumed)
            }

            KeyCode::Char('B') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    controls.add_tracks_to_queue(vec![selected.id]);
                };
                Ok(Output::Consumed)
            }

            KeyCode::Char('A') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    client.add_favorite_track(selected.id).await?;
                    notifications.push(Notification::Info(format!(
                        "{} added to favorites",
                        selected.title
                    )));
                    return Ok(Output::FavoriteAdded(FavoriteAdd::Track(selected.clone())));
                }

                Ok(Output::Consumed)
            }

            KeyCode::Char('U') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    client.remove_favorite_track(selected.id).await?;
                    notifications.push(Notification::Info(format!(
                        "{} removed from favorites",
                        selected.title
                    )));
                    return Ok(Output::FavoriteRemoved(FavoriteRemove::Track(selected.id)));
                }
                Ok(Output::Consumed)
            }

            KeyCode::Char('S') => {
                let ids = self.filter().iter().map(|x| x.id).collect();
                controls.play_tracks(ids, true);
                Ok(Output::Consumed)
            }

            KeyCode::Char('i') => {
                let index = self.items.state.selected();

                let track = index
                    .and_then(|index| self.items.filter().get(index))
                    .cloned();

                let image = match track.as_ref().and_then(|x| x.image.clone()) {
                    Some(x) => fetch_image(&x).await,
                    None => None,
                };

                if let Some(track) = track {
                    return Ok(Output::Popup(Popup::TrackInfo(track, image)));
                }
                Ok(Output::Consumed)
            }

            KeyCode::Enter => {
                let Some(index) = self.items.state.selected() else {
                    return Ok(Output::Consumed);
                };

                match event_type {
                    TrackListEvent::Track => {
                        let selected = self.items.filter().get(index);
                        if let Some(selected) = selected {
                            controls.play_track(selected.id);
                        }
                    }
                    TrackListEvent::Album(id) => controls.play_album(&id, index),
                    TrackListEvent::Playlist(id, shuffle) => {
                        controls.play_playlist(id, index, shuffle)
                    }
                    TrackListEvent::Artist(id) => controls.play_top_tracks(id, index),
                }

                Ok(Output::Consumed)
            }

            _ => Ok(Output::NotConsumed),
        }
    }
}

fn track_table<'a>(
    rows: &[Track],
    show_album: bool,
    focus: bool,
    favorite_tracks: &HashSet<u32>,
) -> Table<'a> {
    let body_rows: Vec<Row<'a>> = rows
        .iter()
        .map(|track| {
            let mut cols: Vec<Line<'a>> = Vec::with_capacity(if show_album { 4 } else { 3 });

            let title =
                mark_explicit_and_hifi(track.title.clone(), track.explicit, track.hires_available);
            cols.push(mark_favorite(title, favorite_tracks.contains(&track.id)));

            cols.push(Line::from(track.artist_name.clone().unwrap_or_default()));

            if show_album {
                cols.push(Line::from(track.album_title.clone().unwrap_or_default()));
            }

            cols.push(Line::from(format_duration(track.duration_seconds)));

            Row::new(cols)
        })
        .collect();

    let is_empty = body_rows.is_empty();

    let constraints: Vec<Constraint> = if show_album {
        vec![
            Constraint::Ratio(2, 6),
            Constraint::Ratio(2, 6),
            Constraint::Ratio(1, 6),
            Constraint::Length(10),
        ]
    } else {
        vec![
            Constraint::Ratio(2, 5),
            Constraint::Ratio(2, 5),
            Constraint::Length(10),
        ]
    };

    let mut table = Table::new(body_rows, constraints)
        .row_highlight_style(if focus {
            HIGHLIGHT_STYLE
        } else {
            SELECTED_STYLE
        })
        .column_spacing(COLUMN_SPACING);

    if !is_empty {
        let header = if show_album {
            Row::new(vec!["Title", "Artist", "Album", "Duration"])
        } else {
            Row::new(vec!["Title", "Artist", "Duration"])
        }
        .add_modifier(Modifier::BOLD);

        table = table.header(header);
    }

    table
}
