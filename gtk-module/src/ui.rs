use std::{collections::HashSet, rc::Rc, sync::Arc};

use gtk4 as gtk;
use libadwaita as adw;

use adw::NavigationPage;
use controls_module::{
    controls::Controls,
    models::{AlbumSimple, Artist, PlaylistSimple, Track},
    tracklist::PlayingEntity,
};
use gtk::{gdk, gio, prelude::*};
use player_module::client::Client;

use crate::{
    UiEventSender,
    ui::{
        album_detail_page::AlbumHeaderInfo, artist_detail_page::ArtistHeaderInfo,
        playlist_detail_page::PlaylistHeaderInfo,
    },
};

pub mod album_detail_page;
pub mod albums_page;
pub mod app_shell;
pub mod artist_detail_page;
pub mod artists_page;
pub mod detail_page;
pub mod discover_page;
pub mod favorite_tracks_page;
pub mod grid_page;
pub mod now_playing_bar;
pub mod playlist_detail_page;
pub mod playlists_page;
pub mod preferences;
pub mod queue;
pub mod search_page;

pub fn set_picture_from_url(url: Option<&str>, picture: &gtk::Picture) {
    let Some(url) = url else {
        return;
    };

    let file = gio::File::for_uri(url);
    let picture = picture.clone();

    file.load_bytes_async(gio::Cancellable::NONE, move |result| match result {
        Ok((bytes, _)) => {
            if let Ok(texture) = gdk::Texture::from_bytes(&bytes) {
                picture.set_paintable(Some(&texture));
            }
        }
        Err(err) => {
            tracing::error!("Failed to load image: {err}");
        }
    });
}

pub fn build_album_tile(album: &AlbumSimple) -> adw::Bin {
    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();

    let cover = gtk::Picture::builder()
        .width_request(200)
        .height_request(200)
        .build();

    set_picture_from_url(Some(&album.image), &cover);
    let cover_frame = gtk::Frame::builder().child(&cover).build();

    let title = gtk::Label::builder()
        .label(&album.title)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(20)
        .build();

    let artist = gtk::Label::builder()
        .label(&album.artist.name)
        .xalign(0.0)
        .css_classes(vec!["dim-label"])
        .wrap(true)
        .max_width_chars(20)
        .build();

    vbox.append(&cover_frame);
    vbox.append(&title);
    vbox.append(&artist);

    adw::Bin::builder()
        .child(&vbox)
        .margin_end(12)
        .margin_bottom(12)
        .margin_top(12)
        .margin_start(12)
        .build()
}

pub fn build_playlist_tile(playlist: &PlaylistSimple) -> adw::Bin {
    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();

    let cover = gtk::Picture::builder()
        .width_request(200)
        .content_fit(gtk::ContentFit::Contain)
        .build();

    let aspect = gtk::AspectFrame::builder().child(&cover).build();

    set_picture_from_url(playlist.image.as_deref(), &cover);
    let cover_frame = gtk::Frame::builder().child(&aspect).build();

    let title = gtk::Label::builder()
        .label(&playlist.title)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(20)
        .build();

    vbox.append(&cover_frame);
    vbox.append(&title);

    adw::Bin::builder()
        .child(&vbox)
        .margin_end(12)
        .margin_bottom(12)
        .margin_top(12)
        .margin_start(12)
        .build()
}

pub fn build_artist_tile(artist: &Artist) -> adw::Bin {
    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::End)
        .spacing(6)
        .build();

    let cover = gtk::Picture::builder()
        .width_request(200)
        .content_fit(gtk::ContentFit::Contain)
        .build();

    let aspect = gtk::AspectFrame::builder().child(&cover).build();

    set_picture_from_url(artist.image.as_deref(), &cover);
    let cover_frame = gtk::Frame::builder().child(&aspect).build();

    let title = gtk::Label::builder()
        .label(&artist.name)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(20)
        .build();

    vbox.append(&cover_frame);
    vbox.append(&title);

    adw::Bin::builder()
        .child(&vbox)
        .margin_end(12)
        .margin_bottom(12)
        .margin_top(12)
        .margin_start(12)
        .build()
}

pub fn clickable_tile<F>(child: &gtk::Widget, on_click: F) -> gtk::Button
where
    F: Fn() + 'static,
{
    let button = gtk::Button::builder()
        .child(child)
        .css_classes(vec!["flat"])
        .focus_on_click(false)
        .has_frame(false)
        .build();

    button.connect_clicked(move |_| on_click());

    button
}

pub fn format_time(seconds: u32) -> String {
    let m = seconds / 60;
    let s = seconds % 60;
    format!("{m}:{s:02}")
}

pub enum DetailPageType {
    Album(String),
    Artist(u32),
    Playlist(u32),
}

impl DetailPageType {
    pub fn is_album(&self, id: &str) -> bool {
        match self {
            DetailPageType::Album(test_id) => test_id == id,
            _ => false,
        }
    }
    pub fn is_artist(&self, id: u32) -> bool {
        match self {
            DetailPageType::Artist(test_id) => test_id == &id,
            _ => false,
        }
    }
    pub fn is_playlist(&self, id: u32) -> bool {
        match self {
            DetailPageType::Playlist(test_id) => test_id == &id,
            _ => false,
        }
    }
}

pub trait DetailPage {
    fn page(&self) -> &NavigationPage;
    fn update_current_playing(&self, playing_entity: PlayingEntity);
    fn detail_type(&self) -> DetailPageType;
}

#[allow(clippy::too_many_arguments)]
pub fn build_track_row(
    track: &Track,
    show_cover: bool,
    show_artist: bool,
    show_album: bool,
    controls: Controls,
    client: Arc<Client>,
    ui_event_sender: UiEventSender,
    favorite_tracks: &HashSet<u32>,
    owned_playlists: &Vec<PlaylistSimple>,
) -> gtk::ListBoxRow {
    let track_row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();

    match show_cover {
        true => {
            let cover = gtk::Picture::new();

            set_picture_from_url(track.image.as_deref(), &cover);

            let cover_frame = gtk::Frame::builder().child(&cover).build();
            track_row_box.append(&cover_frame);
        }
        false => {
            let number_label = gtk::Label::builder()
                .label(format!("{:>2}", track.number))
                .xalign(0.0)
                .css_classes(vec!["dim-label"])
                .width_chars(3)
                .build();

            track_row_box.append(&number_label);
        }
    }

    let title_label = gtk::Label::builder()
        .label(track.title.clone())
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();

    match show_artist || show_album {
        true => {
            let title_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .hexpand(true)
                .build();

            title_box.append(&title_label);

            if show_artist && let Some(artist_name) = &track.artist_name {
                let artist_label = gtk::Label::builder()
                    .label(artist_name.clone())
                    .css_classes(vec!["dim-label"])
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build();

                title_box.append(&artist_label);
            }

            if show_album && let Some(album_title) = &track.album_title {
                let album_label = gtk::Label::builder()
                    .label(album_title.clone())
                    .css_classes(vec!["dim-label"])
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build();

                title_box.append(&album_label);
            }

            track_row_box.append(&title_box);
        }
        false => {
            track_row_box.append(&title_label);
        }
    }

    let duration_label = gtk::Label::builder()
        .label(format_time(track.duration_seconds))
        .xalign(1.0)
        .css_classes(vec!["dim-label"])
        .build();

    track_row_box.append(&duration_label);

    let is_favorite = favorite_tracks.contains(&track.id);
    let favorite_label = if is_favorite {
        "Remove from favorites"
    } else {
        "Add to favorites"
    };

    let menu = gio::Menu::new();
    menu.append(Some("Add to queue"), Some("track.add-to-queue"));
    menu.append(Some("Play next"), Some("track.play-next"));
    menu.append(Some(favorite_label), Some("track.toggle-favorite"));

    let playlist_section = gio::Menu::new();

    if owned_playlists.is_empty() {
        playlist_section.append(Some("No playlists"), None);
    } else {
        for playlist in owned_playlists {
            let item = gio::MenuItem::new(Some(&playlist.title), None);

            item.set_action_and_target_value(
                Some("track.add-to-playlist"),
                Some(&playlist.id.to_variant()),
            );

            playlist_section.append_item(&item);
        }
    }

    menu.append_submenu(Some("Add to playlist"), &playlist_section);

    let action_group = gio::SimpleActionGroup::new();

    let add_to_playlist_action =
        gio::SimpleAction::new("add-to-playlist", Some(&u32::static_variant_type()));

    add_to_playlist_action.connect_activate({
        let client = client.clone();
        let track_id = track.id;

        move |_, parameter| {
            let Some(playlist_id) = parameter.and_then(|p| p.get::<u32>()) else {
                tracing::warn!("Missing playlist id");
                return;
            };

            glib::MainContext::default().spawn_local({
                let client = client.clone();

                async move {
                    if let Err(err) = client.playlist_add_track(playlist_id, &[track_id]).await {
                        tracing::error!("Failed to add track to playlist: {err}");
                    }
                }
            });
        }
    });

    action_group.add_action(&add_to_playlist_action);

    let add_to_queue_action = gio::SimpleAction::new("add-to-queue", None);

    add_to_queue_action.connect_activate({
        let controls = controls.clone();
        let track = track.clone();

        move |_, _| {
            let track = track.clone();
            controls.add_tracks_to_queue(vec![track]);
        }
    });

    action_group.add_action(&add_to_queue_action);

    let toggle_favorite_action = gio::SimpleAction::new("toggle-favorite", None);

    toggle_favorite_action.connect_activate({
        let track_id = track.id;
        let client = client.clone();
        let ui_event_sender = ui_event_sender.clone();

        move |_, _| {
            glib::MainContext::default().spawn_local({
                let client = client.clone();
                let ui_event_sender = ui_event_sender.clone();

                async move {
                    let result = if is_favorite {
                        client.remove_favorite_track(track_id).await
                    } else {
                        client.add_favorite_track(track_id).await
                    };

                    if let Err(error) = result {
                        tracing::error!("{error}");
                    }

                    if let Err(error) = ui_event_sender.send(crate::UiEvent::FavoritesChanged) {
                        tracing::error!("{error}");
                    }
                }
            });
        }
    });

    action_group.add_action(&toggle_favorite_action);

    let play_next_action = gio::SimpleAction::new("play-next", None);

    play_next_action.connect_activate({
        let controls = controls.clone();
        let track = track.clone();

        move |_, _| {
            let track = track.clone();
            controls.play_tracks_next(vec![track]);
        }
    });

    action_group.add_action(&play_next_action);

    let popover_menu = gtk::PopoverMenu::from_model(Some(&menu));

    let menu_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("Track options")
        .popover(&popover_menu)
        .valign(gtk::Align::Center)
        .css_classes(vec!["flat"])
        .build();

    menu_button.insert_action_group("track", Some(&action_group));

    track_row_box.append(&menu_button);

    gtk::ListBoxRow::builder()
        .child(&track_row_box)
        .activatable(true)
        .selectable(true)
        .build()
}

fn section(title: &str, content: gtk::Widget) -> gtk::Box {
    let title = gtk::Label::builder()
        .label(title)
        .css_classes(["title-3"])
        .halign(gtk::Align::Start)
        .build();

    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
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
) -> gtk::Widget {
    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();

    for album in albums {
        let tile = build_album_tile(album).upcast();

        let album_id = album.id.clone();
        let on_open = on_open_album.clone();

        let button = clickable_tile(&tile, move || {
            on_open(AlbumHeaderInfo {
                id: album_id.clone(),
            });
        });

        box_.append(&button);
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&box_)
        .build();

    scroller.upcast()
}

fn artist_scroller(
    artists: &[Artist],
    on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
) -> gtk::Widget {
    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();

    for artist in artists {
        let tile = build_artist_tile(artist).upcast();

        let artist_id = artist.id;
        let on_open = on_open_artist.clone();

        let button = clickable_tile(&tile, move || {
            on_open(ArtistHeaderInfo { id: artist_id });
        });

        box_.append(&button);
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&box_)
        .build();

    scroller.upcast()
}

fn playlist_scroller(
    playlists: &[PlaylistSimple],
    on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();

    for playlist in playlists {
        let tile = build_playlist_tile(playlist).upcast();
        let id = playlist.id;

        let on_open = on_open_playlist.clone();

        let button = clickable_tile(&tile, move || {
            on_open(PlaylistHeaderInfo { id });
        });

        row.append(&button);
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&row)
        .build();

    scroller.upcast()
}
