use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use glib::WeakRef;
use gtk4::prelude::*;
use libadwaita as adw;

use qobuz_player_controls::{
    TracklistReceiver,
    client::Client,
    controls::Controls,
    models::{AlbumSimple, Artist},
    tracklist::PlayingEntity,
};
use tokio::sync::mpsc;

use crate::{
    UiEvent,
    ui::{
        DetailPage, DetailPageType,
        album_detail_page::AlbumHeaderInfo,
        build_album_tile, build_artist_tile, build_track_row, clickable_tile,
        detail_page::{build_detail_header, build_detail_scaffold},
        favorites_button::{FavoriteButtonType, new_favorite_button},
        set_image_from_url,
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

    stack: gtk4::Stack,

    cover: gtk4::Image,
    name: gtk4::Label,

    content: gtk4::Box,
    tracks_list: gtk4::ListBox,

    track_rows: Rc<RefCell<HashMap<u32, WeakRef<gtk4::ListBoxRow>>>>,
    current_selected_id: Rc<RefCell<Option<u32>>>,

    loaded: RefCell<bool>,
}

impl ArtistDetailPage {
    pub fn new(
        artist_id: u32,
        controls: Controls,
        client: Arc<Client>,
        tracklist_receiver: TracklistReceiver,
        on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
        on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
        library_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Self {
        let empty_title = gtk4::Box::builder().hexpand(true).build();
        let nav_bar = adw::HeaderBar::builder().title_widget(&empty_title).build();

        let name = gtk4::Label::builder()
            .css_classes(["title-1"])
            .wrap(true)
            .build();

        let play_button = gtk4::Button::builder()
            .label("Play")
            .icon_name("media-playback-start-symbolic")
            .css_classes(vec!["suggested-action", "pill"])
            .build();

        {
            let controls = controls.clone();
            play_button.connect_clicked(move |_| {
                controls.play_top_tracks(artist_id, 0);
            });
        }

        let header = build_detail_header(
            200,
            vec![name.clone().upcast()],
            vec![play_button.clone().upcast()],
            {
                let client = client.clone();
                let library_tx = library_tx.clone();
                move || {
                    new_favorite_button(client, FavoriteButtonType::Artist(artist_id), library_tx)
                        .upcast()
                }
            },
        );

        let scaffold = build_detail_scaffold(&header.header_section);

        let cover = header.cover.clone();
        let stack = scaffold.stack.clone();
        let content = scaffold.content.clone();
        let tracks_list = scaffold.tracks_list.clone();

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
            tracks_list,
            loaded: RefCell::new(false),
            track_rows: Rc::new(RefCell::new(HashMap::new())),
            current_selected_id: Rc::new(RefCell::new(None)),
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

        glib::MainContext::default().spawn_local(async move {
            match client.artist_page(artist_id).await {
                Ok(artist) => {
                    name.set_label(&artist.name);
                    set_image_from_url(artist.image.as_deref(), &cover);

                    clear_listbox(&tracks_list);

                    for (idx, track) in artist.top_tracks.iter().take(10).enumerate() {
                        let row = build_track_row(track, true, false, true);

                        let weak = glib::WeakRef::new();
                        weak.set(Some(&row));

                        weak.set(Some(&row));
                        track_rows.borrow_mut().insert(track.id, weak);

                        let controls = controls.clone();
                        let click_index = idx as i32;

                        let click = gtk4::GestureClick::new();
                        click.connect_pressed(move |_, _, _, _| {
                            controls.play_top_tracks(artist_id, click_index as usize);
                        });

                        row.add_controller(click);
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

fn section(title: &str, content: gtk4::Widget) -> gtk4::Box {
    let title = gtk4::Label::builder()
        .label(title)
        .css_classes(["title-3"])
        .halign(gtk4::Align::Start)
        .build();

    let box_ = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(12)
        .margin_top(24)
        .build();

    box_.append(&title);
    box_.append(&content);

    box_
}

fn album_scroller(
    albums: &[AlbumSimple],
    on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
) -> gtk4::Widget {
    let box_ = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();

    for album in albums {
        let tile = build_album_tile(album).upcast::<gtk4::Widget>();

        let album_id = album.id.clone();
        let on_open = on_open_album.clone();

        let button = clickable_tile(&tile, move || {
            on_open(AlbumHeaderInfo {
                id: album_id.clone(),
            });
        });

        box_.append(&button);
    }

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Automatic)
        .vscrollbar_policy(gtk4::PolicyType::Never)
        .child(&box_)
        .build();

    scroller.upcast()
}

fn artist_scroller(
    artists: &[Artist],
    on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
) -> gtk4::Widget {
    let box_ = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();

    for artist in artists {
        let tile = build_artist_tile(artist).upcast::<gtk4::Widget>();

        let artist_id = artist.id;
        let on_open = on_open_artist.clone();

        let button = clickable_tile(&tile, move || {
            on_open(ArtistHeaderInfo { id: artist_id });
        });

        box_.append(&button);
    }

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Automatic)
        .vscrollbar_policy(gtk4::PolicyType::Never)
        .child(&box_)
        .build();

    scroller.upcast()
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
