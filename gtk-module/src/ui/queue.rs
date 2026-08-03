use adw::prelude::*;
use gtk::gdk;
use gtk4 as gtk;
use libadwaita as adw;
use player_module::client::StreamClient;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use controls_module::controls::Controls;
use controls_module::models::{PlaylistSimple, Track, TrackStatus};
use controls_module::tracklist::{QueueItem, Tracklist};

use crate::UiEventSender;
use crate::ui::build_track_row;

#[derive(Clone)]
pub struct QueuePage {
    root: gtk::Box,
    controls: Controls,
    client: Arc<StreamClient>,
    ui_event_sender: UiEventSender,
    favorite_tracks: Rc<RefCell<HashSet<u32>>>,
    owned_playlists: Rc<RefCell<Vec<PlaylistSimple>>>,

    listbox: gtk::ListBox,
    queue_items: Rc<RefCell<Vec<QueueItem>>>,
    rows_by_queue_id: Rc<RefCell<HashMap<u64, gtk::ListBoxRow>>>,
}

impl QueuePage {
    pub fn new(controls: Controls, client: Arc<StreamClient>, ui_event_sender: UiEventSender) -> Self {
        let listbox = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(vec!["boxed-list"])
            .show_separators(true)
            .activate_on_single_click(true)
            .vexpand(false)
            .valign(gtk::Align::Start)
            .build();

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true)
            .vexpand(false)
            .valign(gtk::Align::Start)
            .child(&listbox)
            .build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .vexpand(true)
            .valign(gtk::Align::Start)
            .build();

        root.append(&scrolled);

        let queue_items: Rc<RefCell<Vec<QueueItem>>> = Rc::new(RefCell::new(Vec::new()));
        let rows_by_queue_id: Rc<RefCell<HashMap<u64, gtk::ListBoxRow>>> =
            Rc::new(RefCell::new(HashMap::new()));

        listbox.connect_row_activated({
            let controls = controls.clone();

            move |_lb, row| {
                let idx = row.index();

                if idx >= 0
                    && let Ok(idx) = usize::try_from(idx)
                {
                    controls.skip_to_position(idx, true);
                }
            }
        });

        Self {
            root,
            controls,
            client,
            listbox,
            queue_items,
            rows_by_queue_id,
            ui_event_sender,
            favorite_tracks: Rc::default(),
            owned_playlists: Rc::default(),
        }
    }

    pub const fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn load(&self, tracklist: &Tracklist) {
        let new_queue_items: Vec<QueueItem> = tracklist
            .queue()
            .iter()
            .map(|x| QueueItem {
                track: x.track.clone(),
                queue_id: x.queue_id,
                index: x.index,
            })
            .collect();

        sync_queue_list(
            &self.listbox,
            &self.queue_items,
            &self.rows_by_queue_id,
            &self.controls,
            &self.client,
            &self.ui_event_sender,
            new_queue_items,
            &self.favorite_tracks,
            &self.owned_playlists,
        );
    }

    pub fn favorite_tracks_changed(&self, tracks: HashSet<u32>) {
        let mut favorite_tracks = self.favorite_tracks.borrow_mut();
        *favorite_tracks = tracks;
    }

    pub fn owned_playlists_changed(&self, playlists: Vec<PlaylistSimple>) {
        {
            let mut owned_playlists = self.owned_playlists.borrow_mut();
            *owned_playlists = playlists;
        }

        sync_existing_queue_list(
            &self.listbox,
            &self.queue_items,
            &self.rows_by_queue_id,
            &self.controls,
            &self.client,
            &self.ui_event_sender,
            &self.favorite_tracks,
            &self.owned_playlists,
        );
    }
}

fn sync_queue_list(
    listbox: &gtk::ListBox,
    queue_items: &Rc<RefCell<Vec<QueueItem>>>,
    rows_by_queue_id: &Rc<RefCell<HashMap<u64, gtk::ListBoxRow>>>,
    controls: &Controls,
    client: &Arc<StreamClient>,
    ui_event_sender: &UiEventSender,
    new_queue_items: Vec<QueueItem>,
    favorite_tracks: &Rc<RefCell<HashSet<u32>>>,
    owned_playlists: &Rc<RefCell<Vec<PlaylistSimple>>>,
) {
    let old_track_by_queue_id: HashMap<u64, u32> = queue_items
        .borrow()
        .iter()
        .map(|x| (x.queue_id, x.track.id))
        .collect();

    let new_queue_ids: HashSet<u64> = new_queue_items.iter().map(|x| x.queue_id).collect();

    let existing_queue_ids: Vec<u64> = rows_by_queue_id.borrow().keys().copied().collect();

    for queue_id in existing_queue_ids {
        if !new_queue_ids.contains(&queue_id)
            && let Some(row) = rows_by_queue_id.borrow_mut().remove(&queue_id)
        {
            listbox.remove(&row);
        }
    }

    for item in &new_queue_items {
        let must_rebuild = old_track_by_queue_id
            .get(&item.queue_id)
            .is_some_and(|old_track_id| *old_track_id != item.track.id);

        if must_rebuild && let Some(row) = rows_by_queue_id.borrow_mut().remove(&item.queue_id) {
            listbox.remove(&row);
        }

        if rows_by_queue_id.borrow().get(&item.queue_id).is_none() {
            let row = build_queue_row(
                item,
                controls.clone(),
                client.clone(),
                ui_event_sender.clone(),
                favorite_tracks,
                owned_playlists,
            );

            row.set_activatable(true);
            row.set_selectable(true);
            row.set_widget_name(&format!("queue-row-{}", item.queue_id));

            install_row_behaviors(
                listbox,
                &row,
                queue_items.clone(),
                rows_by_queue_id.clone(),
                controls.clone(),
                client.clone(),
                ui_event_sender.clone(),
                favorite_tracks.clone(),
                owned_playlists.clone(),
            );

            rows_by_queue_id.borrow_mut().insert(item.queue_id, row);
        }
    }

    refresh_rows(listbox, &new_queue_items, rows_by_queue_id);

    *queue_items.borrow_mut() = new_queue_items;
}

fn sync_existing_queue_list(
    listbox: &gtk::ListBox,
    queue_items: &Rc<RefCell<Vec<QueueItem>>>,
    rows_by_queue_id: &Rc<RefCell<HashMap<u64, gtk::ListBoxRow>>>,
    controls: &Controls,
    client: &Arc<StreamClient>,
    ui_event_sender: &UiEventSender,
    favorite_tracks: &Rc<RefCell<HashSet<u32>>>,
    owned_playlists: &Rc<RefCell<Vec<PlaylistSimple>>>,
) {
    for (_, row) in rows_by_queue_id.borrow_mut().drain() {
        listbox.remove(&row);
    }

    let items = queue_items.borrow().clone();

    for item in &items {
        let row = build_queue_row(
            item,
            controls.clone(),
            client.clone(),
            ui_event_sender.clone(),
            favorite_tracks,
            owned_playlists,
        );

        row.set_activatable(true);
        row.set_selectable(true);
        row.set_widget_name(&format!("queue-row-{}", item.queue_id));

        install_row_behaviors(
            listbox,
            &row,
            queue_items.clone(),
            rows_by_queue_id.clone(),
            controls.clone(),
            client.clone(),
            ui_event_sender.clone(),
            favorite_tracks.clone(),
            owned_playlists.clone(),
        );

        rows_by_queue_id.borrow_mut().insert(item.queue_id, row);
    }

    refresh_rows(listbox, &items, rows_by_queue_id);
}

fn refresh_rows(
    listbox: &gtk::ListBox,
    items: &[QueueItem],
    rows_by_queue_id: &RefCell<HashMap<u64, gtk::ListBoxRow>>,
) {
    let mut playing_row = None;

    for item in items {
        let Some(row) = rows_by_queue_id.borrow().get(&item.queue_id).cloned() else {
            continue;
        };

        apply_track_status_to_row(&row, &item.track);

        if is_playing(&item.track) {
            playing_row = Some(row);
        }
    }

    for (wanted_index, item) in items.iter().enumerate() {
        let Some(row) = rows_by_queue_id.borrow().get(&item.queue_id).cloned() else {
            continue;
        };

        let Ok(wanted_index) = i32::try_from(wanted_index) else {
            break;
        };

        if row.parent().is_none() {
            listbox.insert(&row, wanted_index);
        } else if row.index() != wanted_index {
            listbox.remove(&row);
            listbox.insert(&row, wanted_index);
        }
    }

    if let Some(row) = playing_row {
        listbox.select_row(Some(&row));
    } else {
        listbox.unselect_all();
    }
}

fn build_queue_row(
    item: &QueueItem,
    controls: Controls,
    client: Arc<StreamClient>,
    ui_event_sender: UiEventSender,
    favorite_tracks: &RefCell<HashSet<u32>>,
    owned_playlists: &RefCell<Vec<PlaylistSimple>>,
) -> gtk::ListBoxRow {
    let row = build_track_row(
        &item.track,
        true,
        true,
        false,
        controls,
        client,
        ui_event_sender,
        &favorite_tracks.borrow(),
        &owned_playlists.borrow(),
    );

    if let Some(child) = row.child()
        && let Ok(hbox) = child.downcast::<gtk::Box>()
    {
        let remove_btn = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Remove from queue")
            .valign(gtk::Align::Center)
            .css_classes(vec!["flat"])
            .focusable(false)
            .build();

        hbox.append(&remove_btn);
        remove_btn.set_widget_name("queue-remove-button");
    }

    row
}

fn apply_track_status_to_row(row: &gtk::ListBoxRow, track: &Track) {
    if is_playing(track) {
        row.set_opacity(1.0);
    } else if is_played(track) {
        row.set_opacity(0.45);
    } else {
        row.set_opacity(1.0);
    }
}

const fn is_playing(track: &Track) -> bool {
    matches!(track.status, TrackStatus::Playing)
}

const fn is_played(track: &Track) -> bool {
    matches!(track.status, TrackStatus::Played)
}

fn install_row_behaviors(
    listbox: &gtk::ListBox,
    row: &gtk::ListBoxRow,
    queue_items: Rc<RefCell<Vec<QueueItem>>>,
    rows_by_queue_id: Rc<RefCell<HashMap<u64, gtk::ListBoxRow>>>,
    controls: Controls,
    client: Arc<StreamClient>,
    ui_event_sender: UiEventSender,
    favorite_tracks: Rc<RefCell<HashSet<u32>>>,
    owned_playlists: Rc<RefCell<Vec<PlaylistSimple>>>,
) {
    if let Some(child) = row.child()
        && let Ok(hbox) = child.downcast::<gtk::Box>()
        && let Some(last) = hbox.last_child()
        && let Ok(remove_btn) = last.downcast::<gtk::Button>()
    {
        let listbox_weak = listbox.downgrade();
        let row_weak = row.downgrade();
        let queue_items_clone = queue_items.clone();
        let rows_by_queue_id_clone = rows_by_queue_id.clone();
        let favorite_tracks = favorite_tracks.clone();
        let owned_playlists = owned_playlists.clone();

        remove_btn.connect_clicked({
            let controls = controls.clone();
            let client = client.clone();
            let ui_event_sender = ui_event_sender.clone();

            move |_| {
                let Some(listbox) = listbox_weak.upgrade() else {
                    return;
                };

                let Some(row) = row_weak.upgrade() else {
                    return;
                };

                let idx = row.index();

                if idx < 0 {
                    return;
                }

                let Ok(idx) = usize::try_from(idx) else {
                    return;
                };

                let new_queue_items = {
                    let vec = queue_items_clone.borrow_mut();

                    if idx >= vec.len() {
                        return;
                    }

                    controls.remove_index_from_queue(idx);

                    vec.clone()
                };

                sync_queue_list(
                    &listbox,
                    &queue_items_clone,
                    &rows_by_queue_id_clone,
                    &controls,
                    &client,
                    &ui_event_sender,
                    new_queue_items,
                    &favorite_tracks,
                    &owned_playlists,
                );
            }
        });
    }

    let drag_source = gtk::DragSource::new();
    drag_source.set_actions(gdk::DragAction::MOVE);

    let row_weak = row.downgrade();

    drag_source.connect_prepare(move |_source, _x, _y| {
        let row = row_weak.upgrade()?;
        let from_index = row.index();

        if from_index < 0 {
            return None;
        }

        let value = from_index.to_value();
        Some(gdk::ContentProvider::for_value(&value))
    });

    row.add_controller(drag_source);

    let drop_target = gtk::DropTarget::new(i32::static_type(), gdk::DragAction::MOVE);

    drop_target.connect_drop({
        let listbox_weak = listbox.downgrade();
        let row_weak = row.downgrade();
        let queue_items_clone = queue_items;
        let rows_by_queue_id_clone = rows_by_queue_id;

        move |_target, value, _x, _y| {
            let Some(listbox) = listbox_weak.upgrade() else {
                return false;
            };

            let Some(row) = row_weak.upgrade() else {
                return false;
            };

            let Ok(from_index) = value.get::<i32>() else {
                return false;
            };

            let Ok(from_index) = usize::try_from(from_index) else {
                return false;
            };

            let Ok(mut to_index) = usize::try_from(row.index()) else {
                return false;
            };

            let new_queue_items = {
                let mut vec = queue_items_clone.borrow_mut();

                if from_index >= vec.len() {
                    return false;
                }

                if from_index == to_index || from_index.checked_add(1) == Some(to_index) {
                    return true;
                }

                let mut order: Vec<usize> = (0..vec.len()).collect();
                let item = vec.remove(from_index);

                if from_index < to_index {
                    to_index = to_index.saturating_sub(1);
                }

                to_index = to_index.min(vec.len());

                vec.insert(to_index, item);

                move_index(&mut order, from_index, to_index);
                controls.reorder_queue(order);

                vec.clone()
            };

            sync_queue_list(
                &listbox,
                &queue_items_clone,
                &rows_by_queue_id_clone,
                &controls,
                &client,
                &ui_event_sender,
                new_queue_items,
                &favorite_tracks,
                &owned_playlists,
            );

            true
        }
    });

    row.add_controller(drop_target);
}

fn move_index(vec: &mut Vec<usize>, from: usize, mut to: usize) {
    if from >= vec.len() {
        return;
    }

    let value = vec.remove(from);
    to = to.min(vec.len());
    vec.insert(to, value);
}
