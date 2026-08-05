use controls_module::models::{PlaylistSimple, Track};
use ratatui::{crossterm::event::KeyCode, prelude::*};

use crate::{
    app::{FavoriteIds, Output},
    ui::block,
    widgets::playlist_list::PlaylistList,
};

pub struct AddTrackOverlay {
    playlists: PlaylistList,
    track: Track,
}

impl AddTrackOverlay {
    pub fn new(track: Track, owned_playlists: Vec<PlaylistSimple>) -> Self {
        Self {
            playlists: PlaylistList::new(owned_playlists),
            track,
        }
    }

    pub fn track_title(&self) -> &str {
        &self.track.title
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, favorites: &FavoriteIds) {
        let title = format!("Add {} to playlist", self.track.title);

        let outer_block = block(Some(&title));
        let inner = outer_block.inner(area);

        frame.render_widget(&outer_block, area);

        self.playlists
            .render(inner, frame.buffer_mut(), true, favorites.playlists());
    }

    pub fn handle_event(&mut self, code: KeyCode) -> Output {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.playlists.select_previous();
                Output::Consumed
            }

            KeyCode::Down | KeyCode::Char('j') => {
                self.playlists.select_next();
                Output::Consumed
            }

            KeyCode::Enter => {
                let playlist_id = self
                    .playlists
                    .selected()
                    .and_then(|index| self.playlists.get(index))
                    .map(|playlist| playlist.id);

                match playlist_id {
                    Some(playlist_id) => {
                        Output::AddTrackToPlaylistAndPopOverlay((self.track.id, playlist_id))
                    }

                    None => Output::Consumed,
                }
            }

            KeyCode::Esc => Output::PopOverlay,

            _ => Output::Consumed,
        }
    }
}
