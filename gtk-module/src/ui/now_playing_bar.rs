use std::{cell::RefCell, rc::Rc, time::Duration};

use gtk4 as gtk;
use libadwaita as adw;

use adw::prelude::*;
use controls_module::{
    Status,
    controls::Controls,
    tracklist::{Tracklist, TracklistType},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::ui::{
    album_detail_page::AlbumHeaderInfo, artist_detail_page::ArtistHeaderInfo, clickable_tile,
    format_time, playlist_detail_page::PlaylistHeaderInfo, set_picture_from_url,
};

#[derive(Clone)]
pub struct NowPlayingBar {
    pub revealer: gtk::Revealer,
    track_title_label: gtk::Label,
    subtitle_box: gtk::Box,
    cover: gtk::Picture,
    pub play_button: gtk::Button,

    progress_scale: gtk::Scale,
    progress_current_label: gtk::Label,
    progress_total_label: gtk::Label,

    output_button: gtk::Button,
    available_devices: Rc<RefCell<Vec<String>>>,
    active_device: Rc<RefCell<String>>,

    on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
    on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
    on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
}

impl NowPlayingBar {
    pub fn new(
        controls: Controls,
        set_connect_active_device: UnboundedSender<String>,
        on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
        on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
        on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
    ) -> Self {
        let available_devices = Rc::new(RefCell::new(Vec::<String>::new()));
        let active_device = Rc::new(RefCell::new(String::new()));

        let title_label = gtk::Label::builder()
            .halign(gtk::Align::Fill)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .wrap(false)
            .xalign(0.5)
            .css_classes(vec!["title-3"])
            .max_width_chars(30)
            .build();

        let subtitle_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .halign(gtk::Align::Fill)
            .build();

        let track_info_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Center)
            .spacing(2)
            .build();

        track_info_box.append(&title_label);
        track_info_box.append(&subtitle_box);

        let connect_button = gtk::Button::builder()
            .icon_name("audio-speakers-symbolic")
            .tooltip_text("Connect")
            .css_classes(vec!["flat"])
            .visible(false)
            .valign(gtk::Align::Center)
            .build();

        let dialog_available_devices = available_devices.clone();
        let dialog_active_device = active_device.clone();
        let dialog_sender = set_connect_active_device.clone();

        connect_button.connect_clicked(move |button| {
            let devices = dialog_available_devices.borrow().clone();
            let active = dialog_active_device.borrow().clone();

            if devices.is_empty() {
                return;
            }

            let dialog = adw::Dialog::builder()
                .title("Connect")
                .content_width(360)
                .content_height(420)
                .build();

            let toolbar_view = adw::ToolbarView::new();
            let header_bar = adw::HeaderBar::new();

            toolbar_view.add_top_bar(&header_bar);

            let list = gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .css_classes(vec!["boxed-list"])
                .build();

            for device in devices {
                let row = adw::ActionRow::builder()
                    .title(&device)
                    .activatable(true)
                    .build();

                if device == active {
                    let check = gtk::Image::from_icon_name("object-select-symbolic");
                    row.add_suffix(&check);
                }

                let row_device = device.clone();
                let row_sender = dialog_sender.clone();
                let row_active_device = dialog_active_device.clone();
                let row_dialog = dialog.clone();

                row.connect_activated(move |_| {
                    *row_active_device.borrow_mut() = row_device.clone();
                    let _ = row_sender.send(row_device.clone());
                    row_dialog.close();
                });

                list.append(&row);
            }

            let group = adw::PreferencesGroup::builder()
                .title("Select output device")
                .build();
            group.add(&list);

            let content = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .margin_start(12)
                .margin_end(12)
                .margin_top(12)
                .margin_bottom(12)
                .build();

            content.append(&group);

            toolbar_view.set_content(Some(&content));
            dialog.set_child(Some(&toolbar_view));
            dialog.present(Some(button));
        });

        let controls_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();

        let controls_prev = controls.clone();
        let prev_button = gtk::Button::builder()
            .icon_name("media-seek-backward-symbolic")
            .css_classes(vec!["flat"])
            .build();
        prev_button.connect_clicked(move |_| controls_prev.previous());

        let controls_play_pause = controls.clone();
        let play_button = gtk::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .css_classes(vec!["flat"])
            .build();
        play_button.connect_clicked(move |_| controls_play_pause.play_pause());

        let controls_next = controls.clone();
        let next_button = gtk::Button::builder()
            .icon_name("media-seek-forward-symbolic")
            .css_classes(vec!["flat"])
            .build();
        next_button.connect_clicked(move |_| controls_next.next());

        controls_box.append(&prev_button);
        controls_box.append(&play_button);
        controls_box.append(&next_button);
        controls_box.append(&connect_button);

        let progress_current_label = gtk::Label::builder()
            .label("0:00")
            .css_classes(vec!["dim-label"])
            .width_chars(6)
            .xalign(0.0)
            .valign(gtk::Align::Center)
            .build();

        let progress_total_label = gtk::Label::builder()
            .label("0:00")
            .css_classes(vec!["dim-label"])
            .width_chars(6)
            .xalign(1.0)
            .valign(gtk::Align::Center)
            .build();

        let progress_scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .draw_value(false)
            .focusable(false)
            .valign(gtk::Align::Center)
            .build();

        let controls_seek = controls.clone();
        progress_scale.connect_change_value(move |_, _, value| {
            controls_seek.seek(Duration::from_millis(value as u64));
            glib::Propagation::Stop
        });

        let progress_time_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .halign(gtk::Align::Fill)
            .spacing(6)
            .build();

        progress_current_label.set_hexpand(true);
        progress_current_label.set_halign(gtk::Align::Start);
        progress_current_label.set_xalign(0.0);

        progress_total_label.set_hexpand(true);
        progress_total_label.set_halign(gtk::Align::End);
        progress_total_label.set_xalign(1.0);

        progress_time_box.append(&progress_current_label);
        progress_time_box.append(&progress_total_label);

        let progress_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Center)
            .spacing(2)
            .build();

        progress_box.append(&progress_scale);
        progress_box.append(&progress_time_box);

        let progress_clamp = adw::Clamp::builder()
            .child(&progress_box)
            .maximum_size(420)
            .tightening_threshold(320)
            .width_request(320)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Center)
            .hexpand(false)
            .build();

        let cover = gtk::Picture::new();
        cover.set_content_fit(gtk::ContentFit::Contain);
        cover.set_can_shrink(true);
        cover.set_hexpand(false);
        cover.set_vexpand(false);

        let clamp = adw::Clamp::builder().child(&cover).maximum_size(75).build();

        let cover_frame = gtk::Frame::builder()
            .child(&clamp)
            .valign(gtk::Align::Center)
            .build();

        let left_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();

        left_box.append(&cover_frame);
        left_box.append(&track_info_box);

        let right_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();

        right_box.append(&progress_clamp);

        let content = gtk::CenterBox::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();

        content.set_start_widget(Some(&left_box));
        content.set_center_widget(Some(&controls_box));
        content.set_end_widget(Some(&right_box));

        let separator = gtk::Separator::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build();

        let bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(vec!["view"])
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::End)
            .hexpand(true)
            .build();

        bar.append(&separator);
        bar.append(&content);

        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .child(&bar)
            .reveal_child(false)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::End)
            .hexpand(true)
            .vexpand(false)
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
            output_button: connect_button,
            available_devices,
            active_device,
            on_open_album,
            on_open_artist,
            on_open_playlist,
        }
    }

    pub fn set_output_devices(&self, available_devices: Vec<String>, active_device: String) {
        let has_devices = !available_devices.is_empty();

        *self.available_devices.borrow_mut() = available_devices;
        *self.active_device.borrow_mut() = active_device.clone();

        self.output_button.set_visible(has_devices);
    }

    pub fn update(&self, tracklist: &Tracklist) {
        let Some(track) = tracklist.current_track() else {
            return;
        };

        let make_label = |text: &str| {
            gtk::Label::builder()
                .label(text)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(vec!["dim-label"])
                .build()
        };

        let append_sep = || {
            let sep = make_label("·");
            self.subtitle_box.append(&sep);
        };

        let image = match tracklist.list_type() {
            TracklistType::Album(a) => a.image.as_ref().or(track.image.as_ref()),
            _ => track.image.as_ref(),
        }
        .cloned();

        self.track_title_label.set_text(&track.title);

        while let Some(child) = self.subtitle_box.first_child() {
            self.subtitle_box.remove(&child);
        }

        match tracklist.list_type() {
            TracklistType::Album(album) => {
                let label = make_label(&album.title);
                let on_open = self.on_open_album.clone();
                let id = album.id.clone();

                let button = clickable_tile(&label.upcast(), move || {
                    on_open(AlbumHeaderInfo { id: id.clone() })
                });
                self.subtitle_box.append(&button);

                if let (Some(name), Some(artist_id)) = (&track.artist_name, track.artist_id) {
                    append_sep();
                    let label = make_label(name);
                    let on_open = self.on_open_artist.clone();

                    let button = clickable_tile(&label.upcast(), move || {
                        on_open(ArtistHeaderInfo { id: artist_id })
                    });
                    self.subtitle_box.append(&button);
                }
            }

            TracklistType::Playlist(playlist) => {
                let label = make_label(&playlist.title);
                let on_open = self.on_open_playlist.clone();
                let id = playlist.id;

                let button = clickable_tile(&label.upcast(), move || {
                    on_open(PlaylistHeaderInfo { id });
                });
                self.subtitle_box.append(&button);

                if let (Some(name), Some(artist_id)) = (&track.artist_name, track.artist_id) {
                    append_sep();
                    let label = make_label(name);
                    let on_open = self.on_open_artist.clone();

                    let button = clickable_tile(&label.upcast(), move || {
                        on_open(ArtistHeaderInfo { id: artist_id });
                    });
                    self.subtitle_box.append(&button);
                }
            }

            TracklistType::TopTracks(top) => {
                let label = make_label(&top.artist_name);
                let id = top.id;
                let on_open = self.on_open_artist.clone();
                let button = clickable_tile(&label.upcast(), move || {
                    on_open(ArtistHeaderInfo { id });
                });
                self.subtitle_box.append(&button);
            }

            TracklistType::Tracks => {
                if let (Some(title), Some(album_id)) = (&track.album_title, &track.album_id) {
                    let label = make_label(title);
                    let on_open = self.on_open_album.clone();
                    let id = album_id.clone();
                    let button = clickable_tile(&label.upcast(), move || {
                        on_open(AlbumHeaderInfo { id: id.clone() });
                    });
                    self.subtitle_box.append(&button);
                }

                if let (Some(name), Some(artist_id)) = (&track.artist_name, track.artist_id) {
                    append_sep();
                    let label = make_label(name);
                    let on_open = self.on_open_artist.clone();
                    let button = clickable_tile(&label.upcast(), move || {
                        on_open(ArtistHeaderInfo { id: artist_id });
                    });
                    self.subtitle_box.append(&button);
                }
            }
        }

        self.progress_scale
            .set_range(0.0, (track.duration_seconds * 1000) as f64);
        self.progress_total_label
            .set_text(&format_time(track.duration_seconds));

        set_picture_from_url(image.as_deref(), &self.cover);

        self.revealer.set_reveal_child(true);
    }
}

pub fn update_progress(bar: &NowPlayingBar, position: &Duration) {
    animate_scale_to(&bar.progress_scale, position.as_millis() as f64, 120);

    bar.progress_current_label
        .set_text(&format_time(position.as_secs() as u32));
}

pub fn update_now_playing_button_icon(status: &Status, button: &gtk::Button) {
    match status {
        Status::Playing => button.set_icon_name("media-playback-pause-symbolic"),
        Status::Buffering => button.set_icon_name("content-loading-symbolic"),
        Status::Paused => button.set_icon_name("media-playback-start-symbolic"),
    }
}

fn animate_scale_to(scale: &gtk::Scale, target: f64, duration_ms: u32) {
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
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}
