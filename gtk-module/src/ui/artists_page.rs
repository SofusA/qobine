use std::rc::Rc;

use controls_module::models::Artist;
use glib::object::Cast;
use gtk4 as gtk;

use crate::ui::artist_detail_page::ArtistHeaderInfo;
use crate::ui::build_artist_tile;
use crate::ui::grid_page::GridPage;

pub type ArtistsPage = GridPage<Artist>;

pub fn new_artists_page(on_open: Rc<dyn Fn(ArtistHeaderInfo)>) -> ArtistsPage {
    let matches_query = |artist: &Artist, query: &str| artist.name.to_lowercase().contains(query);

    let build_tile = |artist: &Artist| build_artist_tile(artist).upcast();

    let on_activate = move |artist: &Artist| {
        on_open(ArtistHeaderInfo { id: artist.id });
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
