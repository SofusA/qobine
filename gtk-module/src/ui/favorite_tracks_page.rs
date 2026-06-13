use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use controls_module::controls::Controls;
use controls_module::models::{PlaylistSimple, Track};
use player_module::client::Client;

use crate::UiEventSender;
use crate::ui::build_track_row;

#[derive(Clone)]
pub struct FavoriteTracksPage {
    root: gtk::Box,
    listbox: gtk::ListBox,
    empty_label: gtk::Label,
    controls: Controls,
    client: Arc<Client>,

    play_button: gtk::Button,
    shuffle_button: gtk::Button,
    tracks: Rc<RefCell<Vec<Track>>>,

    ui_event_sender: UiEventSender,
}

impl FavoriteTracksPage {
    pub fn new(controls: Controls, client: Arc<Client>, ui_event_sender: UiEventSender) -> Self {
        let tracks = Rc::new(RefCell::new(Vec::<Track>::new()));

        let title = gtk::Label::builder()
            .label("Favorite Tracks")
            .halign(gtk::Align::Start)
            .css_classes(vec!["title-1".to_string()])
            .build();

        let play_button = gtk::Button::builder()
            .label("Play")
            .icon_name("media-playback-start-symbolic")
            .sensitive(false)
            .css_classes(vec!["suggested-action".to_string(), "pill".to_string()])
            .build();

        let shuffle_button = gtk::Button::builder()
            .label("Shuffle")
            .icon_name("media-playlist-shuffle-symbolic")
            .sensitive(false)
            .css_classes(vec!["pill".to_string()])
            .build();

        play_button.connect_clicked({
            let controls = controls.clone();
            let tracks = tracks.clone();

            move |_| {
                let tracks = tracks.borrow().iter().map(|x| x.id).collect();
                controls.play_tracks(tracks, false);
            }
        });

        shuffle_button.connect_clicked({
            let controls = controls.clone();
            let tracks = tracks.clone();

            move |_| {
                let tracks = tracks.borrow().iter().map(|x| x.id).collect();
                controls.play_tracks(tracks, true);
            }
        });

        let actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Start)
            .build();

        actions.append(&play_button);
        actions.append(&shuffle_button);

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();

        header.append(&title);
        header.append(&actions);

        let empty_label = gtk::Label::builder()
            .label("No favorite tracks yet")
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .visible(false)
            .css_classes(vec!["dim-label".to_string()])
            .build();

        let listbox = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(vec!["boxed-list".to_string()])
            .show_separators(true)
            .activate_on_single_click(true)
            .vexpand(true)
            .valign(gtk::Align::Start)
            .build();

        listbox.connect_row_activated({
            let controls = controls.clone();
            let tracks = tracks.clone();

            move |_lb, row| {
                let idx = row.index();

                if idx >= 0 {
                    let tracks = tracks.borrow();

                    if let Some(track) = tracks.get(idx as usize) {
                        controls.play_track(track.id);
                    }
                }
            }
        });

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&listbox)
            .build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .vexpand(true)
            .build();

        root.append(&header);
        root.append(&scrolled);
        root.append(&empty_label);

        Self {
            root,
            listbox,
            empty_label,
            controls,
            client,
            tracks,
            play_button,
            shuffle_button,
            ui_event_sender,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn load(&self, tracks: Vec<Track>, owned_playlists: &Vec<PlaylistSimple>) {
        self.clear();

        let is_empty = tracks.is_empty();

        self.listbox.set_visible(!is_empty);
        self.empty_label.set_visible(is_empty);
        self.play_button.set_sensitive(!is_empty);
        self.shuffle_button.set_sensitive(!is_empty);

        let favorite_track_ids = tracks.iter().map(|x| x.id).collect();
        *self.tracks.borrow_mut() = tracks;

        for track in self.tracks.borrow().iter() {
            let row = build_track_row(
                track,
                true,
                true,
                false,
                self.controls.clone(),
                self.client.clone(),
                self.ui_event_sender.clone(),
                &favorite_track_ids,
                owned_playlists,
            );

            self.listbox.append(&row);
        }
    }

    fn clear(&self) {
        while let Some(child) = self.listbox.first_child() {
            self.listbox.remove(&child);
        }
    }
}
