use std::rc::Rc;

use controls_module::models::PlaylistSimple;
use glib::object::Cast;
use gtk4 as gtk;

use crate::ui::build_playlist_tile;
use crate::ui::grid_page::GridPage;
use crate::ui::playlist_detail_page::PlaylistHeaderInfo;

pub type PlaylistsPage = GridPage<PlaylistSimple>;

pub fn new_playlists_page(on_open: Rc<dyn Fn(PlaylistHeaderInfo)>) -> PlaylistsPage {
    let matches_query =
        |playlist: &PlaylistSimple, query: &str| playlist.title.to_lowercase().contains(query);

    let build_tile = |playlist: &PlaylistSimple| build_playlist_tile(playlist).upcast();

    let on_activate = move |playlist: &PlaylistSimple| {
        on_open(PlaylistHeaderInfo { id: playlist.id });
    };

    GridPage::new(
        2,
        10,
        gtk::Align::End,
        matches_query,
        build_tile,
        on_activate,
    )
}
