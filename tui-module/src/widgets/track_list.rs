use std::collections::HashSet;

use controls_module::{controls::Controls, models::Track};
use player_module::{AppResult, client::StreamClient, notification::Notification};
use ratatui::{
    buffer::Buffer,
    crossterm::event::KeyCode,
    layout::{Constraint, Rect},
    style::{Modifier, Stylize},
    text::Line,
    widgets::{Row, StatefulWidget, Table},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{NotificationList, Output},
    detail_pages::{Overlay, TrackInfoOverlay},
    ui::{
        COLUMN_SPACING, DURATION_LENGTH, HIGHLIGHT_STYLE, SELECTED_STYLE, format_duration,
        mark_explicit_and_hifi,
    },
    widgets::filtered_list::FilteredListState,
};

const MAX_ARTIST_LENGTH: u16 = 30;
const MAX_ALBUM_LENGTH: u16 = 40;

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

    pub fn render(&mut self, area: Rect, buf: &mut Buffer, focus: bool, favorites: &HashSet<u32>) {
        let table = track_table(self.items.filter(), focus, favorites);
        table.render(area, buf, &mut self.items.state);
    }

    pub fn all_items(&self) -> &[Track] {
        self.items.all_items()
    }

    pub fn set_filter(&mut self, items: Vec<Track>) {
        let selected_id = self
            .items
            .state
            .selected()
            .and_then(|index| self.items.filter().get(index))
            .map(|track| track.id);

        let selected_index = selected_id
            .and_then(|selected_id| items.iter().position(|track| track.id == selected_id));

        let is_empty = items.is_empty();

        self.items.set_filter(items);

        self.items
            .state
            .select(selected_index.or_else(|| (!is_empty).then_some(0)));
    }

    pub const fn select_index(&mut self, index: usize) {
        self.items.state.select(Some(index));
    }

    pub fn set_all_items(&mut self, items: Vec<Track>) {
        let is_empty = items.is_empty();
        self.items.set_all_items(items);
        if !is_empty {
            self.items.state.select(Some(0));
        }
    }

    pub const fn selected(&self) -> Option<usize> {
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

    pub fn filter(&self) -> &[Track] {
        self.items.filter()
    }

    pub async fn handle_events(
        &mut self,
        event: KeyCode,
        client: &StreamClient,
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
                    return Ok(Output::AddTrackToPlaylistOverlay(id.clone()));
                }
                Ok(Output::Consumed)
            }

            KeyCode::Char('N') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                let Some(selected) = selected else {
                    return Ok(Output::Consumed);
                };

                controls.play_tracks_next(vec![selected.clone()]);
                Ok(Output::Consumed)
            }

            KeyCode::Char('B') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    controls.add_tracks_to_queue(vec![selected.clone()]);
                }
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
                    return Ok(Output::UpdateFavorites);
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
                    return Ok(Output::UpdateFavorites);
                }
                Ok(Output::Consumed)
            }

            KeyCode::Char('S') => {
                let tracks = self.filter();
                controls.play_tracks(tracks, true, 0);
                Ok(Output::Consumed)
            }

            KeyCode::Char('i') => {
                let id = self
                    .items
                    .state
                    .selected()
                    .and_then(|index| self.items.filter().get(index))
                    .map(|track| track.id);

                if let Some(id) = id {
                    let track = client.track(id).await?;
                    let state = TrackInfoOverlay::new(track);
                    return Ok(Output::Overlay(Overlay::TrackInfo(state)));
                }
                Ok(Output::Consumed)
            }

            KeyCode::Enter => {
                let Some(index) = self.items.state.selected() else {
                    return Ok(Output::Consumed);
                };

                match event_type {
                    TrackListEvent::Track => {
                        controls.play_tracks(self.items.filter(), false, index);
                    }
                    TrackListEvent::Album(id) => controls.play_album(&id, index),
                    TrackListEvent::Playlist(id, shuffle) => {
                        controls.play_playlist(id, index, shuffle);
                    }
                    TrackListEvent::Artist(id) => controls.play_top_tracks(id, index),
                }

                Ok(Output::Consumed)
            }

            _ => Ok(Output::NotConsumed),
        }
    }
}

fn column_width<'a>(values: impl Iterator<Item = &'a str>, header: &str, maximum: u16) -> u16 {
    values
        .map(|value| u16::try_from(value.width()).unwrap_or(u16::MAX))
        .chain(std::iter::once(
            u16::try_from(header.width()).unwrap_or(u16::MAX),
        ))
        .max()
        .unwrap_or_default()
        .min(maximum)
}

fn track_table<'a>(rows: &[Track], focus: bool, favorites: &HashSet<u32>) -> Table<'a> {
    let artist_width = column_width(
        rows.iter().filter_map(|track| track.artist_name.as_deref()),
        "Artist",
        MAX_ARTIST_LENGTH,
    );

    let album_width = column_width(
        rows.iter().filter_map(|track| track.album_title.as_deref()),
        "Album",
        MAX_ALBUM_LENGTH,
    );

    let body_rows: Vec<Row<'a>> = rows
        .iter()
        .map(|track| {
            Row::new(vec![
                mark_explicit_and_hifi(
                    track.title.clone(),
                    track.explicit,
                    track.hires_available,
                    favorites.contains(&track.id),
                ),
                Line::from(track.artist_name.clone().unwrap_or_default()),
                Line::from(track.album_title.clone().unwrap_or_default()),
                Line::from(format_duration(track.duration_seconds)).right_aligned(),
            ])
        })
        .collect();

    let is_empty = body_rows.is_empty();

    let constraints = [
        Constraint::Min(1),
        Constraint::Length(artist_width),
        Constraint::Length(album_width),
        Constraint::Length(DURATION_LENGTH),
    ];

    let mut table = Table::new(body_rows, constraints)
        .row_highlight_style(if focus {
            HIGHLIGHT_STYLE
        } else {
            SELECTED_STYLE
        })
        .column_spacing(COLUMN_SPACING);

    if !is_empty {
        table = table.header(
            Row::new(vec!["Title", "Artist", "Album", "Duration"]).add_modifier(Modifier::BOLD),
        );
    }

    table
}
