use std::collections::HashSet;

use controls_module::{
    controls::Controls,
    models::{AlbumSimple, Artist, PlaylistSimple},
};
use player_module::{AppResult, client::Client, notification::Notification};
use ratatui::{
    buffer::Buffer,
    crossterm::event::KeyCode,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Text},
    widgets::{
        Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Widget,
    },
};
use ratatui_image::StatefulImage;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{NotificationList, Output},
    image_cache::ImageManager,
    popup::{
        AlbumPopupState, ArtistPopupState, DeletePlaylistPopupState, NewPlaylistPopupState,
        PlaylistPopupState, Popup,
    },
    ui::{
        ALBUM_COVER_HEIGHT, ALBUM_COVER_WIDTH, HIGHLIGHT_TEXT_STYLE, SELECTED_STYLE,
        album_cover_area, format_duration, mark_as_favorite, mark_as_owned, mark_explicit_and_hifi,
    },
    widgets::filtered_list::FilteredListState,
};

pub struct GridEventContext<'a> {
    pub client: &'a Client,
    pub controls: &'a Controls,
    pub notifications: &'a mut NotificationList,
}

pub trait GridItem {
    type Id: Eq + std::hash::Hash;

    const CARD_WIDTH: u16;
    const CARD_HEIGHT: u16;

    fn render_card(
        &self,
        area: Rect,
        buf: &mut Buffer,
        style: Style,
        favorites: &HashSet<Self::Id>,
        image_cache: &mut ImageManager,
    );

    async fn on_key_event(&self, key: KeyCode, context: GridEventContext<'_>) -> AppResult<Output>;
}

#[derive(Default)]
pub struct Grid<T> {
    items: FilteredListState<T>,
    scroll_row: usize,
    columns: usize,
}

impl<T: GridItem> Grid<T>
where
    T: Clone,
{
    pub fn new(items: Vec<T>) -> Self {
        let mut items = FilteredListState::new(items);

        if !items.filter().is_empty() {
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
        favorites: &HashSet<T::Id>,
        image_cache: &mut ImageManager,
    ) {
        if area.width < T::CARD_WIDTH || area.height < T::CARD_HEIGHT {
            return;
        }

        let visible_rows = usize::from(area.height / T::CARD_HEIGHT);
        let item_count = self.items.filter().len();

        // First calculate assuming no scrollbar.
        let full_columns = usize::from(area.width / T::CARD_WIDTH).max(1);
        let full_total_rows = item_count.div_ceil(full_columns);

        let show_scrollbar = full_total_rows > visible_rows;

        let grid_area = if show_scrollbar {
            Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height)
        } else {
            area
        };

        self.columns = usize::from(grid_area.width / T::CARD_WIDTH).max(1);

        let total_rows = item_count.div_ceil(self.columns);

        self.update_scroll(visible_rows);

        let items = self.items.filter();
        let first_index = self.scroll_row * self.columns;
        let last_index = first_index
            .saturating_add(visible_rows * self.columns)
            .min(items.len());

        for (index, item) in items.iter().enumerate().take(last_index).skip(first_index) {
            let absolute_row = index / self.columns;
            let column = index % self.columns;
            let visible_row = absolute_row - self.scroll_row;

            let card_area = Rect::new(
                grid_area.x + column as u16 * T::CARD_WIDTH,
                grid_area.y + visible_row as u16 * T::CARD_HEIGHT,
                T::CARD_WIDTH,
                T::CARD_HEIGHT,
            );

            let selected = self.items.state.selected() == Some(index);

            let style = match (selected, focus) {
                (true, true) => HIGHLIGHT_TEXT_STYLE,
                (true, false) => SELECTED_STYLE,
                _ => Style::default(),
            };

            item.render_card(card_area, buf, style, favorites, image_cache);
        }

        if show_scrollbar {
            let max_scroll_row = total_rows.saturating_sub(visible_rows);

            let scrollbar_area = Rect::new(area.right().saturating_sub(1), area.y, 1, area.height);

            let mut scrollbar_state = ScrollbarState::new(max_scroll_row).position(self.scroll_row);

            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .render(scrollbar_area, buf, &mut scrollbar_state);
        }
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

            key => {
                let Some(item) = self.selected() else {
                    return Ok(Output::NotConsumed);
                };

                item.on_key_event(
                    key,
                    GridEventContext {
                        client,
                        controls,
                        notifications,
                    },
                )
                .await
            }
        }
    }

    pub fn selected(&self) -> Option<&T> {
        self.items
            .state
            .selected()
            .and_then(|index| self.items.filter().get(index))
    }

    pub fn filter(&self) -> &[T] {
        self.items.filter()
    }

    pub fn all_items(&self) -> &[T] {
        self.items.all_items()
    }

    pub fn set_filter(&mut self, items: Vec<T>) {
        self.items.set_filter(items);
        self.reset_view();
    }

    pub fn set_all_items(&mut self, items: Vec<T>) {
        self.items.set_all_items(items);
        self.reset_view();
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

    fn reset_view(&mut self) {
        self.scroll_row = 0;

        self.items
            .state
            .select((!self.items.filter().is_empty()).then_some(0));
    }
}

impl GridItem for AlbumSimple {
    type Id = String;
    const CARD_WIDTH: u16 = ALBUM_COVER_WIDTH + 2;
    const CARD_HEIGHT: u16 = ALBUM_COVER_HEIGHT + 5;

    fn render_card(
        &self,
        area: Rect,
        buf: &mut Buffer,
        style: Style,
        favorites: &HashSet<Self::Id>,
        image_cache: &mut ImageManager,
    ) {
        Block::default()
            .borders(Borders::ALL)
            .border_style(style)
            .border_type(BorderType::Rounded)
            .render(area, buf);

        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );

        let Some(image_area) = album_cover_area(inner) else {
            return;
        };

        if let Some(image) = image_cache.get_mut(&self.image) {
            StatefulImage::default().render(image_area, buf, &mut image.protocol);
        } else {
            Paragraph::new("Loading...").render(image_area, buf);
        }

        let text_area = Rect::new(
            inner.x,
            image_area.bottom(),
            inner.width,
            inner.bottom().saturating_sub(image_area.bottom()),
        );

        let is_favorite = favorites.contains(&self.id);

        let marked_title = mark_explicit_and_hifi(
            self.title.clone(),
            self.explicit,
            self.hires_available,
            is_favorite,
        );

        let original_width = UnicodeWidthStr::width(self.title.as_str());

        let marker_width = marked_title.width().saturating_sub(original_width);

        let available_width = usize::from(text_area.width).saturating_sub(marker_width);

        let title = mark_explicit_and_hifi(
            truncate_to_width(&self.title, available_width),
            self.explicit,
            self.hires_available,
            is_favorite,
        );

        let artist = truncate_to_width(&self.artist.name, usize::from(text_area.width));

        Paragraph::new(Text::from(vec![
            title.patch_style(style.add_modifier(Modifier::BOLD)),
            Line::from(artist),
            Line::from(self.release_year.to_string()).style(Style::default().italic()),
        ]))
        .render(text_area, buf);
    }

    async fn on_key_event(&self, key: KeyCode, context: GridEventContext<'_>) -> AppResult<Output> {
        match key {
            KeyCode::Char('A') => {
                context.client.add_favorite_album(&self.id).await?;

                context.notifications.push(Notification::Info(format!(
                    "{} added to favorites",
                    self.title
                )));

                Ok(Output::UpdateFavorites)
            }

            KeyCode::Char('U') => {
                context.client.remove_favorite_album(&self.id).await?;

                context.notifications.push(Notification::Info(format!(
                    "{} removed from favorites",
                    self.title
                )));

                Ok(Output::UpdateFavorites)
            }

            KeyCode::Char('B') => {
                let tracks = context.client.album(&self.id).await?.tracks;
                context.controls.add_tracks_to_queue(tracks);

                Ok(Output::Consumed)
            }

            KeyCode::Char('N') => {
                let tracks = context.client.album(&self.id).await?.tracks;
                context.controls.play_tracks_next(tracks);

                Ok(Output::Consumed)
            }

            KeyCode::Enter | KeyCode::Char('i') => {
                let album = context.client.album(&self.id).await?;

                Ok(Output::Popup(Popup::Album(
                    AlbumPopupState::new(album, context.client).await,
                )))
            }

            _ => Ok(Output::NotConsumed),
        }
    }
}

impl GridItem for Artist {
    type Id = u32;
    const CARD_WIDTH: u16 = ALBUM_COVER_WIDTH + 2;
    const CARD_HEIGHT: u16 = ALBUM_COVER_HEIGHT + 3;

    fn render_card(
        &self,
        area: Rect,
        buf: &mut Buffer,
        style: Style,
        favorites: &HashSet<Self::Id>,
        image_cache: &mut ImageManager,
    ) {
        Block::default()
            .borders(Borders::ALL)
            .border_style(style)
            .border_type(BorderType::Rounded)
            .render(area, buf);

        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );

        let Some(image_area) = album_cover_area(inner) else {
            return;
        };

        if let Some(image_key) = &self.image {
            if let Some(image) = image_cache.get_mut(image_key) {
                StatefulImage::default().render(image_area, buf, &mut image.protocol);
            } else {
                Paragraph::new("Loading...").render(image_area, buf);
            }
        } else {
            Paragraph::new("No image").render(image_area, buf);
        }

        let text_area = Rect::new(
            inner.x,
            image_area.bottom(),
            inner.width,
            inner.bottom().saturating_sub(image_area.bottom()),
        );

        let is_favorite = favorites.contains(&self.id);

        let marker_width = mark_as_favorite(Line::default(), is_favorite).width();

        let available_name_width = usize::from(text_area.width).saturating_sub(marker_width);

        let name = Line::from(truncate_to_width(&self.name, available_name_width));

        let name =
            mark_as_favorite(name, is_favorite).patch_style(style.add_modifier(Modifier::BOLD));

        Paragraph::new(name).render(text_area, buf);
    }

    async fn on_key_event(&self, key: KeyCode, context: GridEventContext<'_>) -> AppResult<Output> {
        match key {
            KeyCode::Char('A') => {
                context.client.add_favorite_artist(self.id).await?;

                context.notifications.push(Notification::Info(format!(
                    "{} added to favorites",
                    self.name
                )));

                Ok(Output::UpdateFavorites)
            }

            KeyCode::Char('U') => {
                context.client.remove_favorite_artist(self.id).await?;

                context.notifications.push(Notification::Info(format!(
                    "{} removed from favorites",
                    self.name
                )));

                Ok(Output::UpdateFavorites)
            }

            KeyCode::Enter | KeyCode::Char('i') => {
                let state = ArtistPopupState::new(self, context.client).await?;

                Ok(Output::Popup(Popup::Artist(state)))
            }

            _ => Ok(Output::NotConsumed),
        }
    }
}

impl GridItem for PlaylistSimple {
    type Id = u32;
    const CARD_WIDTH: u16 = ALBUM_COVER_WIDTH + 2;
    const CARD_HEIGHT: u16 = ALBUM_COVER_HEIGHT + 4;

    fn render_card(
        &self,
        area: Rect,
        buf: &mut Buffer,
        style: Style,
        favorites: &HashSet<Self::Id>,
        image_cache: &mut ImageManager,
    ) {
        Block::default()
            .borders(Borders::ALL)
            .border_style(style)
            .border_type(BorderType::Rounded)
            .render(area, buf);

        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );

        let Some(image_area) = album_cover_area(inner) else {
            return;
        };

        if let Some(image_key) = &self.image {
            if let Some(image) = image_cache.get_mut(image_key) {
                StatefulImage::default().render(image_area, buf, &mut image.protocol);
            } else {
                Paragraph::new("Loading...").render(image_area, buf);
            }
        } else {
            Paragraph::new("No image").render(image_area, buf);
        }

        let text_area = Rect::new(
            inner.x,
            image_area.bottom(),
            inner.width,
            inner.bottom().saturating_sub(image_area.bottom()),
        );

        let is_favorite = favorites.contains(&self.id);

        // Calculate marker width before truncating the title.
        let markers = mark_as_owned(
            mark_as_favorite(Line::default(), is_favorite),
            self.is_owned,
        );

        let marker_width = markers.width();
        let title_width = usize::from(text_area.width).saturating_sub(marker_width);

        let title = Line::from(truncate_to_width(&self.title, title_width));

        let title = mark_as_owned(mark_as_favorite(title, is_favorite), self.is_owned)
            .patch_style(style.add_modifier(Modifier::BOLD));

        let details = format!(
            "{} · {} tracks",
            format_duration(self.duration_seconds),
            self.tracks_count,
        );

        Paragraph::new(Text::from(vec![
            title,
            Line::from(truncate_to_width(&details, usize::from(text_area.width)))
                .style(Style::default().italic()),
        ]))
        .render(text_area, buf);
    }

    async fn on_key_event(&self, key: KeyCode, context: GridEventContext<'_>) -> AppResult<Output> {
        match key {
            KeyCode::Char('A') if !self.is_owned => {
                context.client.add_favorite_playlist(self.id).await?;

                context.notifications.push(Notification::Info(format!(
                    "{} added to favorites",
                    self.title
                )));

                Ok(Output::UpdateFavorites)
            }

            KeyCode::Char('U') if self.is_owned => Ok(Output::Popup(Popup::DeletePlaylist(
                DeletePlaylistPopupState::new(self.clone()),
            ))),

            KeyCode::Char('U') => {
                context.client.remove_favorite_playlist(self.id).await?;

                context.notifications.push(Notification::Info(format!(
                    "{} removed from favorites",
                    self.title
                )));

                Ok(Output::UpdateFavorites)
            }

            KeyCode::Char('C') => Ok(Output::Popup(Popup::NewPlaylist(
                NewPlaylistPopupState::new(),
            ))),

            KeyCode::Char('B') => {
                let tracks = context.client.playlist(self.id).await?.tracks;
                context.controls.add_tracks_to_queue(tracks);

                Ok(Output::Consumed)
            }

            KeyCode::Char('N') => {
                let tracks = context.client.playlist(self.id).await?.tracks;
                context.controls.play_tracks_next(tracks);

                Ok(Output::Consumed)
            }

            KeyCode::Enter | KeyCode::Char('i') => {
                let playlist = context.client.playlist(self.id).await?;

                Ok(Output::Popup(Popup::Playlist(PlaylistPopupState::new(
                    playlist,
                ))))
            }

            _ => Ok(Output::NotConsumed),
        }
    }
}

fn truncate_to_width(value: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_owned();
    }

    if max_width == 0 {
        return String::new();
    }

    let ellipsis = "…";
    let ellipsis_width = UnicodeWidthStr::width(ellipsis);

    if max_width < ellipsis_width {
        return String::new();
    }

    let content_width = max_width - ellipsis_width;
    let mut result = String::new();
    let mut width = 0;

    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);

        if width + character_width > content_width {
            break;
        }

        result.push(character);
        width += character_width;
    }

    result.truncate(result.trim_end().len());
    result.push_str(ellipsis);
    result
}
