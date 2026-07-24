use controls_module::models::PlaylistSimple;
use player_module::{AppResult, client::Client};
use ratatui::{crossterm::event::KeyCode, prelude::*};

use crate::{
    app::Output,
    ui::{block, tab_bar},
};

pub struct DeletePlaylistOverlay {
    title: String,
    id: u32,
    confirm: bool,
}

impl DeletePlaylistOverlay {
    pub fn new(playlist: PlaylistSimple) -> Self {
        Self {
            title: playlist.title,
            id: playlist.id,
            confirm: false,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let title = format!("Delete {}?", self.title);

        let selected = if self.confirm { 0 } else { 1 };

        let buttons = tab_bar(["Delete", "Cancel"].into(), selected).block(block(Some(&title)));

        frame.render_widget(buttons, area);
    }

    pub async fn handle_event(&mut self, code: KeyCode, client: &Client) -> AppResult<Output> {
        match code {
            KeyCode::Enter if self.confirm => {
                client.delete_playlist(self.id).await?;
                Ok(Output::PopOverlayUpdateFavorites)
            }

            KeyCode::Enter => Ok(Output::PopOverlayUpdateFavorites),

            KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
                self.confirm = !self.confirm;
                Ok(Output::Consumed)
            }

            KeyCode::Esc => Ok(Output::PopOverlay),

            _ => Ok(Output::Consumed),
        }
    }
}
