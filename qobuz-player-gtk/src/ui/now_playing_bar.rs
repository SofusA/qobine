use std::{rc::Rc, time::Duration};

use libadwaita::prelude::*;
use qobuz_player_controls::{
    Status,
    controls::Controls,
    tracklist::{Tracklist, TracklistType},
};

use crate::ui::{
    album_detail_page::AlbumHeaderInfo, artist_detail_page::ArtistHeaderInfo, clickable_tile,
    format_time, playlist_detail_page::PlaylistHeaderInfo, set_image_from_url,
};

#[derive(Clone)]
pub struct NowPlayingBar {
    pub revealer: gtk4::Revealer,
    track_title_label: gtk4::Label,
    subtitle_box: gtk4::Box,
    cover: gtk4::Image,
    pub play_button: gtk4::Button,

    progress_scale: gtk4::Scale,
    progress_current_label: gtk4::Label,
    progress_total_label: gtk4::Label,
    on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
    on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
    on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
}

impl NowPlayingBar {
    pub fn new(
        controls: Controls,
        on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
        on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
        on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
    ) -> Self {
        let title_label = gtk4::Label::builder()
            .halign(gtk4::Align::Center)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .wrap(false)
            .build();
        title_label.add_css_class("title-3");

        let subtitle_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .halign(gtk4::Align::Center)
            .build();

        let text_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .halign(gtk4::Align::Center)
            .spacing(4)
            .hexpand(true)
            .build();
        text_box.append(&title_label);
        text_box.append(&subtitle_box);

        let controls_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk4::Align::Center)
            .build();

        let controls_prev = controls.clone();
        let prev_button = gtk4::Button::builder()
            .icon_name("media-seek-backward-symbolic")
            .build();
        prev_button.add_css_class("flat");
        prev_button.connect_clicked(move |_| controls_prev.previous());

        let controls_play_pause = controls.clone();
        let play_button = gtk4::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .build();
        play_button.add_css_class("flat");
        play_button.connect_clicked(move |_| controls_play_pause.play_pause());

        let controls_next = controls.clone();
        let next_button = gtk4::Button::builder()
            .icon_name("media-seek-forward-symbolic")
            .build();
        next_button.add_css_class("flat");
        next_button.connect_clicked(move |_| controls_next.next());

        controls_box.append(&prev_button);
        controls_box.append(&play_button);
        controls_box.append(&next_button);

        let progress_current_label = gtk4::Label::builder()
            .label("0:00")
            .width_chars(6)
            .xalign(0.0)
            .build();

        let progress_total_label = gtk4::Label::builder()
            .label("0:00")
            .width_chars(6)
            .xalign(1.0)
            .build();

        let progress_scale = gtk4::Scale::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .hexpand(true)
            .draw_value(false)
            .focusable(false)
            .build();

        let controls_seek = controls.clone();
        progress_scale.connect_change_value(move |_, _, value| {
            controls_seek.seek(Duration::from_millis(value as u64));
            glib::Propagation::Stop
        });

        let progress_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .hexpand(true)
            .build();

        progress_box.append(&progress_current_label);
        progress_box.append(&progress_scale);
        progress_box.append(&progress_total_label);

        let left_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(8)
            .hexpand(true)
            .valign(gtk4::Align::Center)
            .build();

        left_box.append(&text_box);
        left_box.append(&controls_box);
        left_box.append(&progress_box);

        let cover = gtk4::Image::builder().pixel_size(150).build();
        let cover_frame = gtk4::Frame::builder().child(&cover).build();
        cover_frame.add_css_class("card");

        let content = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(24)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();

        content.append(&cover_frame);
        content.append(&left_box);

        let frame = gtk4::Frame::builder().child(&content).build();
        frame.add_css_class("card");

        let revealer = gtk4::Revealer::builder()
            .transition_type(gtk4::RevealerTransitionType::SlideUp)
            .child(&frame)
            .reveal_child(false)
            .build();

        NowPlayingBar {
            revealer,
            track_title_label: title_label,
            subtitle_box,
            cover,
            play_button,
            progress_scale,
            progress_current_label,
            progress_total_label,
            on_open_album,
            on_open_artist,
            on_open_playlist,
        }
    }
}

pub fn update_now_playing(bar: &NowPlayingBar, tracklist: &Tracklist) {
    let Some(track) = tracklist.current_track() else {
        return;
    };

    let make_label = |text: &str| {
        let l = gtk4::Label::builder()
            .label(text)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        l.add_css_class("dim-label");
        l
    };

    let append_sep = || {
        let sep = make_label("·");
        bar.subtitle_box.append(&sep);
    };

    let image = match tracklist.list_type() {
        TracklistType::Album(a) => a.image.as_ref().or(track.image.as_ref()),
        _ => track.image.as_ref(),
    }
    .cloned();

    bar.track_title_label.set_text(&track.title);

    while let Some(child) = bar.subtitle_box.first_child() {
        bar.subtitle_box.remove(&child);
    }

    match tracklist.list_type() {
        TracklistType::Album(album) => {
            let label = make_label(&album.title);
            let on_open = bar.on_open_album.clone();
            let id = album.id.clone();

            let button = clickable_tile(&label.upcast(), move || {
                on_open(AlbumHeaderInfo { id: id.clone() })
            });
            bar.subtitle_box.append(&button);

            if let (Some(name), Some(artist_id)) = (&track.artist_name, track.artist_id) {
                append_sep();
                let label = make_label(name);
                let on_open = bar.on_open_artist.clone();

                let button = clickable_tile(&label.upcast(), move || {
                    on_open(ArtistHeaderInfo { id: artist_id })
                });
                bar.subtitle_box.append(&button);
            }
        }

        TracklistType::Playlist(playlist) => {
            let label = make_label(&playlist.title);
            let on_open = bar.on_open_playlist.clone();
            let id = playlist.id;

            let button = clickable_tile(&label.upcast(), move || {
                on_open(PlaylistHeaderInfo { id });
            });
            bar.subtitle_box.append(&button);

            if let (Some(name), Some(artist_id)) = (&track.artist_name, track.artist_id) {
                append_sep();
                let label = make_label(name);
                let on_open = bar.on_open_artist.clone();

                let button = clickable_tile(&label.upcast(), move || {
                    on_open(ArtistHeaderInfo { id: artist_id });
                });
                bar.subtitle_box.append(&button);
            }
        }

        TracklistType::TopTracks(top) => {
            let label = make_label(&top.artist_name);
            let id = top.id;
            let on_open = bar.on_open_artist.clone();
            let button = clickable_tile(&label.upcast(), move || {
                on_open(ArtistHeaderInfo { id });
            });
            bar.subtitle_box.append(&button);
        }

        TracklistType::Tracks => {
            if let (Some(title), Some(album_id)) = (&track.album_title, &track.album_id) {
                let label = make_label(title);
                let on_open = bar.on_open_album.clone();
                let id = album_id.clone();
                let button = clickable_tile(&label.upcast(), move || {
                    on_open(AlbumHeaderInfo { id: id.clone() });
                });
                bar.subtitle_box.append(&button);
            }

            if let (Some(name), Some(artist_id)) = (&track.artist_name, track.artist_id) {
                append_sep();
                let label = make_label(name);
                let on_open = bar.on_open_artist.clone();
                let button = clickable_tile(&label.upcast(), move || {
                    on_open(ArtistHeaderInfo { id: artist_id });
                });
                bar.subtitle_box.append(&button);
            }
        }
    }

    bar.progress_scale
        .set_range(0.0, (track.duration_seconds * 1000) as f64);
    bar.progress_total_label
        .set_text(&format_time(track.duration_seconds));

    set_image_from_url(image.as_deref(), &bar.cover);

    bar.revealer.set_reveal_child(true);
}

pub fn update_progress(bar: &NowPlayingBar, position: &Duration) {
    animate_scale_to(&bar.progress_scale, position.as_millis() as f64, 120);

    bar.progress_current_label
        .set_text(&format_time(position.as_secs() as u32));
}

pub fn update_now_playing_button_icon(status: &Status, button: &gtk4::Button) {
    match status {
        Status::Playing => button.set_icon_name("media-playback-pause-symbolic"),
        Status::Buffering => button.set_icon_name("content-loading-symbolic"),
        Status::Paused => button.set_icon_name("media-playback-start-symbolic"),
        Status::Stopped => button.set_icon_name("media-playback-stop-symbolic"),
    }
}

fn animate_scale_to(scale: &gtk4::Scale, target: f64, duration_ms: u32) {
    let adjustment = scale.adjustment();
    let start = adjustment.value();
    let delta = target - start;

    let start_time = std::time::Instant::now();

    scale.add_tick_callback(move |_, _| {
        let elapsed = start_time.elapsed().as_millis() as u32;
        let t = (elapsed as f64 / duration_ms as f64).min(1.0);

        let eased = 1.0 - (1.0 - t).powi(3);

        adjustment.set_value(start + delta * eased);

        if t >= 1.0 {
            gtk4::glib::ControlFlow::Break
        } else {
            gtk4::glib::ControlFlow::Continue
        }
    });
}
