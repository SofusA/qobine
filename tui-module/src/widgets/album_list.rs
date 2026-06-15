use std::collections::HashSet;

use controls_module::{controls::Controls, models::AlbumSimple};
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
    popup::{AlbumPopupState, Popup},
    ui::{
        COLUMN_SPACING, HIGHLIGHT_STYLE, SELECTED_STYLE, fetch_image, format_duration,
        mark_explicit_and_hifi, mark_favorite,
    },
};

#[derive(Default)]
pub struct AlbumList {
    items: FilteredListState<AlbumSimple>,
}

impl AlbumList {
    pub fn new(albums: Vec<AlbumSimple>) -> Self {
        let is_empty = albums.is_empty();
        let mut albums = FilteredListState::new(albums);

        if !is_empty {
            albums.state.select(Some(0));
        }

        Self { items: albums }
    }

    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        focus: bool,
        favorite_albums: &HashSet<String>,
    ) {
        let table = album_table(self.items.filter(), focus, favorite_albums);
        table.render(area, buf, &mut self.items.state);
    }

    pub fn filter(&self) -> &Vec<AlbumSimple> {
        self.items.filter()
    }

    pub fn set_filter(&mut self, items: Vec<AlbumSimple>) {
        self.items.set_filter(items);
    }

    pub fn all_items(&self) -> &Vec<AlbumSimple> {
        self.items.all_items()
    }

    pub fn set_all_items(&mut self, items: Vec<AlbumSimple>) {
        self.items.set_all_items(items);
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

            KeyCode::Char('A') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    client.add_favorite_album(&selected.id).await?;
                    notifications.push(Notification::Info(format!(
                        "{} added to favorites",
                        selected.title
                    )));
                    return Ok(Output::FavoriteAdded(FavoriteAdd::Album(selected.clone())));
                }

                Ok(Output::Consumed)
            }

            KeyCode::Char('U') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    client.remove_favorite_album(&selected.id).await?;

                    notifications.push(Notification::Info(format!(
                        "{} removed from favorites",
                        selected.title
                    )));
                    return Ok(Output::FavoriteRemoved(FavoriteRemove::Album(
                        selected.id.clone(),
                    )));
                }

                Ok(Output::Consumed)
            }

            KeyCode::Char('B') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    let tracks = client.album(&selected.id).await?.tracks;
                    controls.add_tracks_to_queue(tracks);
                }

                Ok(Output::Consumed)
            }

            KeyCode::Char('N') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    let tracks = client.album(&selected.id).await?.tracks;
                    controls.play_tracks_next(tracks);
                }

                Ok(Output::Consumed)
            }

            KeyCode::Char('i') => {
                let index = self.items.state.selected();

                let id = index
                    .and_then(|index| self.items.filter().get(index))
                    .map(|album| album.id.clone());

                if let Some(id) = id {
                    let album = client.album(&id).await?;
                    let image = fetch_image(&album.image).await;

                    return Ok(Output::Popup(Popup::AlbumInfo(album, false, image)));
                }

                Ok(Output::Consumed)
            }

            KeyCode::Enter => {
                let index = self.items.state.selected();

                let id = index
                    .and_then(|index| self.items.filter().get(index))
                    .map(|album| album.id.clone());

                if let Some(id) = id {
                    let album = client.album(&id).await?;

                    return Ok(Output::Popup(Popup::Album(AlbumPopupState::new(album))));
                }

                Ok(Output::Consumed)
            }

            _ => Ok(Output::NotConsumed),
        }
    }
}

pub fn album_table<'a>(
    rows: &[AlbumSimple],
    focus: bool,
    favorite_albums: &HashSet<String>,
) -> Table<'a> {
    let body_rows: Vec<Row<'a>> = rows
        .iter()
        .map(|album| {
            let title =
                mark_explicit_and_hifi(album.title.clone(), album.explicit, album.hires_available);
            Row::new(vec![
                mark_favorite(title, favorite_albums.contains(&album.id)),
                Line::from(album.artist.name.clone()),
                Line::from(album.release_year.to_string()),
                Line::from(format_duration(album.duration_seconds)),
            ])
        })
        .collect();

    let is_empty = body_rows.is_empty();

    let constraints = [
        Constraint::Ratio(2, 3),
        Constraint::Ratio(1, 3),
        Constraint::Length(4),
        Constraint::Length(10),
    ];

    let mut table = Table::new(body_rows, constraints)
        .row_highlight_style(if focus {
            HIGHLIGHT_STYLE
        } else {
            SELECTED_STYLE
        })
        .column_spacing(COLUMN_SPACING);

    if !is_empty {
        table = table
            .header(Row::new(["Title", "Artist", "Year", "Duration"]).add_modifier(Modifier::BOLD));
    }

    table
}
