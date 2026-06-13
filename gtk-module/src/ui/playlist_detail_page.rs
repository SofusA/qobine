use std::{cell::RefCell, rc::Rc, sync::Arc};

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::{gdk, gio, prelude::*};
use libadwaita as adw;

use controls_module::{
    TracklistReceiver, controls::Controls, models::Track, tracklist::PlayingEntity,
};
use player_module::client::Client;

use crate::ui::set_picture_from_url;
use crate::{
    UiEvent, UiEventSender,
    ui::{
        DetailPage, DetailPageType, build_track_row,
        detail_page::{
            DetailType, build_detail_header, build_detail_scaffold, populate_playlist_menu,
        },
        format_time,
    },
};

#[derive(Clone, Debug)]
pub struct PlaylistHeaderInfo {
    pub id: u32,
}

pub struct PlaylistDetailPage {
    page: adw::NavigationPage,

    client: Arc<Client>,
    controls: Controls,
    tracklist_receiver: TracklistReceiver,
    playlist_id: u32,

    stack: gtk::Stack,

    cover: gtk::Picture,
    title: gtk::Label,
    meta: gtk::Label,
    owner: gtk::Label,
    playlist_menu: gio::Menu,
    delete_button: gtk::Button,
    favorite_button: gtk::Button,

    tracks_list: gtk::ListBox,

    current_selected_index: Rc<RefCell<Option<usize>>>,
    tracks: Rc<RefCell<Vec<Track>>>,

    loaded: RefCell<bool>,
    ui_event_sender: UiEventSender,
}

impl PlaylistDetailPage {
    pub fn new(
        playlist_id: u32,
        controls: Controls,
        client: Arc<Client>,
        tracklist_receiver: TracklistReceiver,
        ui_event_sender: UiEventSender,
    ) -> Self {
        let empty_title = gtk::Box::builder().hexpand(true).build();
        let nav_bar = adw::HeaderBar::builder().title_widget(&empty_title).build();

        let title = gtk::Label::builder()
            .wrap(true)
            .css_classes(vec!["title-1"])
            .build();

        let meta = gtk::Label::builder()
            .wrap(true)
            .css_classes(vec!["dim-label"])
            .build();

        let owner = gtk::Label::builder().wrap(true).build();

        let play_button = gtk::Button::builder()
            .label("Play")
            .icon_name("media-playback-start-symbolic")
            .css_classes(vec!["suggested-action", "pill"])
            .build();

        play_button.connect_clicked({
            let controls = controls.clone();
            move |_| {
                controls.play_playlist(playlist_id, 0, false);
            }
        });

        let shuffle_button = gtk::Button::builder()
            .label("Shuffle")
            .icon_name("media-playlist-shuffle-symbolic")
            .css_classes(vec!["pill"])
            .build();

        shuffle_button.connect_clicked({
            let controls = controls.clone();
            move |_| {
                controls.play_playlist(playlist_id, 0, true);
            }
        });

        let delete_button = gtk::Button::builder()
            .label("Delete")
            .icon_name("user-trash-symbolic")
            .css_classes(vec!["destructive-action", "pill"])
            .visible(false)
            .build();

        delete_button.connect_clicked({
            let client = client.clone();
            let ui_event_sender = ui_event_sender.clone();

            move |button| {
                let client = client.clone();
                let button = button.clone();
                let ui_event_sender = ui_event_sender.clone();

                glib::MainContext::default().spawn_local(async move {
                    let dialog = adw::AlertDialog::builder()
                        .heading("Delete playlist?")
                        .body("This playlist will be permanently deleted.")
                        .build();

                    dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);

                    dialog.set_default_response(Some("cancel"));
                    dialog.set_close_response("cancel");
                    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

                    let response = dialog.choose_future(Some(&button)).await;

                    if response.as_str() != "delete" {
                        return;
                    }

                    button.set_sensitive(false);

                    match client.delete_playlist(playlist_id).await {
                        Ok(_) => {
                            if let Some(nav_view) = button
                                .ancestor(adw::NavigationView::static_type())
                                .and_then(|w| w.downcast::<adw::NavigationView>().ok())
                            {
                                nav_view.pop();
                            }
                            if let Err(error) = ui_event_sender.send(UiEvent::FavoritesChanged) {
                                tracing::error!("{error}")
                            };
                        }
                        Err(err) => {
                            tracing::error!("Failed to delete playlist {playlist_id}: {err}");
                            button.set_sensitive(true);
                        }
                    }
                });
            }
        });

        let header = build_detail_header(
            client.clone(),
            controls.clone(),
            ui_event_sender.clone(),
            vec![
                title.clone().upcast(),
                owner.clone().upcast(),
                meta.clone().upcast(),
            ],
            vec![
                play_button.clone().upcast(),
                shuffle_button.clone().upcast(),
                delete_button.clone().upcast(),
            ],
            DetailType::Playlist(playlist_id),
        );

        let scaffold = build_detail_scaffold(&header.header_section, {
            let controls = controls.clone();
            move |index| {
                controls.play_playlist(playlist_id, index, false);
            }
        });

        let cover = header.cover;
        let stack = scaffold.stack;
        let tracks_list = scaffold.tracks_list;
        let menu = header.playlist_menu;
        let favorite_button = header.favorite_button;

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&nav_bar);
        toolbar.set_content(Some(&stack));

        let page = adw::NavigationPage::builder()
            .title("Playlist")
            .child(&toolbar)
            .build();

        let s = Self {
            page,
            client,
            controls,
            tracklist_receiver,
            playlist_id,
            stack,
            cover,
            title,
            meta,
            playlist_menu: menu,
            tracks_list,
            loaded: RefCell::new(false),
            current_selected_index: Rc::new(RefCell::new(None)),
            ui_event_sender,
            delete_button,
            tracks: Default::default(),
            favorite_button,
            owner,
        };

        s.load_playlist();

        s
    }

    fn load_playlist(&self) {
        if *self.loaded.borrow() {
            return;
        }
        *self.loaded.borrow_mut() = true;

        let client = self.client.clone();
        let ui_event_sender = self.ui_event_sender.clone();
        let controls = self.controls.clone();
        let playlist_id = self.playlist_id;

        let stack = self.stack.clone();
        let cover = self.cover.clone();
        let title = self.title.clone();
        let meta = self.meta.clone();
        let owner = self.owner.clone();
        let tracks_list = self.tracks_list.clone();
        let tracklist_receiver = self.tracklist_receiver.clone();
        let current_playing_index = self.current_selected_index.clone();

        let stored_tracks = self.tracks.clone();

        let delete_button = self.delete_button.clone();
        let favorite_button = self.favorite_button.clone();

        stack.set_visible_child_name("loading");
        populate_playlist_menu(self.playlist_menu.clone(), client.clone());

        glib::MainContext::default().spawn_local(async move {
            match client.playlist(playlist_id).await {
                Ok(playlist) => {
                    title.set_label(&playlist.title);

                    let dur_str = format_time(playlist.duration_seconds);
                    meta.set_label(&dur_str.to_string());
                    owner.set_label(&format!("By {}", playlist.owner.name));

                    set_picture_from_url(playlist.image.as_deref(), &cover);

                    delete_button.set_visible(playlist.is_owned);
                    favorite_button.set_visible(!playlist.is_owned);

                    clear_listbox(&tracks_list);

                    let favorites = client.favorites().await.unwrap_or_default();
                    let favorite_tracks = favorites.tracks.into_iter().map(|x| x.id).collect();
                    let owned_playlists = favorites
                        .playlists
                        .into_iter()
                        .filter(|x| x.is_owned)
                        .map(|x| x.into())
                        .collect();

                    for track in playlist.tracks.iter() {
                        let row = build_track_row(
                            track,
                            true,
                            true,
                            false,
                            controls.clone(),
                            client.clone(),
                            ui_event_sender.clone(),
                            &favorite_tracks,
                            &owned_playlists,
                        );

                        if playlist.is_owned {
                            add_owned_playlist_track_controls(
                                playlist_id,
                                &row,
                                &tracks_list,
                                client.clone(),
                                stored_tracks.clone(),
                                current_playing_index.clone(),
                            );
                        }

                        tracks_list.append(&row);
                    }

                    let playing_entity = tracklist_receiver.borrow().current_playing_entity();
                    if let Some(playing_entity) = playing_entity {
                        update_current_playing(
                            &playing_entity,
                            playlist_id,
                            &current_playing_index,
                            &tracks_list,
                        );
                    }
                    stack.set_visible_child_name("content");
                    let mut tracks = stored_tracks.borrow_mut();
                    *tracks = playlist.tracks;
                }
                Err(err) => {
                    tracing::error!("Failed to load playlist {playlist_id}: {err}");

                    clear_listbox(&tracks_list);

                    let label = gtk::Label::builder()
                        .label("Failed to load playlist.")
                        .xalign(0.0)
                        .margin_top(12)
                        .margin_bottom(12)
                        .margin_start(12)
                        .margin_end(12)
                        .css_classes(vec!["dim-label"])
                        .build();

                    let row = adw::ActionRow::builder().child(&label).build();
                    tracks_list.append(&row);

                    stack.set_visible_child_name("content");
                }
            }
        });
    }
}

impl DetailPage for PlaylistDetailPage {
    fn page(&self) -> &adw::NavigationPage {
        &self.page
    }

    fn update_current_playing(&self, playing_entity: PlayingEntity) {
        update_current_playing(
            &playing_entity,
            self.playlist_id,
            &self.current_selected_index,
            &self.tracks_list,
        );
    }

    fn detail_type(&self) -> DetailPageType {
        DetailPageType::Playlist(self.playlist_id)
    }
}

fn update_current_playing(
    playing_entity: &PlayingEntity,
    playlist_id: u32,
    current_selected_index: &Rc<RefCell<Option<usize>>>,
    tracks_list: &gtk::ListBox,
) {
    let playing = match playing_entity {
        PlayingEntity::Playlist(p) => p,
        _ => return,
    };

    if playing.playlist_id != playlist_id {
        tracks_list.unselect_all();
        *current_selected_index.borrow_mut() = None;
        return;
    }

    let idx = playing.index;
    *current_selected_index.borrow_mut() = Some(idx);

    if let Some(row) = tracks_list.row_at_index(idx as i32) {
        tracks_list.select_row(Some(&row));
    } else {
        tracks_list.unselect_all();
    }
}

fn clear_listbox(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn add_owned_playlist_track_controls(
    playlist_id: u32,
    row: &impl IsA<gtk::ListBoxRow>,
    tracks_list: &gtk::ListBox,
    client: Arc<Client>,
    stored_tracks: Rc<RefCell<Vec<Track>>>,
    current_selected_index: Rc<RefCell<Option<usize>>>,
) {
    let listbox_row = row.upcast_ref::<gtk::ListBoxRow>();

    let remove_button = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Remove from playlist")
        .valign(gtk::Align::Center)
        .css_classes(vec!["flat"])
        .build();

    remove_button.connect_clicked({
        let row = listbox_row.clone();
        let tracks_list = tracks_list.clone();
        let client = client.clone();
        let stored_tracks = stored_tracks.clone();

        move |_| {
            let Some(track_playlist_id) = stored_tracks
                .borrow()
                .get(row.index() as usize)
                .and_then(|track| track.playlist_track_id)
            else {
                return;
            };

            let row = row.clone();
            let tracks_list = tracks_list.clone();
            let client = client.clone();
            let stored_tracks = stored_tracks.clone();
            let current_selected_index = current_selected_index.clone();

            glib::MainContext::default().spawn_local(async move {
                if let Err(error) = client
                    .playlist_delete_track(playlist_id, &[track_playlist_id])
                    .await
                {
                    tracing::error!("{error}");
                    return;
                }

                let removed_index = row.index();

                if removed_index < 0 {
                    return;
                }

                let removed_index = removed_index as usize;

                let mut tracks = stored_tracks.borrow_mut();

                if removed_index < tracks.len() {
                    tracks.remove(removed_index);
                }

                tracks_list.remove(&row);

                let mut current_selected = current_selected_index.borrow_mut();

                match *current_selected {
                    Some(current_index) if current_index == removed_index => {
                        // Deleted the playing track, so nothing in this playlist should look playing.
                        *current_selected = None;
                        glib::idle_add_local_once(move || {
                            tracks_list.unselect_all();
                        });
                    }
                    Some(current_index) if current_index > removed_index => {
                        // A row before the playing track was removed, so its index shifts down.
                        let new_index = current_index - 1;
                        *current_selected = Some(new_index);

                        if let Some(row) = tracks_list.row_at_index(new_index as i32) {
                            glib::idle_add_local_once(move || {
                                tracks_list.select_row(Some(&row));
                            });
                        } else {
                            glib::idle_add_local_once(move || {
                                tracks_list.unselect_all();
                            });
                        }
                    }
                    Some(current_index) => {
                        // Playing track still exists at same index.
                        if let Some(row) = tracks_list.row_at_index(current_index as i32) {
                            glib::idle_add_local_once(move || {
                                tracks_list.select_row(Some(&row));
                            });
                        } else {
                            glib::idle_add_local_once(move || {
                                tracks_list.unselect_all();
                            });
                        }
                    }
                    None => {
                        // Nothing is playing, so make sure nothing appears selected.
                        glib::idle_add_local_once(move || {
                            tracks_list.unselect_all();
                        });
                    }
                }
            });
        }
    });

    if let Some(child) = listbox_row.child()
        && let Ok(hbox) = child.downcast::<gtk::Box>()
    {
        hbox.append(&remove_button);
    }

    let drag_source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();

    drag_source.connect_prepare({
        let row = listbox_row.clone();

        move |_, _, _| {
            let from_index = row.index();
            Some(gdk::ContentProvider::for_value(
                &(from_index as u32).to_value(),
            ))
        }
    });

    listbox_row.add_controller(drag_source);

    let drop_target = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);

    drop_target.connect_drop({
        let target_row = listbox_row.clone();
        let tracks_list = tracks_list.clone();
        let client = client.clone();
        let stored_tracks = stored_tracks.clone();

        move |_, value, _, _| {
            let Ok(from_index) = value.get::<u32>() else {
                return false;
            };

            let from_index = from_index as usize;
            let to_index = target_row.index();

            if to_index < 0 {
                return false;
            }

            let to_index = to_index as usize;

            if from_index == to_index {
                return true;
            }

            let Some(track_playlist_id) = stored_tracks
                .borrow()
                .get(from_index)
                .and_then(|track| track.playlist_track_id)
            else {
                return false;
            };

            let Some(dragged_row) = tracks_list.row_at_index(from_index as i32) else {
                return false;
            };

            let client = client.clone();
            let tracks_list = tracks_list.clone();
            let stored_tracks = stored_tracks.clone();

            glib::MainContext::default().spawn_local(async move {
                println!("{to_index}");
                if let Err(error) = client
                    .update_playlist_track_position(to_index, playlist_id, track_playlist_id)
                    .await
                {
                    tracing::error!("{error}");
                    return;
                }

                let mut tracks = stored_tracks.borrow_mut();

                if from_index >= tracks.len() || to_index >= tracks.len() {
                    return;
                }

                let track = tracks.remove(from_index);
                tracks.insert(to_index, track);

                tracks_list.remove(&dragged_row);
                tracks_list.insert(&dragged_row, to_index as i32);
            });

            true
        }
    });

    listbox_row.add_controller(drop_target);
}
