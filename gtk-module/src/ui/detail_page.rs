use std::{cell::Cell, rc::Rc, sync::Arc};

use gtk4 as gtk;
use libadwaita as adw;

use adw::prelude::*;
use controls_module::{controls::Controls, models::Track};
use gtk::gio;
use player_module::{AppResult, client::Client};

use crate::{UiEvent, UiEventSender};

pub struct DetailHeaderParts {
    pub header_section: gtk::Box,
    pub cover: gtk::Picture,
    pub playlist_menu: gio::Menu,
    pub favorite_button: gtk::Button,
}

pub fn build_detail_header(
    client: Arc<Client>,
    controls: Controls,
    ui_event_sender: UiEventSender,
    text_rows: Vec<gtk::Widget>,
    buttons: Vec<gtk::Button>,
    favorite_button_type: DetailType,
) -> DetailHeaderParts {
    let cover = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .build();

    let cover_frame = gtk::Frame::builder()
        .valign(gtk::Align::End)
        .child(&cover)
        .build();

    let header_text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::End)
        .spacing(12)
        .hexpand(true)
        .build();

    for row in text_rows {
        header_text.append(&row);
    }

    let button_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .halign(gtk4::Align::Center)
        .spacing(12)
        .build();

    for button in buttons {
        button_box.append(&button);
    }
    let favorite_button = new_favorite_button(
        client.clone(),
        favorite_button_type.clone(),
        ui_event_sender,
    );

    let menu = gio::Menu::new();
    menu.append(Some("Add to queue"), Some("tracks.add-to-queue"));
    menu.append(Some("Play next"), Some("tracks.play-next"));

    let playlist_section = gio::Menu::new();
    menu.append_submenu(Some("Add to playlist"), &playlist_section);

    let action_group = gio::SimpleActionGroup::new();

    let add_to_queue_action = gio::SimpleAction::new("add-to-queue", None);

    add_to_queue_action.connect_activate({
        let controls = controls.clone();
        let detail_type = favorite_button_type.clone();
        let client = client.clone();

        move |_, _| {
            glib::MainContext::default().spawn_local({
                let controls = controls.clone();
                let detail_type = detail_type.clone();
                let client = client.clone();

                async move {
                    match fetch_tracks(&client, detail_type).await {
                        Ok(tracks) => {
                            controls.add_tracks_to_queue(tracks);
                        }
                        Err(err) => {
                            tracing::error!("Failed to fetch tracks: {err}");
                        }
                    }
                }
            });
        }
    });

    action_group.add_action(&add_to_queue_action);

    let play_next_action = gio::SimpleAction::new("play-next", None);

    play_next_action.connect_activate({
        let controls = controls.clone();
        let detail_type = favorite_button_type.clone();
        let client = client.clone();

        move |_, _| {
            glib::MainContext::default().spawn_local({
                let controls = controls.clone();
                let detail_type = detail_type.clone();
                let client = client.clone();

                async move {
                    match fetch_tracks(&client, detail_type).await {
                        Ok(tracks) => {
                            controls.play_tracks_next(tracks);
                        }
                        Err(err) => {
                            tracing::error!("Failed to fetch tracks: {err}");
                        }
                    }
                }
            });
        }
    });

    action_group.add_action(&play_next_action);

    let add_to_playlist_action =
        gio::SimpleAction::new("add-to-playlist", Some(&u32::static_variant_type()));

    add_to_playlist_action.connect_activate({
        let client = client.clone();
        let detail_type = favorite_button_type.clone();

        move |_, parameter| {
            let Some(playlist_id) = parameter.and_then(|p| p.get::<u32>()) else {
                tracing::error!("Missing playlist id");
                return;
            };

            glib::MainContext::default().spawn_local({
                let client = client.clone();
                let detail_type = detail_type.clone();

                async move {
                    match fetch_tracks(&client, detail_type).await {
                        Ok(track_ids) => {
                            if let Err(err) = client
                                .playlist_add_track(
                                    playlist_id,
                                    &track_ids.iter().map(|x| x.id).collect::<Vec<_>>(),
                                )
                                .await
                            {
                                tracing::error!("Failed to add tracks to playlist: {err}");
                            }
                        }
                        Err(err) => {
                            tracing::error!("Failed to fetch tracks: {err}");
                        }
                    }
                }
            });
        }
    });

    action_group.add_action(&add_to_playlist_action);

    let popover_menu = gtk::PopoverMenu::from_model(Some(&menu));
    let actions_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .css_classes(vec!["flat", "circular"])
        .popover(&popover_menu)
        .build();
    actions_button.insert_action_group("tracks", Some(&action_group));

    button_box.append(&favorite_button);
    button_box.append(&actions_button);
    header_text.append(&button_box);

    let header_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(18)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    header_section.append(&cover_frame);
    header_section.append(&header_text);

    DetailHeaderParts {
        header_section,
        cover,
        playlist_menu: playlist_section,
        favorite_button,
    }
}

pub fn populate_playlist_menu(playlist_menu: gio::Menu, client: Arc<Client>) {
    glib::MainContext::default().spawn_local(async move {
        match client.favorites().await {
            Ok(favorites) => {
                let playlists: Vec<_> = favorites
                    .playlists
                    .into_iter()
                    .filter(|x| x.is_owned)
                    .collect();

                if playlists.is_empty() {
                    playlist_menu.append(Some("No playlists"), None);
                    return;
                }

                for playlist in playlists {
                    let item = gio::MenuItem::new(Some(&playlist.title), None);

                    item.set_action_and_target_value(
                        Some("tracks.add-to-playlist"),
                        Some(&playlist.id.to_variant()),
                    );

                    playlist_menu.append_item(&item);
                }
            }
            Err(err) => {
                tracing::warn!("Failed to fetch playlists: {err}");
                playlist_menu.append(Some("Failed to load playlists"), None);
            }
        }
    });
}

pub struct DetailScaffoldParts {
    pub stack: gtk::Stack,
    pub content: gtk::Box,
    pub tracks_list: gtk::ListBox,
}

pub fn build_detail_scaffold(
    header_section: &impl IsA<gtk::Widget>,
    on_track_activated: impl Fn(usize) + 'static,
) -> DetailScaffoldParts {
    let spinner = gtk::Spinner::new();
    spinner.start();

    let spinner_box = gtk::Box::builder()
        .vexpand(true)
        .hexpand(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    spinner_box.append(&spinner);

    let tracks_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .activate_on_single_click(true)
        .css_classes(vec!["boxed-list"])
        .margin_start(18)
        .margin_end(18)
        .margin_bottom(18)
        .build();

    tracks_list.connect_row_activated(move |_, row| {
        let index = row.index();

        if index >= 0 {
            on_track_activated(index as usize);
        }
    });

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .hexpand(true)
        .vexpand(false)
        .valign(gtk::Align::Start)
        .build();

    content.append(header_section);
    content.append(&tracks_list);

    let clamp = adw::Clamp::builder()
        .maximum_size(900)
        .tightening_threshold(700)
        .child(&content)
        .hexpand(true)
        .vexpand(true)
        .valign(gtk::Align::Start)
        .build();

    let scroller = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&clamp)
        .build();

    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();

    stack.add_named(&spinner_box, Some("loading"));
    stack.add_named(&scroller, Some("content"));
    stack.set_visible_child_name("loading");

    DetailScaffoldParts {
        stack,
        content,
        tracks_list,
    }
}

async fn fetch_tracks(client: &Client, favorite_button_type: DetailType) -> AppResult<Vec<Track>> {
    let tracks = match favorite_button_type {
        DetailType::Album(id) => client.album(&id).await?.tracks,
        DetailType::Artist(id) => client.artist_page(id).await?.top_tracks,
        DetailType::Playlist(id) => client.playlist(id).await?.tracks,
    };

    Ok(tracks)
}

#[derive(Clone)]
pub enum DetailType {
    Album(String),
    Artist(u32),
    Playlist(u32),
}

fn new_favorite_button(
    client: Arc<Client>,
    button_type: DetailType,
    ui_event_sender: UiEventSender,
) -> gtk4::Button {
    let is_favorite = Rc::new(Cell::new(false));
    let button_type = Rc::new(button_type);

    let favorites_button = gtk4::Button::builder()
        .label("Favorite")
        .icon_name("non-starred-symbolic")
        .css_classes(vec!["pill"])
        .build();

    glib::MainContext::default().spawn_local(glib::clone!(
        #[weak]
        favorites_button,
        #[strong]
        client,
        #[strong]
        button_type,
        #[strong]
        is_favorite,
        async move {
            if let Ok(favorites) = client.favorites().await {
                let fav = match &*button_type {
                    DetailType::Album(album_id) => {
                        favorites.albums.iter().any(|x| x.id == *album_id)
                    }
                    DetailType::Artist(artist_id) => {
                        favorites.artists.iter().any(|x| x.id == *artist_id)
                    }
                    DetailType::Playlist(playlist_id) => {
                        favorites.playlists.iter().any(|x| x.id == *playlist_id)
                    }
                };

                is_favorite.set(fav);
                favorites_button.set_icon_name(if fav {
                    "starred-symbolic"
                } else {
                    "non-starred-symbolic"
                });
            }
        }
    ));

    favorites_button.connect_clicked(glib::clone!(
        #[weak]
        favorites_button,
        #[strong]
        client,
        #[strong]
        button_type,
        #[strong]
        ui_event_sender,
        #[strong]
        is_favorite,
        move |_| {
            let client = client.clone();
            let button_type = button_type.clone();

            glib::MainContext::default().spawn_local(glib::clone!(
                #[weak]
                favorites_button,
                #[strong]
                ui_event_sender,
                #[strong]
                is_favorite,
                async move {
                    let next = !is_favorite.get();

                    let res = match &*button_type {
                        DetailType::Album(album_id) => {
                            if next {
                                client.add_favorite_album(album_id).await
                            } else {
                                client.remove_favorite_album(album_id).await
                            }
                        }
                        DetailType::Artist(artist_id) => {
                            if next {
                                client.add_favorite_artist(*artist_id).await
                            } else {
                                client.remove_favorite_artist(*artist_id).await
                            }
                        }
                        DetailType::Playlist(playlist_id) => {
                            if next {
                                client.add_favorite_playlist(*playlist_id).await
                            } else {
                                client.remove_favorite_playlist(*playlist_id).await
                            }
                        }
                    };

                    if res.is_ok() {
                        is_favorite.set(next);
                        favorites_button.set_icon_name(if next {
                            "starred-symbolic"
                        } else {
                            "non-starred-symbolic"
                        });
                        let _ = ui_event_sender.send(UiEvent::FavoritesChanged);
                    }
                }
            ));
        }
    ));

    favorites_button
}
