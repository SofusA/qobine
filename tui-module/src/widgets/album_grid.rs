use std::collections::HashSet;

use controls_module::{controls::Controls, models::AlbumSimple};
use player_module::{AppResult, client::Client, notification::Notification};
use ratatui::{
    buffer::Buffer,
    crossterm::event::KeyCode,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Text},
    widgets::{Block, BorderType, Borders, Paragraph, StatefulWidget, Widget},
};
use ratatui_image::StatefulImage;

use crate::{
    app::{NotificationList, Output},
    image_cache::ImageManager,
    popup::{AlbumPopupState, Popup},
    ui::{
        ALBUM_COVER_HEIGHT, ALBUM_COVER_WIDTH, HIGHLIGHT_TEXT_STYLE, SELECTED_STYLE,
        album_cover_area, mark_explicit_and_hifi,
    },
    widgets::filtered_list::FilteredListState,
};

const CARD_WIDTH: u16 = ALBUM_COVER_WIDTH + 2;
const CARD_HEIGHT: u16 = ALBUM_COVER_HEIGHT + 5;

#[derive(Default)]
pub struct AlbumGrid {
    items: FilteredListState<AlbumSimple>,
    scroll_row: usize,
    columns: usize,
}

impl AlbumGrid {
    pub fn new(albums: Vec<AlbumSimple>) -> Self {
        let is_empty = albums.is_empty();
        let mut items = FilteredListState::new(albums);

        if !is_empty {
            items.state.select(Some(0));
        }

        Self {
            items,
            scroll_row: 0,
            columns: 1,
        }
    }

    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        focus: bool,
        favorites: &HashSet<String>,
        image_cache: &mut ImageManager,
    ) {
        if area.width < CARD_WIDTH || area.height < CARD_HEIGHT {
            return;
        }

        self.columns = usize::from(area.width / CARD_WIDTH);
        let visible_rows = usize::from(area.height / CARD_HEIGHT);

        self.update_scroll(visible_rows);

        let albums = self.items.filter();

        let first_index = self.scroll_row * self.columns;
        let last_index = first_index
            .saturating_add(visible_rows * self.columns)
            .min(albums.len());

        for (index, album) in albums.iter().enumerate().take(last_index).skip(first_index) {
            let absolute_row = index / self.columns;
            let column = index % self.columns;
            let visible_row = absolute_row - self.scroll_row;

            let x = area.x + column as u16 * CARD_WIDTH;
            let y = area.y + visible_row as u16 * CARD_HEIGHT;

            let card_area = Rect::new(x, y, CARD_WIDTH, CARD_HEIGHT);

            let selected = self.items.state.selected() == Some(index);

            let style = if selected {
                if focus {
                    HIGHLIGHT_TEXT_STYLE
                } else {
                    SELECTED_STYLE
                }
            } else {
                Style::default()
            };

            Block::default()
                .borders(Borders::ALL)
                .border_style(style)
                .border_type(BorderType::Rounded)
                .render(card_area, buf);

            let inner = Rect::new(
                card_area.x + 1,
                card_area.y + 1,
                card_area.width.saturating_sub(2),
                card_area.height.saturating_sub(2),
            );

            let Some(image_area) = album_cover_area(inner) else {
                continue;
            };

            if let Some(image) = image_cache.get_mut(&album.image) {
                StatefulImage::default().render(image_area, buf, &mut image.protocol);
            } else {
                Paragraph::new("Loading...").render(image_area, buf);
            }

            let title = mark_explicit_and_hifi(
                album.title.clone(),
                album.explicit,
                album.hires_available,
                favorites.contains(&album.id),
            );

            let text_area = Rect::new(
                inner.x,
                image_area.bottom(),
                inner.width,
                inner.bottom().saturating_sub(image_area.bottom()),
            );

            Paragraph::new(Text::from(vec![
                title.patch_style(style.add_modifier(Modifier::BOLD)),
                Line::from(album.artist.name.clone()),
                Line::from(album.release_year.to_string()).style(Style::default().italic()),
            ]))
            .render(text_area, buf);
        }
    }

    fn update_scroll(&mut self, visible_rows: usize) {
        if visible_rows == 0 || self.columns == 0 {
            return;
        }

        let Some(selected) = self.items.state.selected() else {
            return;
        };

        let selected_row = selected / self.columns;

        if selected_row < self.scroll_row {
            self.scroll_row = selected_row;
        } else if selected_row >= self.scroll_row + visible_rows {
            self.scroll_row = selected_row.saturating_sub(visible_rows - 1);
        }

        let item_count = self.items.filter().len();
        let total_rows = item_count.div_ceil(self.columns);
        let max_scroll_row = total_rows.saturating_sub(visible_rows);

        self.scroll_row = self.scroll_row.min(max_scroll_row);
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.items.filter().len();

        if len == 0 {
            return;
        }

        let current = self.items.state.selected().unwrap_or(0);

        let next = current
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));

        self.items.state.select(Some(next));
    }

    pub fn all_items(&self) -> &[AlbumSimple] {
        self.items.all_items()
    }

    fn reset_view(&mut self) {
        self.scroll_row = 0;

        let selection = if self.items.filter().is_empty() {
            None
        } else {
            Some(0)
        };

        self.items.state.select(selection);
    }

    pub fn set_filter(&mut self, items: Vec<AlbumSimple>) {
        self.items.set_filter(items);
        self.reset_view();
    }

    pub fn set_all_items(&mut self, items: Vec<AlbumSimple>) {
        self.items.set_all_items(items);
        self.reset_view();
    }

    pub async fn handle_events(
        &mut self,
        event: KeyCode,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match event {
            KeyCode::Right | KeyCode::Char('l') => {
                self.move_selection(1);
                Ok(Output::Consumed)
            }

            KeyCode::Left | KeyCode::Char('h') => {
                self.move_selection(-1);
                Ok(Output::Consumed)
            }

            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(self.columns as isize);
                Ok(Output::Consumed)
            }

            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-(self.columns as isize));
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
                    return Ok(Output::UpdateFavorites);
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
                    return Ok(Output::UpdateFavorites);
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

            KeyCode::Enter | KeyCode::Char('i') => {
                let index = self.items.state.selected();

                let id = index
                    .and_then(|index| self.items.filter().get(index))
                    .map(|album| album.id.clone());

                if let Some(id) = id {
                    let album = client.album(&id).await?;

                    return Ok(Output::Popup(Popup::Album(
                        AlbumPopupState::new(album, client).await,
                    )));
                }

                Ok(Output::Consumed)
            }

            _ => Ok(Output::NotConsumed),
        }
    }
}
