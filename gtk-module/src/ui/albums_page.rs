use std::rc::Rc;

use controls_module::models::AlbumSimple;
use glib::object::Cast;
use gtk4 as gtk;

use crate::ui::album_detail_page::AlbumHeaderInfo;
use crate::ui::build_album_tile;
use crate::ui::grid_page::GridPage;

pub type AlbumsPage = GridPage<AlbumSimple>;

pub fn new_albums_page(on_open: Rc<dyn Fn(AlbumHeaderInfo)>) -> AlbumsPage {
    let matches_query = |album: &AlbumSimple, query: &str| {
        album.title.to_lowercase().contains(query)
            || album.artist.name.to_lowercase().contains(query)
    };

    let alphabetical_compare = |a: &AlbumSimple, b: &AlbumSimple| {
        a.artist
            .name
            .to_lowercase()
            .cmp(&b.artist.name.to_lowercase())
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    };

    let build_tile = |album: &AlbumSimple| build_album_tile(album).upcast();

    let on_activate = move |album: &AlbumSimple| {
        on_open(AlbumHeaderInfo {
            id: album.id.clone(),
        });
    };

    GridPage::new(
        2,
        8,
        gtk::Align::Start,
        matches_query,
        alphabetical_compare,
        build_tile,
        on_activate,
    )
}
