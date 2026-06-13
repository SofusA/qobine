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
        Rc::new(|playlist: &PlaylistSimple, q: &str| playlist.title.to_lowercase().contains(q));

    let build_tile = Rc::new(|playlist: &PlaylistSimple| build_playlist_tile(playlist).upcast());

    let on_activate = Rc::new(move |playlist: &PlaylistSimple| {
        on_open(PlaylistHeaderInfo { id: playlist.id });
    });

    GridPage::new(
        2,
        10,
        gtk::Align::End,
        matches_query,
        build_tile,
        on_activate,
    )
}
