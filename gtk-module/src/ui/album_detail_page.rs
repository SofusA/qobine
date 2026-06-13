use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use glib::WeakRef;
use gtk::{gio, prelude::*};
use gtk4 as gtk;
use libadwaita as adw;

use controls_module::{TracklistReceiver, controls::Controls, tracklist::PlayingEntity};
use player_module::client::Client;

use crate::{
    UiEventSender,
    ui::{
        DetailPage, DetailPageType, album_scroller,
        artist_detail_page::ArtistHeaderInfo,
        build_track_row, clickable_tile,
        detail_page::{
            DetailType, build_detail_header, build_detail_scaffold, populate_playlist_menu,
        },
        format_time, section, set_picture_from_url,
    },
};

#[derive(Clone, Debug)]
pub struct AlbumHeaderInfo {
    pub id: String,
}

pub struct AlbumDetailPage {
    page: adw::NavigationPage,

    client: Arc<Client>,
    controls: Controls,
    tracklist_receiver: TracklistReceiver,

    album_id: String,

    stack: gtk::Stack,

    cover: gtk::Picture,
    title: gtk::Label,
    artist_box: gtk::Box,
    meta: gtk::Label,
    playlist_menu: gio::Menu,

    content: gtk::Box,
    tracks_list: gtk::ListBox,

    track_rows: Rc<RefCell<HashMap<u32, WeakRef<gtk::ListBoxRow>>>>,
    current_selected_id: Rc<RefCell<Option<u32>>>,
    on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
    on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
    loaded: RefCell<bool>,
    ui_event_sender: UiEventSender,
}

impl AlbumDetailPage {
    pub fn new(
        album_id: String,
        controls: Controls,
        client: Arc<Client>,
        tracklist_receiver: TracklistReceiver,
        ui_event_sender: UiEventSender,
        on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
        on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
    ) -> Self {
        let empty_title = gtk::Box::builder().hexpand(true).build();
        let nav_bar = adw::HeaderBar::builder().title_widget(&empty_title).build();

        let title = gtk::Label::builder()
            .wrap(true)
            .css_classes(vec!["title-1"])
            .build();

        let artist_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .halign(gtk::Align::Center)
            .build();

        let meta = gtk::Label::builder()
            .wrap(true)
            .css_classes(vec!["dim-label"])
            .build();

        let play_button = gtk::Button::builder()
            .label("Play")
            .icon_name("media-playback-start-symbolic")
            .css_classes(vec!["suggested-action", "pill"])
            .build();

        play_button.connect_clicked({
            let controls = controls.clone();
            let album_id = album_id.clone();

            move |_| {
                controls.play_album(&album_id, 0);
            }
        });

        let header = build_detail_header(
            client.clone(),
            controls.clone(),
            ui_event_sender.clone(),
            vec![
                title.clone().upcast(),
                artist_box.clone().upcast(),
                meta.clone().upcast(),
            ],
            vec![play_button],
            DetailType::Album(album_id.clone()),
        );

        let scaffold = build_detail_scaffold(&header.header_section, {
            let controls = controls.clone();
            let album_id = album_id.clone();
            move |index| {
                controls.play_album(&album_id, index);
            }
        });

        let cover = header.cover;
        let stack = scaffold.stack;
        let tracks_list = scaffold.tracks_list;
        let menu = header.playlist_menu;

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&nav_bar);
        toolbar.set_content(Some(&stack));

        let page = adw::NavigationPage::builder()
            .title("Album")
            .child(&toolbar)
            .build();

        let s = Self {
            page,
            client,
            controls,
            tracklist_receiver,
            content: scaffold.content,
            album_id,
            stack,
            cover,
            title,
            artist_box,
            meta,
            tracks_list,
            loaded: RefCell::new(false),
            track_rows: Rc::new(RefCell::new(HashMap::new())),
            current_selected_id: Rc::new(RefCell::new(None)),
            on_open_artist,
            on_open_album,
            ui_event_sender,
            playlist_menu: menu,
        };

        s.load_album();

        s
    }

    fn load_album(&self) {
        if *self.loaded.borrow() {
            return;
        }
        *self.loaded.borrow_mut() = true;

        let client = self.client.clone();
        let ui_event_sender = self.ui_event_sender.clone();
        let controls = self.controls.clone();
        let tracklist_receiver = self.tracklist_receiver.clone();
        let album_id = self.album_id.clone();

        let stack = self.stack.clone();
        let content = self.content.clone();
        let cover = self.cover.clone();
        let title = self.title.clone();
        let artist_box = self.artist_box.clone();
        let meta = self.meta.clone();
        let tracks_list = self.tracks_list.clone();
        let track_rows = self.track_rows.clone();
        let current_selected_id = self.current_selected_id.clone();
        let on_open_artist = self.on_open_artist.clone();
        let on_open_album = self.on_open_album.clone();

        stack.set_visible_child_name("loading");

        populate_playlist_menu(self.playlist_menu.clone(), client.clone());

        glib::MainContext::default().spawn_local(async move {
            match tokio::try_join!(client.album(&album_id), client.suggested_albums(&album_id)) {
                Ok((album, suggestions)) => {
                    title.set_label(&album.title);

                    while let Some(child) = artist_box.first_child() {
                        artist_box.remove(&child);
                    }

                    let artist_label = gtk::Label::builder()
                        .label(&album.artist.name)
                        .wrap(true)
                        .css_classes(vec!["title-3", "dim-label"])
                        .build();

                    let artist_id = album.artist.id;
                    let button = clickable_tile(&artist_label.upcast(), move || {
                        on_open_artist(ArtistHeaderInfo { id: artist_id });
                    });

                    artist_box.append(&button);

                    let year_string = album.release_year.to_string();
                    let duration_string = format_time(album.duration_seconds);
                    let mut meta_info = vec![year_string.as_str(), &duration_string];

                    if album.explicit {
                        meta_info.push("Explicit");
                    }
                    if album.hires_available {
                        meta_info.push("Hi-res");
                    }

                    let meta_info_label = meta_info.join(" • ");

                    meta.set_label(&meta_info_label);

                    set_picture_from_url(Some(&album.image), &cover);

                    clear_listbox(&tracks_list);

                    let favorites = client.favorites().await.unwrap_or_default();
                    let favorite_tracks = favorites.tracks.into_iter().map(|x| x.id).collect();
                    let owned_playlists = favorites
                        .playlists
                        .into_iter()
                        .filter(|x| x.is_owned)
                        .map(|x| x.into())
                        .collect();

                    for track in album.tracks {
                        let row = build_track_row(
                            &track,
                            false,
                            false,
                            false,
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

                    if !suggestions.is_empty() {
                        content.append(&section(
                            "Similar albums",
                            album_scroller(&suggestions, on_open_album.clone()),
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
                    tracing::error!("Failed to load album {album_id}: {err}");

                    clear_listbox(&tracks_list);
                    stack.set_visible_child_name("content");
                }
            }
        });
    }
}

impl DetailPage for AlbumDetailPage {
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
        DetailPageType::Album(self.album_id.clone())
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
