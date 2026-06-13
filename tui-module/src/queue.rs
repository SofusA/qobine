use std::collections::HashSet;

use controls_module::{
    controls::Controls,
    models::{Track, TrackStatus},
};
use player_module::{AppResult, client::Client, notification::Notification};
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind},
    prelude::*,
    style::Styled,
    widgets::*,
};

use crate::{
    app::{FavoriteAdd, FavoriteRemove, NotificationList, Output},
    ui::{basic_list_table, block, mark_explicit_and_hifi, mark_favorite},
};

pub struct QueueState {
    items: Vec<Track>,
    state: TableState,
}

impl QueueState {
    pub fn new(tracks: Vec<Track>) -> Self {
        Self {
            items: tracks,
            state: Default::default(),
        }
    }
    pub fn render(&mut self, frame: &mut Frame, area: Rect, favorite_tracks: &HashSet<u32>) {
        let table = basic_list_table(
            self.items
                .iter()
                .enumerate()
                .map(|(index, track)| {
                    let style = match track.status {
                        TrackStatus::Played => Style::default().add_modifier(Modifier::CROSSED_OUT),
                        TrackStatus::Playing => Style::default().add_modifier(Modifier::BOLD),
                        TrackStatus::Unplayed => Style::default(),
                        TrackStatus::Unplayable => {
                            Style::default().add_modifier(Modifier::CROSSED_OUT)
                        }
                    };

                    let title = mark_favorite(
                        mark_explicit_and_hifi(
                            track.title.clone(),
                            track.explicit,
                            track.hires_available,
                        ),
                        favorite_tracks.contains(&track.id),
                    );

                    let mut spans = vec![Span::from(format!("{} ", index + 1))];
                    spans.extend(title.spans);

                    Row::new(vec![Line::from(spans).set_style(style)])
                })
                .collect(),
            true,
        )
        .block(block(None));

        frame.render_stateful_widget(table, area, &mut self.state);
    }

    pub fn items(&self) -> &Vec<Track> {
        &self.items
    }

    pub fn set_items(&mut self, items: Vec<Track>) {
        self.items = items
    }

    pub async fn handle_events(
        &mut self,
        event: Event,
        client: &Client,
        controls: &Controls,
        notifications: &mut NotificationList,
    ) -> AppResult<Output> {
        match event {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                match key_event.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.state.select_next();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.state.select_previous();
                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('d') => {
                        let index = self.state.selected();

                        if let Some(index) = index {
                            if index == self.items().len() - 1 {
                                return Ok(Output::Consumed);
                            }

                            let mut order: Vec<_> =
                                self.items().iter().enumerate().map(|x| x.0).collect();

                            order.swap(index, index + 1);
                            controls.reorder_queue(order);
                        }
                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('u') => {
                        let index = self.state.selected();

                        if let Some(index) = index {
                            if index == 0 {
                                return Ok(Output::Consumed);
                            }
                            let mut order: Vec<_> =
                                self.items().iter().enumerate().map(|x| x.0).collect();

                            order.swap(index, index - 1);
                            controls.reorder_queue(order);
                        }
                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('D') => {
                        let index = self.state.selected();

                        if let Some(index) = index {
                            controls.remove_index_from_queue(index);
                        }
                        Ok(Output::Consumed)
                    }
                    KeyCode::Char('A') => {
                        let selected = self
                            .state
                            .selected()
                            .and_then(|index| self.items.get(index));

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
                        let selected = self
                            .state
                            .selected()
                            .and_then(|index| self.items.get(index));

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
                    KeyCode::Enter => {
                        let index = self.state.selected();

                        if let Some(index) = index {
                            controls.skip_to_position(index, true);
                        }
                        Ok(Output::Consumed)
                    }

                    _ => Ok(Output::NotConsumed),
                }
            }
            _ => Ok(Output::NotConsumed),
        }
    }
}
