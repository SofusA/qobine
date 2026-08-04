use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use num_traits::ToPrimitive;

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
    play_button: gtk::Button,
    volume_scale: gtk::Scale,

    progress_scale: gtk::Scale,
    progress_current_label: gtk::Label,
    progress_total_label: gtk::Label,

    connect_button: gtk::MenuButton,
    available_devices: Rc<RefCell<Vec<String>>>,
    active_device: Rc<RefCell<String>>,

    on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
    on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
    on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,

    setting_volume_from_backend: Rc<Cell<bool>>,
}

impl NowPlayingBar {
    pub fn new(
        controls: Controls,
        set_connect_active_device: UnboundedSender<String>,
        on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
        on_open_artist: Rc<dyn Fn(ArtistHeaderInfo)>,
        on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
        volume: f32,
    ) -> Self {
        let setting_volume_from_backend = Rc::new(Cell::new(false));
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

        let track_info_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        track_info_spacer.set_vexpand(true);

        let track_info_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Fill)
            .build();

        track_info_box.append(&title_label);
        track_info_box.append(&track_info_spacer);
        track_info_box.append(&subtitle_box);

        let connect_button = gtk::MenuButton::builder()
            .icon_name("audio-speakers-symbolic")
            .tooltip_text("Connect")
            .css_classes(vec!["flat"])
            .visible(false)
            .valign(gtk::Align::Center)
            .build();

        let connect_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(vec!["boxed-list"])
            .build();

        let connect_group = adw::PreferencesGroup::builder()
            .title("Select output device")
            .build();

        connect_group.add(&connect_list);

        let connect_content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .width_request(300)
            .build();

        connect_content.append(&connect_group);

        let connect_popover = gtk::Popover::builder()
            .child(&connect_content)
            .has_arrow(true)
            .build();

        connect_button.set_popover(Some(&connect_popover));

        connect_popover.connect_show({
            let connect_list = connect_list;
            let available_devices = available_devices.clone();
            let active_device = active_device.clone();
            let sender = set_connect_active_device;
            let popover = connect_popover.clone();

            move |_| {
                while let Some(child) = connect_list.first_child() {
                    connect_list.remove(&child);
                }

                let devices = available_devices.borrow().clone();
                let active = active_device.borrow().clone();

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
                    let row_active_device = active_device.clone();
                    let row_sender = sender.clone();
                    let row_popover = popover.clone();

                    row.connect_activated(move |_| {
                        row_active_device.borrow_mut().clone_from(&row_device);
                        let _ = row_sender.send(row_device.clone());

                        row_popover.popdown();
                    });

                    connect_list.append(&row);
                }
            }
        });

        let volume_button = gtk::MenuButton::builder()
            .icon_name("audio-volume-high-symbolic")
            .tooltip_text("Volume")
            .css_classes(vec!["flat"])
            .valign(gtk::Align::Center)
            .build();

        let volume_scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Vertical)
            .draw_value(false)
            .inverted(true)
            .height_request(120)
            .vexpand(true)
            .build();

        volume_scale.set_range(0.0, 1.0);
        volume_scale.set_value(f64::from(volume));

        volume_scale.connect_value_changed({
            let volume_button = volume_button.clone();
            let controls = controls.clone();
            let updating = setting_volume_from_backend.clone();

            move |scale| {
                let value = scale.value().to_f32().unwrap_or(1.0);

                if !updating.get() {
                    controls.set_volume(value);
                }

                let icon = if value <= 0.0 {
                    "audio-volume-muted-symbolic"
                } else if value < 0.33 {
                    "audio-volume-low-symbolic"
                } else if value < 0.66 {
                    "audio-volume-medium-symbolic"
                } else {
                    "audio-volume-high-symbolic"
                };

                volume_button.set_icon_name(icon);
            }
        });

        let volume_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();

        volume_box.append(&volume_scale);

        let volume_popover = gtk::Popover::builder()
            .child(&volume_box)
            .has_arrow(true)
            .build();

        volume_button.set_popover(Some(&volume_popover));

        let controls_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
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

        controls_box.append(&volume_button);
        controls_box.append(&prev_button);
        controls_box.append(&play_button);
        controls_box.append(&next_button);
        controls_box.append(&connect_button);

        let progress_current_label = gtk::Label::builder()
            .label("0:00")
            .css_classes(vec!["dim-label"])
            .width_chars(6)
            .xalign(1.0)
            .valign(gtk::Align::Center)
            .build();

        let progress_total_label = gtk::Label::builder()
            .label("0:00")
            .css_classes(vec!["dim-label"])
            .width_chars(6)
            .xalign(0.0)
            .valign(gtk::Align::Center)
            .build();

        let progress_scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .width_request(300)
            .draw_value(false)
            .focusable(false)
            .valign(gtk::Align::Center)
            .build();

        let controls_seek = controls;
        progress_scale.connect_change_value(move |_, _, value| {
            if let Some(value) = value.to_u64() {
                controls_seek.seek(Duration::from_millis(value));
            }

            glib::Propagation::Stop
        });

        let cover = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .can_shrink(true)
            .hexpand(false)
            .vexpand(false)
            .build();

        let clamp = adw::Clamp::builder().child(&cover).maximum_size(70).build();

        let cover_frame = gtk::Frame::builder()
            .child(&clamp)
            .valign(gtk::Align::Center)
            .build();

        let progress_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(2)
            .build();

        progress_box.append(&progress_current_label);
        progress_box.append(&progress_scale);
        progress_box.append(&progress_total_label);

        let playback_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        playback_spacer.set_vexpand(true);

        let playback_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Fill)
            .build();

        playback_box.append(&controls_box);
        playback_box.append(&playback_spacer);
        playback_box.append(&progress_box);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .hexpand(true)
            .build();

        let left_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .valign(gtk::Align::Fill)
            .build();

        left_box.append(&cover_frame);
        left_box.append(&track_info_box);

        content.append(&left_box);
        content.append(&playback_box);

        let separator = gtk::Separator::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build();

        let bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(vec!["view"])
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

        Self {
            revealer,
            track_title_label: title_label,
            subtitle_box,
            cover,
            play_button,
            progress_scale,
            progress_current_label,
            progress_total_label,
            connect_button,
            available_devices,
            active_device,
            on_open_album,
            on_open_artist,
            on_open_playlist,
            volume_scale,
            setting_volume_from_backend,
        }
    }

    pub fn set_volume(&self, volume: f32) {
        let volume = f64::from(volume.clamp(0.0, 1.0));

        if (self.volume_scale.value() - volume).abs() < 0.001 {
            return;
        }

        self.setting_volume_from_backend.set(true);
        self.volume_scale.set_value(volume);
        self.setting_volume_from_backend.set(false);
    }

    pub fn set_output_devices(&self, available_devices: Vec<String>, active_device: String) {
        let has_devices = !available_devices.is_empty();

        *self.available_devices.borrow_mut() = available_devices;
        *self.active_device.borrow_mut() = active_device;

        self.connect_button.set_visible(has_devices);
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
                    on_open(AlbumHeaderInfo { id: id.clone() });
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
            .set_range(0.0, f64::from(track.duration_seconds.saturating_mul(1000)));
        self.progress_total_label
            .set_text(&format_time(track.duration_seconds));

        set_picture_from_url(image.as_deref(), &self.cover);

        self.revealer.set_reveal_child(true);
    }

    pub fn update_progress(&self, position: &Duration) {
        if let Some(position_ms) = position.as_millis().to_f64() {
            animate_scale_to(&self.progress_scale, position_ms, 120);
        }

        if let Some(position_s) = position.as_millis().to_u32() {
            self.progress_current_label
                .set_text(&format_time(position_s));
        }
    }

    pub fn update_now_playing_button_icon(&self, status: Status) {
        match status {
            Status::Playing => self
                .play_button
                .set_icon_name("media-playback-pause-symbolic"),
            Status::Buffering => self.play_button.set_icon_name("content-loading-symbolic"),
            Status::Paused => self
                .play_button
                .set_icon_name("media-playback-start-symbolic"),
        }
    }
}

fn animate_scale_to(scale: &gtk::Scale, target: f64, duration_ms: u32) {
    let adjustment = scale.adjustment();
    let start = adjustment.value();
    let delta = target - start;

    let start_time = std::time::Instant::now();

    scale.add_tick_callback(move |_, _| {
        if let Some(elapsed) = start_time.elapsed().as_millis().to_u32() {
            let t = (f64::from(elapsed) / f64::from(duration_ms)).min(1.0);

            let eased = 1.0 - (1.0 - t).powi(3);

            adjustment.set_value(delta.mul_add(eased, start));

            if t >= 1.0 {
                gtk::glib::ControlFlow::Break
            } else {
                gtk::glib::ControlFlow::Continue
            }
        } else {
            gtk::glib::ControlFlow::Break
        }
    });
}
