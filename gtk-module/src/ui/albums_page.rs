use std::rc::Rc;

use controls_module::models::AlbumSimple;
use glib::object::Cast;
use gtk4 as gtk;

use crate::ui::album_detail_page::AlbumHeaderInfo;
use crate::ui::build_album_tile;
use crate::ui::grid_page::GridPage;

pub type AlbumsPage = GridPage<AlbumSimple>;

pub fn new_albums_page(on_open: Rc<dyn Fn(AlbumHeaderInfo)>) -> AlbumsPage {
    let matches_query = Rc::new(|album: &AlbumSimple, q: &str| {
        album.title.to_lowercase().contains(q) || album.artist.name.to_lowercase().contains(q)
    });

    let build_tile = Rc::new(|album: &AlbumSimple| build_album_tile(album).upcast::<gtk::Widget>());

    let on_activate = Rc::new(move |album: &AlbumSimple| {
        on_open(AlbumHeaderInfo {
            id: album.id.clone(),
        });
    });

    GridPage::new(
        2,
        8,
        gtk::Align::Start,
        matches_query,
        build_tile,
        on_activate,
    )
}
