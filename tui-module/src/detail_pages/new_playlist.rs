use player_module::{AppResult, client::StreamClient};
use ratatui::{
    crossterm::event::{Event, KeyCode},
    prelude::*,
};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::{app::Output, ui::render_input};

pub struct NewPlaylistOverlay {
    name: Input,
}

impl NewPlaylistOverlay {
    pub fn new() -> Self {
        Self {
            name: Input::default(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        render_input(&self.name, false, area, frame, "Create playlist");
    }

    pub async fn handle_event(
        &mut self,
        code: KeyCode,
        event: &Event,
        client: &StreamClient,
    ) -> AppResult<Output> {
        match code {
            KeyCode::Enter => {
                let name = self.name.value().trim();

                if name.is_empty() {
                    return Ok(Output::Consumed);
                }

                client
                    .create_playlist(name.to_owned(), false, String::default(), None)
                    .await?;

                Ok(Output::PopOverlayUpdateFavorites)
            }

            KeyCode::Esc => Ok(Output::PopOverlay),

            _ => {
                self.name.handle_event(event);
                Ok(Output::Consumed)
            }
        }
    }
}

impl Default for NewPlaylistOverlay {
    fn default() -> Self {
        Self::new()
    }
}
