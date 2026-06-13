use std::{rc::Rc, sync::Arc};

use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use player_module::client::Client;

use crate::ui::albums_page::AlbumsPage;
use crate::ui::albums_page::new_albums_page;
use crate::ui::artists_page::ArtistsPage;
use crate::ui::artists_page::new_artists_page;
use crate::ui::playlists_page::PlaylistsPage;
use crate::ui::playlists_page::new_playlists_page;
use crate::ui::{
    album_detail_page::AlbumHeaderInfo, artist_detail_page::ArtistHeaderInfo,
    playlist_detail_page::PlaylistHeaderInfo,
};

pub struct SearchPage {
    root: gtk4::Box,
    client: Arc<Client>,
    spinner: gtk4::Spinner,
    albums_page: AlbumsPage,
    artists_page: ArtistsPage,
    playlists_page: PlaylistsPage,
}

impl SearchPage {
    pub fn new(
        client: Arc<Client>,
        on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
        on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
        on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
    ) -> Self {
        let stack = adw::ViewStack::new();

        let albums_page = new_albums_page(on_open_album.clone());
        let artists_page = new_artists_page(on_open_artist.clone());
        let playlists_page = new_playlists_page(on_open_playlist.clone());

        stack.add_titled(albums_page.widget(), Some("albums"), "Albums");
        stack.add_titled(artists_page.widget(), Some("artists"), "Artists");
        stack.add_titled(playlists_page.widget(), Some("playlists"), "Playlists");

        let switcher = adw::InlineViewSwitcher::builder()
            .stack(&stack)
            .css_classes(["round"])
            .halign(gtk4::Align::Center)
            .build();

        let spinner = gtk4::Spinner::new();
        spinner.set_visible(false);

        let spinner_box = gtk4::Box::builder()
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .build();
        spinner_box.append(&spinner);

        let overlay = gtk4::Overlay::new();
        overlay.set_vexpand(true);
        overlay.set_hexpand(true);
        overlay.set_child(Some(&stack));
        overlay.add_overlay(&spinner_box);

        let root = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(12)
            .vexpand(true)
            .hexpand(true)
            .build();

        root.append(&switcher);
        root.append(&overlay);

        Self {
            root,
            client,
            spinner,
            albums_page,
            artists_page,
            playlists_page,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.root
    }

    pub fn search(&mut self, query: String) {
        if query.is_empty() {
            return;
        }

        self.spinner.set_visible(true);
        self.spinner.start();

        let mut albums_page = self.albums_page.clone();
        let mut artists_page = self.artists_page.clone();
        let mut playlists_page = self.playlists_page.clone();
        let spinner = self.spinner.clone();
        let client = self.client.clone();

        albums_page.clear();
        artists_page.clear();
        playlists_page.clear();

        glib::MainContext::default().spawn_local(async move {
            match client.search(query).await {
                Ok(search) => {
                    let albums: Vec<_> = search.albums.into_iter().map(|x| x.into()).collect();
                    albums_page.load(albums);

                    artists_page.load(search.artists);

                    let playlists: Vec<_> =
                        search.playlists.into_iter().map(|x| x.into()).collect();
                    playlists_page.load(playlists);
                }
                Err(err) => tracing::error!("Search failed: {err}"),
            }

            spinner.stop();
            spinner.set_visible(false);
        });
    }
}
