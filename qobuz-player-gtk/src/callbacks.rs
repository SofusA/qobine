use gtk4::glib;
use libadwaita as adw;
use qobuz_player_controls::{TracklistReceiver, client::Client, controls::Controls};
use std::{
    rc::{Rc, Weak},
    sync::Arc,
};

use crate::{
    UiEvent,
    ui::{
        DetailPage,
        album_detail_page::{AlbumDetailPage, AlbumHeaderInfo},
        artist_detail_page::{ArtistDetailPage, ArtistHeaderInfo},
        playlist_detail_page::{PlaylistDetailPage, PlaylistHeaderInfo},
    },
};

type OpenAlbumCb = Rc<dyn Fn(AlbumHeaderInfo) + 'static>;
type OpenArtistCb = Rc<dyn Fn(ArtistHeaderInfo) + 'static>;
type OpenPlaylistCb = Rc<dyn Fn(PlaylistHeaderInfo) + 'static>;

struct Callbacks {
    open_album: OpenAlbumCb,
    open_artist: OpenArtistCb,
    open_playlist: OpenPlaylistCb,
}

pub struct CallbackHandles {
    pub open_album: OpenAlbumCb,
    pub open_artist: OpenArtistCb,
    pub open_playlist: OpenPlaylistCb,
    _keepalive: Rc<Callbacks>,
}

pub fn build_callbacks(
    app_nav: adw::NavigationView,
    controls: Controls,
    client: Arc<Client>,
    detail_pages: Rc<std::cell::RefCell<Vec<Rc<dyn DetailPage>>>>,
    tracklist_receiver: TracklistReceiver,
    sender: async_channel::Sender<UiEvent>,
) -> CallbackHandles {
    let main_ctx = glib::MainContext::default();

    let callbacks: Rc<Callbacks> = Rc::new_cyclic(|weak_callbacks: &Weak<Callbacks>| {
        let weak_for_album = weak_callbacks.clone();
        let open_album: OpenAlbumCb = Rc::new({
            let main_ctx = main_ctx.clone();
            let app_nav = app_nav.clone();
            let controls = controls.clone();
            let client = client.clone();
            let detail_pages = detail_pages.clone();
            let tracklist_receiver = tracklist_receiver.clone();
            let sender = sender.clone();

            move |info: AlbumHeaderInfo| {
                let weak_for_album = weak_for_album.clone();
                let app_nav = app_nav.clone();
                let controls = controls.clone();
                let client = client.clone();
                let detail_pages = detail_pages.clone();
                let tracklist_receiver = tracklist_receiver.clone();
                let sender = sender.clone();

                main_ctx.invoke_local(move || {
                    let Some(callbacks) = weak_for_album.upgrade() else {
                        return;
                    };

                    let already_open = {
                        let pages = detail_pages.borrow();
                        pages.iter().any(|p| p.detail_type().is_album(&info.id))
                    };
                    if already_open {
                        return;
                    }

                    let open_artist = callbacks.open_artist.clone();

                    let detail = AlbumDetailPage::new(
                        info.id,
                        controls,
                        client,
                        tracklist_receiver,
                        sender,
                        open_artist,
                    );

                    app_nav.push(detail.page());
                    detail_pages.borrow_mut().push(Rc::new(detail));
                });
            }
        });

        let weak_for_artist = weak_callbacks.clone();
        let open_artist: OpenArtistCb = Rc::new({
            let main_ctx = main_ctx.clone();
            let app_nav = app_nav.clone();
            let controls = controls.clone();
            let client = client.clone();
            let detail_pages = detail_pages.clone();
            let tracklist_receiver = tracklist_receiver.clone();
            let sender = sender.clone();

            move |info: ArtistHeaderInfo| {
                let weak_for_artist = weak_for_artist.clone();
                let app_nav = app_nav.clone();
                let controls = controls.clone();
                let client = client.clone();
                let detail_pages = detail_pages.clone();
                let tracklist_receiver = tracklist_receiver.clone();
                let sender = sender.clone();

                main_ctx.invoke_local(move || {
                    let Some(callbacks) = weak_for_artist.upgrade() else {
                        return;
                    };

                    let already_open = {
                        let pages = detail_pages.borrow();
                        pages.iter().any(|p| p.detail_type().is_artist(info.id))
                    };
                    if already_open {
                        return;
                    }

                    let open_album = callbacks.open_album.clone();
                    let open_artist = callbacks.open_artist.clone();

                    let detail = ArtistDetailPage::new(
                        info.id,
                        controls,
                        client,
                        tracklist_receiver,
                        open_album,
                        open_artist,
                        sender,
                    );

                    app_nav.push(detail.page());
                    detail_pages.borrow_mut().push(Rc::new(detail));
                });
            }
        });

        let open_playlist: OpenPlaylistCb = Rc::new({
            let main_ctx = main_ctx.clone();
            let app_nav = app_nav.clone();
            let controls = controls.clone();
            let client = client.clone();
            let detail_pages = detail_pages.clone();
            let tracklist_receiver = tracklist_receiver.clone();
            let sender = sender.clone();

            move |info: PlaylistHeaderInfo| {
                let app_nav = app_nav.clone();
                let controls = controls.clone();
                let client = client.clone();
                let detail_pages = detail_pages.clone();
                let tracklist_receiver = tracklist_receiver.clone();
                let sender = sender.clone();

                main_ctx.invoke_local(move || {
                    let already_open = {
                        let pages = detail_pages.borrow();
                        pages.iter().any(|p| p.detail_type().is_playlist(info.id))
                    };
                    if already_open {
                        return;
                    }

                    let detail = PlaylistDetailPage::new(
                        info.id,
                        controls,
                        client,
                        tracklist_receiver,
                        sender,
                    );

                    app_nav.push(detail.page());
                    detail_pages.borrow_mut().push(Rc::new(detail));
                });
            }
        });

        Callbacks {
            open_album,
            open_artist,
            open_playlist,
        }
    });

    CallbackHandles {
        open_album: callbacks.open_album.clone(),
        open_artist: callbacks.open_artist.clone(),
        open_playlist: callbacks.open_playlist.clone(),
        _keepalive: callbacks,
    }
}
