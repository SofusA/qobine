use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use glib::WeakRef;
use gtk4 as gtk;
use gtk4::{gio, prelude::*};
use libadwaita as adw;

use controls_module::{TracklistReceiver, controls::Controls, tracklist::PlayingEntity};
use player_module::client::Client;

use crate::ui::set_picture_from_url;
use crate::{
    UiEventSender,
    ui::{
        DetailPage, DetailPageType,
        album_detail_page::AlbumHeaderInfo,
        album_scroller, artist_scroller, build_track_row,
        detail_page::{
            DetailType, build_detail_header, build_detail_scaffold, populate_playlist_menu,
        },
        section,
    },
};

#[derive(Clone, Debug)]
pub struct ArtistHeaderInfo {
    pub id: u32,
}

pub struct ArtistDetailPage {
    page: adw::NavigationPage,

    client: Arc<Client>,
    controls: Controls,
    tracklist_receiver: TracklistReceiver,
    artist_id: u32,

    on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
    on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,

    stack: gtk::Stack,

    cover: gtk::Picture,
    name: gtk::Label,
    playlist_menu: gio::Menu,

    content: gtk::Box,
    tracks_list: gtk::ListBox,

    track_rows: Rc<RefCell<HashMap<u32, WeakRef<gtk::ListBoxRow>>>>,
    current_selected_id: Rc<RefCell<Option<u32>>>,

    loaded: RefCell<bool>,
    ui_event_sender: UiEventSender,
}

impl ArtistDetailPage {
    pub fn new(
        artist_id: u32,
        controls: Controls,
        client: Arc<Client>,
        tracklist_receiver: TracklistReceiver,
        on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
        on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
        ui_event_sender: UiEventSender,
    ) -> Self {
        let empty_title = gtk::Box::builder().hexpand(true).build();
        let nav_bar = adw::HeaderBar::builder().title_widget(&empty_title).build();

        let name = gtk::Label::builder()
            .css_classes(["title-1"])
            .wrap(true)
            .build();

        let play_button = gtk::Button::builder()
            .label("Play")
            .icon_name("media-playback-start-symbolic")
            .css_classes(vec!["suggested-action", "pill"])
            .build();

        play_button.connect_clicked({
            let controls = controls.clone();
            move |_| {
                controls.play_top_tracks(artist_id, 0);
            }
        });

        let spacer = gtk::Box::builder().height_request(50).build();

        let header = build_detail_header(
            client.clone(),
            controls.clone(),
            ui_event_sender.clone(),
            vec![name.clone().upcast(), spacer.upcast()],
            vec![play_button],
            DetailType::Artist(artist_id),
        );

        let scaffold = build_detail_scaffold(&header.header_section, {
            let controls = controls.clone();
            move |index| {
                controls.play_top_tracks(artist_id, index);
            }
        });

        let cover = header.cover;
        let stack = scaffold.stack;
        let content = scaffold.content;
        let tracks_list = scaffold.tracks_list;
        let menu = header.playlist_menu;

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&nav_bar);
        toolbar.set_content(Some(&stack));

        let page = adw::NavigationPage::builder()
            .title("Artist")
            .child(&toolbar)
            .build();

        let s = Self {
            page,
            client,
            controls,
            tracklist_receiver,
            artist_id,
            stack,
            on_open_album,
            on_open_artist,
            content,
            cover,
            name,
            playlist_menu: menu,
            tracks_list,
            loaded: RefCell::new(false),
            track_rows: Rc::new(RefCell::new(HashMap::new())),
            current_selected_id: Rc::new(RefCell::new(None)),
            ui_event_sender,
        };

        s.load_artist();

        s
    }

    fn load_artist(&self) {
        if *self.loaded.borrow() {
            return;
        }
        *self.loaded.borrow_mut() = true;

        let client = self.client.clone();
        let ui_event_sender = self.ui_event_sender.clone();
        let artist_id = self.artist_id;

        let stack = self.stack.clone();

        let cover = self.cover.clone();
        let name = self.name.clone();
        let tracks_list = self.tracks_list.clone();
        let track_rows = self.track_rows.clone();
        let current_selected_id = self.current_selected_id.clone();
        let controls = self.controls.clone();
        let tracklist_receiver = self.tracklist_receiver.clone();

        let on_open_album = self.on_open_album.clone();
        let on_open_artist = self.on_open_artist.clone();

        let content = self.content.clone();

        stack.set_visible_child_name("loading");
        populate_playlist_menu(self.playlist_menu.clone(), client.clone());

        glib::MainContext::default().spawn_local(async move {
            match client.artist_page(artist_id).await {
                Ok(artist) => {
                    name.set_label(&artist.name);
                    set_picture_from_url(artist.image.as_deref(), &cover);

                    clear_listbox(&tracks_list);

                    let favorites = client.favorites().await.unwrap_or_default();
                    let favorite_tracks = favorites.tracks.into_iter().map(|x| x.id).collect();
                    let owned_playlists = favorites
                        .playlists
                        .into_iter()
                        .filter(|x| x.is_owned)
                        .map(|x| x.into())
                        .collect();

                    for track in artist.top_tracks.iter().take(10) {
                        let row = build_track_row(
                            track,
                            true,
                            false,
                            true,
                            controls.clone(),
                            client.clone(),
                            ui_event_sender.clone(),
                            &favorite_tracks,
                            &owned_playlists,
                        );

                        let weak = glib::WeakRef::new();
                        weak.set(Some(&row));

                        weak.set(Some(&row));
                        track_rows.borrow_mut().insert(track.id, weak);

                        tracks_list.append(&row);
                    }

                    if !artist.albums.is_empty() {
                        content.append(&section(
                            "Albums",
                            album_scroller(&artist.albums, on_open_album.clone()),
                        ));
                    }

                    if !artist.singles.is_empty() {
                        content.append(&section(
                            "Singles",
                            album_scroller(&artist.singles, on_open_album.clone()),
                        ));
                    }

                    if !artist.live.is_empty() {
                        content.append(&section(
                            "Live",
                            album_scroller(&artist.live, on_open_album.clone()),
                        ));
                    }

                    if !artist.compilations.is_empty() {
                        content.append(&section(
                            "Compilations",
                            album_scroller(&artist.compilations, on_open_album.clone()),
                        ));
                    }

                    if !artist.similar_artists.is_empty() {
                        content.append(&section(
                            "Similar Artists",
                            artist_scroller(&artist.similar_artists, on_open_artist.clone()),
                        ));
                    }

                    let playing_entity = tracklist_receiver.borrow().current_playing_entity();
                    if let Some(playing_entity) = playing_entity {
                        update_current_playing(
                            &playing_entity,
                            &current_selected_id,
                            &tracks_list,
                            &track_rows,
                        );
                    }

                    stack.set_visible_child_name("content");
                }
                Err(err) => {
                    tracing::error!("Failed to load artist {artist_id}: {err}");
                    stack.set_visible_child_name("content");
                }
            }
        });
    }
}

impl DetailPage for ArtistDetailPage {
    fn page(&self) -> &adw::NavigationPage {
        &self.page
    }

    fn update_current_playing(&self, playing_entity: PlayingEntity) {
        update_current_playing(
            &playing_entity,
            &self.current_selected_id,
            &self.tracks_list,
            &self.track_rows,
        );
    }

    fn detail_type(&self) -> DetailPageType {
        DetailPageType::Artist(self.artist_id)
    }
}

fn update_current_playing(
    playing_entity: &PlayingEntity,
    current_selected_id: &Rc<RefCell<Option<u32>>>,
    tracks_list: &gtk::ListBox,
    track_rows: &Rc<RefCell<HashMap<u32, WeakRef<gtk::ListBoxRow>>>>,
) {
    let track_id = match playing_entity {
        PlayingEntity::Track(t) => Some(t.id),
        PlayingEntity::Playlist(p) => Some(p.track_id),
    };

    *current_selected_id.borrow_mut() = track_id;

    let Some(track_id) = track_id else {
        tracks_list.unselect_all();
        return;
    };

    if let Some(weak) = track_rows.borrow().get(&track_id) {
        if let Some(row) = weak.upgrade() {
            tracks_list.select_row(Some(&row));
        } else {
            tracks_list.unselect_all();
        }
    } else {
        tracks_list.unselect_all();
    }
}

fn clear_listbox(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}
