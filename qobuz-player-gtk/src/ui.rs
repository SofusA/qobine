use gtk4::{Image, gdk, gio, prelude::*};
use libadwaita::{self as adw, NavigationPage};
use qobuz_player_controls::{
    models::{AlbumSimple, Artist, PlaylistSimple, Track},
    tracklist::PlayingEntity,
};

pub mod album_detail_page;
pub mod albums_page;
pub mod artist_detail_page;
pub mod artists_page;
pub mod detail_page;
pub mod favorites_button;
pub mod grid_page;
pub mod library_page;
pub mod now_playing_bar;
pub mod playlist_detail_page;
pub mod playlists_page;
pub mod search_page;

pub fn set_image_from_url(url: Option<&str>, image: &Image) {
    let Some(url) = url else {
        return;
    };

    let file = gio::File::for_uri(url);

    let image = image.clone();
    file.load_bytes_async(gio::Cancellable::NONE, move |result| match result {
        Ok((bytes, _)) => {
            if let Ok(texture) = gdk::Texture::from_bytes(&bytes) {
                image.set_paintable(Some(&texture));
            }
        }
        Err(err) => {
            tracing::error!("Failed to load image: {err}");
            image.set_icon_name(Some("image-missing"));
        }
    });
}

pub fn build_album_tile(album: &AlbumSimple) -> adw::Bin {
    let vbox = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(6)
        .build();

    let cover = gtk4::Image::builder().pixel_size(200).build();
    set_image_from_url(Some(&album.image), &cover);
    let cover_frame = gtk4::Frame::builder().child(&cover).build();
    cover_frame.add_css_class("card");

    let title = gtk4::Label::builder()
        .label(&album.title)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(20)
        .build();

    let artist = gtk4::Label::builder()
        .label(&album.artist.name)
        .xalign(0.0)
        .css_classes(vec![String::from("dim-label")])
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
    let vbox = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(6)
        .build();

    let cover = gtk4::Image::builder().pixel_size(200).build();
    set_image_from_url(playlist.image.as_deref(), &cover);
    let cover_frame = gtk4::Frame::builder().child(&cover).build();
    cover_frame.add_css_class("card");

    let title = gtk4::Label::builder()
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
    let vbox = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(6)
        .build();

    let cover = gtk4::Image::builder().pixel_size(200).build();
    set_image_from_url(artist.image.as_deref(), &cover);
    let cover_frame = gtk4::Frame::builder().child(&cover).build();
    cover_frame.add_css_class("card");

    let title = gtk4::Label::builder()
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

pub fn clickable_tile<F>(child: &gtk4::Widget, on_click: F) -> gtk4::Button
where
    F: Fn() + 'static,
{
    let button = gtk4::Button::builder().child(child).build();

    button.set_has_frame(false);
    button.add_css_class("flat");
    button.set_focus_on_click(false);
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

pub fn build_track_row(
    track: &Track,
    show_cover: bool,
    show_artist: bool,
    show_album: bool,
) -> gtk4::ListBoxRow {
    let track_row_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(12)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();

    match show_cover {
        true => {
            let cover = gtk4::Image::builder().pixel_size(50).build();
            set_image_from_url(track.image.as_deref(), &cover);
            let cover_frame = gtk4::Frame::builder().child(&cover).build();
            cover_frame.add_css_class("card");

            track_row_box.append(&cover_frame);
        }
        false => {
            let number_label = gtk4::Label::builder()
                .label(format!("{:>2}", track.number))
                .xalign(0.0)
                .css_classes(vec!["dim-label"])
                .width_chars(3)
                .build();
            track_row_box.append(&number_label);
        }
    }

    let title_label = gtk4::Label::builder()
        .label(track.title.clone())
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();
    match show_artist || show_album {
        true => {
            let title_box = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Vertical)
                .spacing(6)
                .build();
            title_box.append(&title_label);
            if show_artist && let Some(artist_name) = &track.artist_name {
                let artist_label = gtk4::Label::builder()
                    .label(artist_name.clone())
                    .css_classes(vec![String::from("dim-label")])
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk4::pango::EllipsizeMode::End)
                    .build();

                title_box.append(&artist_label);
            }

            if show_album && let Some(album_title) = &track.album_title {
                let album_label = gtk4::Label::builder()
                    .label(album_title.clone())
                    .css_classes(vec![String::from("dim-label")])
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk4::pango::EllipsizeMode::End)
                    .build();

                title_box.append(&album_label);
            }

            track_row_box.append(&title_box);
        }
        false => {
            track_row_box.append(&title_label);
        }
    }

    let duration_label = gtk4::Label::builder()
        .label(format_time(track.duration_seconds))
        .xalign(1.0)
        .css_classes(vec!["dim-label"])
        .build();

    track_row_box.append(&duration_label);

    gtk4::ListBoxRow::builder().child(&track_row_box).build()
}
