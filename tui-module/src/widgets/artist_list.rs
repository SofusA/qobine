use std::collections::HashSet;

use controls_module::models::Artist;
use player_module::{AppResult, client::Client, notification::Notification};
use ratatui::{
    buffer::Buffer,
    crossterm::event::KeyCode,
    layout::Rect,
    text::Line,
    widgets::{Row, StatefulWidget},
};

use crate::{
    app::{FavoriteAdd, FavoriteRemove, FilteredListState, NotificationList, Output},
    popup::{ArtistPopupState, Popup},
    ui::{basic_list_table, fetch_image, mark_favorite},
};

#[derive(Default)]
pub struct ArtistList {
    items: FilteredListState<Artist>,
}

impl ArtistList {
    pub fn new(artists: Vec<Artist>) -> Self {
        let is_empty = artists.is_empty();

        let mut artists = FilteredListState::new(artists);

        if !is_empty {
            artists.state.select(Some(0));
        }

        Self { items: artists }
    }

    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        focus: bool,
        favorite_artists: &HashSet<u32>,
    ) {
        let table = basic_list_table(
            self.items
                .filter()
                .iter()
                .map(|artist| {
                    let name = Line::from(artist.name.clone());
                    Row::new(vec![mark_favorite(
                        name,
                        favorite_artists.contains(&artist.id),
                    )])
                })
                .collect::<Vec<_>>(),
            focus,
        );

        table.render(area, buf, &mut self.items.state);
    }

    pub fn set_filter(&mut self, items: Vec<Artist>) {
        self.items.set_filter(items);
    }

    pub fn all_items(&self) -> &Vec<Artist> {
        self.items.all_items()
    }

    pub fn set_all_items(&mut self, items: Vec<Artist>) {
        self.items.set_all_items(items);
    }

    pub async fn handle_events(
        &mut self,
        event: KeyCode,
        client: &Client,
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
                    client.add_favorite_artist(selected.id).await?;

                    notifications.push(Notification::Info(format!(
                        "{} added to favorites",
                        selected.name
                    )));
                    return Ok(Output::FavoriteAdded(FavoriteAdd::Artist(selected.clone())));
                }
                Ok(Output::Consumed)
            }

            KeyCode::Char('U') => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                if let Some(selected) = selected {
                    client.remove_favorite_artist(selected.id).await?;

                    notifications.push(Notification::Info(format!(
                        "{} removed from favorites",
                        selected.name
                    )));
                    return Ok(Output::FavoriteRemoved(FavoriteRemove::Artist(selected.id)));
                }
                Ok(Output::Consumed)
            }

            KeyCode::Char('i') => {
                let index = self.items.state.selected();

                let id = index
                    .and_then(|index| self.items.filter().get(index))
                    .map(|artist| artist.id);

                if let Some(id) = id {
                    let artist = client.artist_page(id).await?;
                    let image = match artist.image.as_ref() {
                        Some(x) => fetch_image(x).await,
                        None => None,
                    };
                    return Ok(Output::Popup(Popup::ArtistInfo(artist, image)));
                }
                Ok(Output::Consumed)
            }

            KeyCode::Enter => {
                let index = self.items.state.selected();
                let selected = index.and_then(|index| self.items.filter().get(index));

                let Some(selected) = selected else {
                    return Ok(Output::Consumed);
                };

                let state = ArtistPopupState::new(selected, client).await?;

                Ok(Output::Popup(Popup::Artist(state)))
            }

            _ => Ok(Output::NotConsumed),
        }
    }
}
