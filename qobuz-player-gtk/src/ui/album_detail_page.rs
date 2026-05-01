use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use glib::WeakRef;
use gtk4::prelude::*;
use libadwaita as adw;

use qobuz_player_controls::{
    TracklistReceiver, client::Client, controls::Controls, tracklist::PlayingEntity,
};
use tokio::sync::mpsc;

use crate::{
    UiEvent,
    ui::{
        DetailPage, DetailPageType,
        artist_detail_page::ArtistHeaderInfo,
        build_track_row, clickable_tile,
        favorites_button::{FavoriteButtonType, new_favorite_button},
        format_time, set_image_from_url,
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

    stack: gtk4::Stack,

    cover: gtk4::Image,
    title: gtk4::Label,
    artist_box: gtk4::Box,
    meta: gtk4::Label,

    tracks_list: gtk4::ListBox,

    track_rows: Rc<RefCell<HashMap<u32, WeakRef<gtk4::ListBoxRow>>>>,
    current_selected_id: Rc<RefCell<Option<u32>>>,
    on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
    loaded: RefCell<bool>,
}

impl AlbumDetailPage {
    pub fn new(
        album_id: String,
        controls: Controls,
        client: Arc<Client>,
        tracklist_receiver: TracklistReceiver,
        library_tx: mpsc::UnboundedSender<UiEvent>,
        on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
    ) -> Self {
        let empty_title = gtk4::Box::builder().hexpand(true).build();

        let nav_bar = adw::HeaderBar::builder().title_widget(&empty_title).build();

        let spinner = gtk4::Spinner::new();
        spinner.start();
        let spinner_box = gtk4::Box::builder()
            .vexpand(true)
            .hexpand(true)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .build();
        spinner_box.append(&spinner);

        let cover = gtk4::Image::builder().pixel_size(400).build();
        let cover_frame = gtk4::Frame::builder().child(&cover).build();
        cover_frame.add_css_class("card");

        let title = gtk4::Label::builder()
            .wrap(true)
            .css_classes(vec!["title-1"])
            .build();

        let artist_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .halign(gtk4::Align::Center)
            .build();

        let meta = gtk4::Label::builder()
            .wrap(true)
            .css_classes(vec!["dim-label"])
            .build();

        let play_button = gtk4::Button::builder()
            .label("Play")
            .icon_name("media-playback-start-symbolic")
            .css_classes(vec!["suggested-action", "pill"])
            .build();

        {
            let controls = controls.clone();
            let album_id = album_id.clone();
            play_button.connect_clicked(move |_| {
                controls.play_album(&album_id, 0);
            });
        }

        let favorites_button = new_favorite_button(
            client.clone(),
            FavoriteButtonType::Album(album_id.clone()),
            library_tx,
        );

        let button_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .halign(gtk4::Align::Center)
            .spacing(12)
            .build();
        button_box.append(&play_button);
        button_box.append(&favorites_button);

        let header_text = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .halign(gtk4::Align::Center)
            .spacing(12)
            .hexpand(true)
            .build();
        header_text.append(&title);
        header_text.append(&artist_box);
        header_text.append(&meta);

        header_text.append(&button_box);

        let header_section = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(18)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();

        header_section.append(&cover_frame);
        header_section.append(&header_text);

        let tracks_list = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .activate_on_single_click(true)
            .css_classes(vec!["boxed-list"])
            .margin_start(18)
            .margin_end(18)
            .margin_bottom(18)
            .build();

        let content = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .vexpand(true)
            .hexpand(true)
            .build();

        content.append(&header_section);
        content.append(&tracks_list);

        let clamp = adw::Clamp::builder()
            .maximum_size(900)
            .tightening_threshold(700)
            .child(&content)
            .build();

        let scroller = gtk4::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&clamp)
            .build();

        let stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&spinner_box, Some("loading"));
        stack.add_named(&scroller, Some("content"));
        stack.set_visible_child_name("loading");

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
        let controls = self.controls.clone();
        let tracklist_receiver = self.tracklist_receiver.clone();
        let album_id = self.album_id.clone();

        let stack = self.stack.clone();
        let cover = self.cover.clone();
        let title = self.title.clone();
        let artist_box = self.artist_box.clone();
        let meta = self.meta.clone();
        let tracks_list = self.tracks_list.clone();
        let track_rows = self.track_rows.clone();
        let current_selected_id = self.current_selected_id.clone();
        let on_open_artist = self.on_open_artist.clone();

        stack.set_visible_child_name("loading");

        glib::MainContext::default().spawn_local(async move {
            match client.album(&album_id).await {
                Ok(album) => {
                    title.set_label(&album.title);

                    while let Some(child) = artist_box.first_child() {
                        artist_box.remove(&child);
                    }

                    let artist_label = gtk4::Label::builder()
                        .label(&album.artist.name)
                        .wrap(true)
                        .css_classes(vec!["title-3", "dim-label"])
                        .build();

                    let artist_id = album.artist.id;
                    let button = clickable_tile(&artist_label.upcast(), move || {
                        on_open_artist(ArtistHeaderInfo { id: artist_id });
                    });

                    artist_box.append(&button);

                    let year_str = album.release_year.to_string();
                    let dur_str = format_time(album.duration_seconds);
                    meta.set_label(&format!("{year_str} • {dur_str}"));

                    set_image_from_url(Some(&album.image), &cover);

                    clear_listbox(&tracks_list);

                    for (idx, track) in album.tracks.iter().enumerate() {
                        let row = build_track_row(track, false, false, false);

                        let weak = glib::WeakRef::new();
                        weak.set(Some(&row));

                        weak.set(Some(&row));
                        track_rows.borrow_mut().insert(track.id, weak);

                        let controls = controls.clone();
                        let album_id = album_id.clone();
                        let click_index = idx;

                        let click = gtk4::GestureClick::new();
                        click.connect_pressed(move |_, _, _, _| {
                            controls.play_album(&album_id, click_index);
                        });

                        row.add_controller(click);
                        tracks_list.append(&row);
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
    tracks_list: &gtk4::ListBox,
    track_rows: &Rc<RefCell<HashMap<u32, WeakRef<gtk4::ListBoxRow>>>>,
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

fn clear_listbox(list: &gtk4::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}
