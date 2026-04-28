use std::{cell::RefCell, rc::Rc, sync::Arc};

use gtk4::Spinner;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;

use qobuz_player_controls::client::Client;

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

pub struct LibraryPage {
    root: gtk4::Box,
    client: Arc<Client>,
    spinner: Spinner,
    waiting_label: gtk4::Label,
    albums_page: Rc<RefCell<AlbumsPage>>,
    artists_page: Rc<RefCell<ArtistsPage>>,
    playlists_page: Rc<RefCell<PlaylistsPage>>,
}

impl LibraryPage {
    pub fn new(
        client: Arc<Client>,
        on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
        on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
        on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
    ) -> Self {
        let albums_page = Rc::new(RefCell::new(new_albums_page(on_open_album)));
        let artists_page = Rc::new(RefCell::new(new_artists_page(on_open_artist)));
        let playlists_page = Rc::new(RefCell::new(new_playlists_page(on_open_playlist)));

        let stack = adw::ViewStack::new();

        stack.add_titled(albums_page.borrow().widget(), Some("albums"), "Albums");
        stack.add_titled(artists_page.borrow().widget(), Some("artists"), "Artists");
        stack.add_titled(
            playlists_page.borrow().widget(),
            Some("playlists"),
            "Playlists",
        );

        let switcher = adw::InlineViewSwitcher::builder()
            .stack(&stack)
            .css_classes(["round"])
            .halign(gtk4::Align::Center)
            .build();

        let search_entry = gtk4::SearchEntry::builder()
            .placeholder_text("Filter…")
            .build();

        {
            let albums_page = albums_page.clone();
            let artists_page = artists_page.clone();
            let playlists_page = playlists_page.clone();

            search_entry.connect_changed(move |entry| {
                let query = entry.text();

                albums_page.borrow().filter(&query);
                artists_page.borrow().filter(&query);
                playlists_page.borrow().filter(&query);
            });
        }

        let top_box = gtk4::Box::builder()
            .halign(gtk4::Align::Center)
            .spacing(12)
            .build();
        top_box.append(&search_entry);
        top_box.append(&switcher);

        let spinner = gtk4::Spinner::new();
        spinner.start();
        spinner.set_visible(true);

        let spinner_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .build();
        spinner_box.append(&spinner);

        let waiting_label = gtk4::Label::builder()
            .label("Waiting for login...")
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .visible(true)
            .build();
        spinner_box.append(&waiting_label);

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

        root.append(&top_box);
        root.append(&overlay);

        Self {
            root,
            client,
            spinner,
            waiting_label,
            albums_page,
            artists_page,
            playlists_page,
        }
    }

    pub fn reload(&self) {
        reload(
            self.client.clone(),
            &self.spinner,
            &self.waiting_label,
            &self.albums_page,
            &self.artists_page,
            &self.playlists_page,
        )
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.root
    }
}

pub fn reload(
    client: Arc<Client>,
    spinner: &Spinner,
    waiting_label: &gtk4::Label,
    albums_page: &Rc<RefCell<AlbumsPage>>,
    artists_page: &Rc<RefCell<ArtistsPage>>,
    playlists_page: &Rc<RefCell<PlaylistsPage>>,
) {
    let albums_page = albums_page.clone();
    let artists_page = artists_page.clone();
    let playlists_page = playlists_page.clone();
    let spinner = spinner.clone();
    let waiting_label = waiting_label.clone();

    waiting_label.set_visible(false);
    spinner.set_visible(true);
    spinner.start();

    glib::MainContext::default().spawn_local({
        async move {
            match client.favorites().await {
                Ok(favorites) => {
                    spinner.set_visible(false);
                    spinner.stop();

                    albums_page.borrow_mut().load(favorites.albums);

                    artists_page.borrow_mut().load(favorites.artists);

                    playlists_page
                        .borrow_mut()
                        .load(favorites.playlists.into_iter().map(|x| x.into()).collect());
                }
                Err(err) => {
                    spinner.set_visible(false);
                    spinner.stop();
                    tracing::error!("{err}");
                }
            }
        }
    });
}
