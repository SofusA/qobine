use controls_module::{controls::Controls, models::PlaylistSimple};
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
    app::{FilteredListState, NotificationList, Output},
    popup::{DeletePlaylistPopupState, NewPlaylistPopupState, PlaylistPopupState, Popup},
    ui::{
        COLUMN_SPACING, HIGHLIGHT_STYLE, SELECTED_STYLE, fetch_image, format_duration,
        mark_as_owned,
    },
};

#[derive(Default)]
pub struct PlaylistList {
    items: FilteredListState<PlaylistSimple>,
}

impl PlaylistList {
    pub fn new(playlists: Vec<PlaylistSimple>) -> Self {
        let is_empty = playlists.is_empty();

        let mut playlists = FilteredListState::new(playlists);

        if !is_empty {
            playlists.state.select(Some(0));
        }

        Self { items: playlists }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer, focus: bool) {
        let table = playlist_list(self.items.filter(), focus);
        table.render(area, buf, &mut self.items.state);
    }

    pub fn set_filter(&mut self, items: Vec<PlaylistSimple>) {
        self.items.set_filter(items);
    }

    pub fn all_items(&self) -> &Vec<PlaylistSimple> {
        self.items.all_items()
    }

    pub fn set_all_items(&mut self, items: Vec<PlaylistSimple>) {
        self.items.set_all_items(items);
    }

    pub fn selected(&self) -> Option<usize> {
        self.items.state.selected()
    }

    pub fn get(&self, index: usize) -> Option<&PlaylistSimple> {
        self.items.filter().get(index)
    }

    pub fn select_next(&mut self) {
        self.items.state.select_next();
    }

    pub fn select_previous(&mut self) {
        self.items.state.select_previous();
    }

    pub async fn handle_events(
        &mut self,
        event: KeyCode,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
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

            KeyCode::Char('C') => Ok(Output::Popup(Popup::NewPlaylist(
                NewPlaylistPopupState::new(),
            ))),

            KeyCode::Char('A') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected
                    && !selected.is_owned
                {
                    client.add_favorite_playlist(selected.id).await?;

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
                    match selected.is_owned {
                        true => {
                            return Ok(Output::Popup(Popup::DeletePlaylist(
                                DeletePlaylistPopupState::new(selected.clone()),
                            )));
                        }
                        false => {
                            client.remove_favorite_playlist(selected.id).await?;

                            notifications.push(Notification::Info(format!(
                                "{} removed from favorites",
                                selected.title
                            )));
                            return Ok(Output::UpdateFavorites);
                        }
                    }
                }

                Ok(Output::Consumed)
            }

            KeyCode::Char('B') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    let ids = client
                        .playlist(selected.id)
                        .await?
                        .tracks
                        .into_iter()
                        .map(|x| x.id)
                        .collect();

                    controls.add_tracks_to_queue(ids);
                }

                Ok(Output::Consumed)
            }

            KeyCode::Char('N') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    let ids = client
                        .playlist(selected.id)
                        .await?
                        .tracks
                        .into_iter()
                        .map(|x| x.id)
                        .collect();
                    controls.play_tracks_next(ids);
                }

                Ok(Output::Consumed)
            }

            KeyCode::Char('i') => {
                let index = self.items.state.selected();

                let id = index
                    .and_then(|index| self.items.filter().get(index))
                    .map(|playlist| playlist.id);

                if let Some(id) = id {
                    let playlist = client.playlist(id).await?;
                    let image = match playlist.image.as_ref() {
                        Some(x) => fetch_image(x).await,
                        None => None,
                    };

                    return Ok(Output::Popup(Popup::PlaylistInfo(playlist, image)));
                }
                Ok(Output::Consumed)
            }

            KeyCode::Enter => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                let Some(selected) = selected else {
                    return Ok(Output::Consumed);
                };

                let playlist = client.playlist(selected.id).await?;

                Ok(Output::Popup(Popup::Playlist(PlaylistPopupState::new(
                    playlist,
                ))))
            }

            _ => Ok(Output::NotConsumed),
        }
    }
}

fn playlist_list<'a>(rows: &[PlaylistSimple], focus: bool) -> Table<'a> {
    let body_rows: Vec<Row<'a>> = rows
        .iter()
        .map(|playlist| {
            Row::new(vec![
                mark_as_owned(playlist.title.clone(), playlist.is_owned),
                Line::from(format_duration(playlist.duration_seconds)),
            ])
        })
        .collect();

    let is_empty = body_rows.is_empty();

    let constraints = [Constraint::Ratio(2, 3), Constraint::Length(10)];

    let mut table = Table::new(body_rows, constraints)
        .row_highlight_style(if focus {
            HIGHLIGHT_STYLE
        } else {
            SELECTED_STYLE
        })
        .column_spacing(COLUMN_SPACING);

    if !is_empty {
        table = table.header(Row::new(["Title", "Duration"]).add_modifier(Modifier::BOLD));
    }

    table
}
