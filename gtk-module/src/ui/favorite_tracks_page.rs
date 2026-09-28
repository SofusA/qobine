use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use controls_module::controls::Controls;
use controls_module::models::{PlaylistSimple, Track};
use player_module::client::StreamClient;

use crate::UiEventSender;
use crate::ui::build_track_row;
use crate::ui::grid_page::FavoriteSort;

#[derive(Clone)]
pub struct FavoriteTracksPage {
    root: gtk::Box,
    listbox: gtk::ListBox,
    empty_label: gtk::Label,
    controls: Controls,
    client: Arc<StreamClient>,
    play_button: gtk::Button,
    shuffle_button: gtk::Button,
    original_tracks: Rc<RefCell<Vec<Track>>>,
    tracks: Rc<RefCell<Vec<Track>>>,
    owned_playlists: Rc<RefCell<Vec<PlaylistSimple>>>,
    query: Rc<RefCell<String>>,
    sort: Rc<RefCell<FavoriteSort>>,
    ui_event_sender: UiEventSender,
}

impl FavoriteTracksPage {
    pub fn new(
        controls: Controls,
        client: Arc<StreamClient>,
        ui_event_sender: UiEventSender,
    ) -> Self {
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
                controls.play_tracks(&tracks.borrow(), false, 0);
            }
        });

        shuffle_button.connect_clicked({
            let controls = controls.clone();
            let tracks = tracks.clone();

            move |_| {
                controls.play_tracks(&tracks.borrow(), true, 0);
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

                if idx >= 0
                    && let Ok(idx) = usize::try_from(idx)
                {
                    controls.play_tracks(&tracks.borrow(), false, idx);
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

        let original_tracks = Rc::new(RefCell::new(Vec::new()));
        let tracks = Rc::new(RefCell::new(Vec::new()));
        let owned_playlists = Rc::new(RefCell::new(Vec::new()));
        let query = Rc::new(RefCell::new(String::new()));
        let sort = Rc::new(RefCell::new(FavoriteSort::DateAdded));

        Self {
            root,
            listbox,
            empty_label,
            controls,
            client,
            play_button,
            shuffle_button,
            original_tracks,
            tracks,
            owned_playlists,
            query,
            sort,
            ui_event_sender,
        }
    }

    pub const fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn load(&self, tracks: Vec<Track>, owned_playlists: &[PlaylistSimple]) {
        *self.original_tracks.borrow_mut() = tracks;
        *self.owned_playlists.borrow_mut() = owned_playlists.to_vec();
        *self.query.borrow_mut() = String::new();

        self.rebuild();
    }

    pub fn set_sort(&self, sort: FavoriteSort) {
        if *self.sort.borrow() == sort {
            return;
        }

        *self.sort.borrow_mut() = sort;
        self.rebuild();
    }

    pub fn filter(&self, query: &str) {
        *self.query.borrow_mut() = query.trim().to_lowercase();
        self.rebuild();
    }

    fn rebuild(&self) {
        self.clear();

        let query = self.query.borrow();

        let mut tracks: Vec<Track> = self
            .original_tracks
            .borrow()
            .iter()
            .filter(|track| {
                query.is_empty()
                    || track.title.to_lowercase().contains(query.as_str())
                    || track
                        .artist_name
                        .as_ref()
                        .is_some_and(|artist| artist.to_lowercase().contains(query.as_str()))
            })
            .cloned()
            .collect();

        if *self.sort.borrow() == FavoriteSort::Alphabetical {
            tracks.sort_by(|a, b| {
                a.title
                    .to_lowercase()
                    .cmp(&b.title.to_lowercase())
                    .then_with(|| {
                        a.artist_name
                            .as_deref()
                            .unwrap_or_default()
                            .to_lowercase()
                            .cmp(&b.artist_name.as_deref().unwrap_or_default().to_lowercase())
                    })
            });
        }

        let is_empty = tracks.is_empty();
        let favorite_track_ids = self
            .original_tracks
            .borrow()
            .iter()
            .map(|track| track.id)
            .collect();

        *self.tracks.borrow_mut() = tracks;

        self.listbox.set_visible(!is_empty);
        self.empty_label.set_visible(is_empty);
        self.play_button.set_sensitive(!is_empty);
        self.shuffle_button.set_sensitive(!is_empty);

        let owned_playlists = self.owned_playlists.borrow();

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
                &owned_playlists,
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
