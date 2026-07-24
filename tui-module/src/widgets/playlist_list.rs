use std::collections::HashSet;

use controls_module::models::PlaylistSimple;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Modifier, Stylize},
    text::Line,
    widgets::{Row, StatefulWidget, Table},
};

use crate::{
    ui::{
        COLUMN_SPACING, HIGHLIGHT_STYLE, SELECTED_STYLE, format_duration, mark_as_favorite,
        mark_as_owned,
    },
    widgets::filtered_list::FilteredListState,
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

    pub fn render(&mut self, area: Rect, buf: &mut Buffer, focus: bool, favorites: &HashSet<u32>) {
        let table = playlist_list(self.items.filter(), focus, favorites);
        table.render(area, buf, &mut self.items.state);
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
}

fn playlist_list<'a>(rows: &[PlaylistSimple], focus: bool, favorites: &HashSet<u32>) -> Table<'a> {
    let body_rows: Vec<Row<'a>> = rows
        .iter()
        .map(|playlist| {
            let name = Line::from(playlist.title.clone());
            let line = mark_as_favorite(name, favorites.contains(&playlist.id));
            let line = mark_as_owned(line, playlist.is_owned);
            Row::new(vec![
                line,
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
