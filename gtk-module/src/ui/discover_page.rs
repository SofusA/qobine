use std::{cell::RefCell, fmt, future::Future, rc::Rc, sync::Arc};

use futures_util::{
    future::{Either, select},
    pin_mut,
};
use gtk::{gio, glib, prelude::*};
use gtk4 as gtk;

use controls_module::models::{AlbumSimple, Genre, PlaylistSimple, PlaylistTag};
use player_module::client::{GenrePlaylistSlug, StreamClient};

use crate::ui::{
    album_detail_page::AlbumHeaderInfo, album_scroller, playlist_detail_page::PlaylistHeaderInfo,
    playlist_scroller,
};

const LOAD_TIMEOUT_SECONDS: u32 = 10;

#[derive(Debug)]
enum LoadError {
    Timeout,
    Request(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => write!(f, "Request timed out"),
            Self::Request(err) => write!(f, "{err}"),
        }
    }
}

async fn with_timeout<T, E, F>(future: F) -> Result<T, LoadError>
where
    F: Future<Output = Result<T, E>>,
    E: fmt::Display,
{
    let timeout = glib::timeout_future_seconds(LOAD_TIMEOUT_SECONDS);

    pin_mut!(future);
    pin_mut!(timeout);

    match select(future, timeout).await {
        Either::Left((result, _)) => result.map_err(|err| LoadError::Request(err.to_string())),
        Either::Right(((), _)) => Err(LoadError::Timeout),
    }
}

#[derive(Clone)]
pub struct DiscoverPage {
    root: gtk::Box,
    scroller: gtk::ScrolledWindow,
    client: Arc<StreamClient>,

    selected: Rc<RefCell<GenrePlaylistTag>>,
    playlist_section: Rc<RefCell<Option<gtk::Box>>>,

    on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
    on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
}

impl DiscoverPage {
    pub fn new(
        client: Arc<StreamClient>,
        on_open_album: Rc<dyn Fn(AlbumHeaderInfo)>,
        on_open_playlist: Rc<dyn Fn(PlaylistHeaderInfo)>,
    ) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();

        let scroller = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&root)
            .build();

        let selected = GenrePlaylistTag {
            genre_id: None,
            playlist_tag: None,
        };

        Self {
            root,
            scroller,
            client,
            selected: Rc::new(RefCell::new(selected)),
            playlist_section: Rc::new(RefCell::new(None)),
            on_open_album,
            on_open_playlist,
        }
    }

    pub const fn widget(&self) -> &gtk::ScrolledWindow {
        &self.scroller
    }

    pub fn load(&self) {
        self.clear();
        self.render_loading();

        let page = self.clone();
        let client = self.client.clone();
        let selected = self.selected.borrow().clone();

        glib::MainContext::default().spawn_local(async move {
            let discover_data =
                match with_timeout(client.discover_page(selected.clone().genre_id)).await {
                    Ok(data) => data,
                    Err(err) => {
                        tracing::error!("{err}");
                        page.render_error("Failed to load Discover");
                        return;
                    }
                };

            let genres = match with_timeout(client.genres()).await {
                Ok(genres) => genres,
                Err(err) => {
                    tracing::error!("{err}");
                    page.render_error("Failed to load genres");
                    return;
                }
            };

            let playlists = match with_timeout(client.genre_playlists(GenrePlaylistSlug {
                genre_id: selected.genre_id,
                playlist_slug: selected.playlist_tag.map(|x| x.slug),
            }))
            .await
            {
                Ok(playlists) => playlists,
                Err(err) => {
                    tracing::error!("{err}");
                    page.render_error("Failed to load playlists");
                    return;
                }
            };

            page.clear();

            let header = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(12)
                .build();

            let title = gtk::Label::builder()
                .label("Discover")
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["title-1"])
                .build();

            header.append(&title);

            let genre_button = page.genre_selector_button(&genres);
            header.append(&genre_button);

            page.root.append(&header);

            add_album_section(
                &page.root,
                "New releases",
                &discover_data.new_releases,
                &page.on_open_album,
            );

            page.add_playlist_section(&discover_data.playlists_tags, &playlists);

            add_album_section(
                &page.root,
                "Essential Discography",
                &discover_data.ideal_discography,
                &page.on_open_album,
            );

            add_album_section(
                &page.root,
                "Qobuzissime",
                &discover_data.qobuzissims,
                &page.on_open_album,
            );

            add_album_section(
                &page.root,
                "Album of the week",
                &discover_data.album_of_the_week,
                &page.on_open_album,
            );

            add_album_section(
                &page.root,
                "Press Accolades",
                &discover_data.press_awards,
                &page.on_open_album,
            );

            add_album_section(
                &page.root,
                "Most streamed",
                &discover_data.most_streamed,
                &page.on_open_album,
            );
        });
    }

    fn clear(&self) {
        while let Some(child) = self.root.first_child() {
            self.root.remove(&child);
        }

        *self.playlist_section.borrow_mut() = None;
    }

    fn render_loading(&self) {
        let spinner = gtk::Spinner::builder()
            .spinning(true)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .margin_top(48)
            .margin_bottom(48)
            .build();

        self.root.append(&spinner);
    }

    fn render_error(&self, message: &str) {
        self.clear();

        let box_ = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .margin_top(48)
            .margin_bottom(48)
            .build();

        let label = gtk::Label::builder()
            .label(message)
            .css_classes(["title-3"])
            .build();

        let retry = gtk::Button::builder().label("Retry").build();

        let page = self.clone();
        retry.connect_clicked(move |_| {
            page.load();
        });

        box_.append(&label);
        box_.append(&retry);

        self.root.append(&box_);
    }

    fn genre_selector_button(&self, genres: &[Genre]) -> gtk::MenuButton {
        let menu = gio::Menu::new();

        let all_item = gio::MenuItem::new(Some("All genres"), None);
        all_item
            .set_action_and_target_value(Some("discover.select-genre"), Some(&"all".to_variant()));
        menu.append_item(&all_item);

        for genre in genres {
            let id = genre.id.to_string();
            let name = genre.name.clone();

            let item = gio::MenuItem::new(Some(&name), None);
            item.set_action_and_target_value(Some("discover.select-genre"), Some(&id.to_variant()));

            menu.append_item(&item);
        }

        let action = gio::SimpleAction::new("select-genre", Some(&String::static_variant_type()));

        let page = self.clone();

        action.connect_activate(move |_action, target| {
            let Some(target) = target.and_then(glib::Variant::get::<String>) else {
                return;
            };

            {
                let mut selected = page.selected.borrow_mut();

                selected.genre_id = if target == "all" {
                    None
                } else {
                    target.parse::<u32>().ok()
                };

                selected.playlist_tag = None;
            }

            page.load();
        });

        let action_group = gio::SimpleActionGroup::new();
        action_group.add_action(&action);

        let selected = self.selected.borrow();

        let label = selected
            .genre_id
            .and_then(|selected_id| {
                genres
                    .iter()
                    .find(|genre| genre.id == selected_id)
                    .map(|genre| genre.name.clone())
            })
            .unwrap_or_else(|| "All genres".to_string());

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.insert_action_group("discover", Some(&action_group));

        gtk::MenuButton::builder()
            .label(&label)
            .popover(&popover)
            .build()
    }

    fn playlist_tag_selector_button(&self, playlist_tags: &[PlaylistTag]) -> gtk::MenuButton {
        let menu = gio::Menu::new();

        let all_item = gio::MenuItem::new(Some("All playlists"), None);
        all_item.set_action_and_target_value(
            Some("discover.select-playlist-tag"),
            Some(&"all".to_variant()),
        );
        menu.append_item(&all_item);

        for tag in playlist_tags {
            let item = gio::MenuItem::new(Some(&tag.name), None);
            item.set_action_and_target_value(
                Some("discover.select-playlist-tag"),
                Some(&tag.slug.to_variant()),
            );

            menu.append_item(&item);
        }

        let action =
            gio::SimpleAction::new("select-playlist-tag", Some(&String::static_variant_type()));

        let page = self.clone();
        let tags = playlist_tags.to_vec();

        action.connect_activate(move |_action, target| {
            let Some(target) = target.and_then(glib::Variant::get::<String>) else {
                return;
            };

            {
                let mut selected = page.selected.borrow_mut();

                if target == "all" {
                    if selected.playlist_tag.is_none() {
                        return;
                    }

                    selected.playlist_tag = None;
                } else {
                    let current = selected.playlist_tag.as_ref().map(|tag| tag.slug.clone());

                    if current.as_deref() == Some(target.as_str()) {
                        return;
                    }

                    selected.playlist_tag = tags.iter().find(|tag| tag.slug == target).cloned();
                }
            }

            page.reload_playlist_section(tags.clone());
        });

        let action_group = gio::SimpleActionGroup::new();
        action_group.add_action(&action);

        let selected = self.selected.borrow();

        let label = selected
            .playlist_tag
            .as_ref()
            .map_or_else(|| "All playlists".to_string(), |tag| tag.name.clone());

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.insert_action_group("discover", Some(&action_group));

        gtk::MenuButton::builder()
            .label(&label)
            .popover(&popover)
            .build()
    }

    fn add_playlist_section(&self, playlist_tags: &[PlaylistTag], playlists: &[PlaylistSimple]) {
        let section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();

        self.render_playlist_section_content(&section, playlist_tags, playlists);

        *self.playlist_section.borrow_mut() = Some(section.clone());

        self.root.append(&section);
    }

    fn render_playlist_section_content(
        &self,
        section: &gtk::Box,
        playlist_tags: &[PlaylistTag],
        playlists: &[PlaylistSimple],
    ) {
        while let Some(child) = section.first_child() {
            section.remove(&child);
        }

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();

        let title = gtk::Label::builder()
            .label("Playlists")
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["title-2"])
            .build();

        header.append(&title);

        let playlist_tag_button = self.playlist_tag_selector_button(playlist_tags);
        header.append(&playlist_tag_button);

        section.append(&header);

        let playlist_row = playlist_scroller(playlists, &self.on_open_playlist);
        section.append(&playlist_row);
    }

    fn render_playlist_section_loading(&self, section: &gtk::Box, playlist_tags: &[PlaylistTag]) {
        while let Some(child) = section.first_child() {
            section.remove(&child);
        }

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();

        let title = gtk::Label::builder()
            .label("Playlists")
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["title-2"])
            .build();

        header.append(&title);

        let playlist_tag_button = self.playlist_tag_selector_button(playlist_tags);
        header.append(&playlist_tag_button);

        section.append(&header);

        let spinner = gtk::Spinner::builder()
            .spinning(true)
            .halign(gtk::Align::Center)
            .margin_top(24)
            .margin_bottom(24)
            .build();

        section.append(&spinner);
    }

    fn reload_playlist_section(&self, playlist_tags: Vec<PlaylistTag>) {
        let Some(section) = self.playlist_section.borrow().clone() else {
            return;
        };

        self.render_playlist_section_loading(&section, &playlist_tags);

        let page = self.clone();
        let client = self.client.clone();
        let selected = self.selected.borrow().clone();

        glib::MainContext::default().spawn_local(async move {
            let playlists = match with_timeout(client.genre_playlists(GenrePlaylistSlug {
                genre_id: selected.genre_id,
                playlist_slug: selected.playlist_tag.map(|x| x.slug),
            }))
            .await
            {
                Ok(playlists) => playlists,
                Err(err) => {
                    tracing::error!("{err}");
                    return;
                }
            };

            page.render_playlist_section_content(&section, &playlist_tags, &playlists);
        });
    }
}

fn add_album_section(
    root: &gtk::Box,
    title: &str,
    albums: &[AlbumSimple],
    on_open_album: &Rc<dyn Fn(AlbumHeaderInfo)>,
) {
    if albums.is_empty() {
        return;
    }

    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();

    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["title-2"])
        .build();

    section.append(&label);
    section.append(&album_scroller(albums, on_open_album));

    root.append(&section);
}

#[derive(Debug, Clone)]
struct GenrePlaylistTag {
    pub genre_id: Option<u32>,
    pub playlist_tag: Option<PlaylistTag>,
}
