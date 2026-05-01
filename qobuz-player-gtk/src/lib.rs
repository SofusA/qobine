use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use adw::{Application, prelude::*};
use libadwaita::{self as adw, ApplicationWindow};
use qobuz_player_controls::{
    AppResult, ExitSender, PositionReceiver, Status, StatusReceiver, TracklistReceiver,
    client::{Client, exchange_oauth_code},
    controls::Controls,
    database::{Credentials, Database},
    error::Error,
    tracklist::Tracklist,
};
use tokio::sync::mpsc;
use webkit6::{WebView, prelude::*};

use crate::{
    callbacks::{CallbackHandles, build_callbacks},
    ui::{
        DetailPage,
        library_page::LibraryPage,
        now_playing_bar::{
            NowPlayingBar, update_now_playing, update_now_playing_button_icon, update_progress,
        },
        search_page::SearchPage,
    },
};

mod callbacks;
mod ui;

fn oauth_login_window(
    app: &Application,
    oauth_url: &str,
    sender: tokio::sync::mpsc::UnboundedSender<String>,
) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Sign in to Qobuz")
        .default_width(480)
        .default_height(720)
        .build();

    let webview = WebView::new();
    webview.load_uri(oauth_url);

    let window_weak = window.downgrade();

    webview.connect_load_changed(move |webview, event| {
        if event == webkit6::LoadEvent::Committed
            && let Some(uri) = webview.uri()
            && uri.starts_with("http://localhost/")
            && let Some(code) = extract_code_from_uri(&uri)
        {
            sender.send(code).unwrap();

            if let Some(window) = window_weak.upgrade() {
                window.close();
            }
        }
    });

    window.set_content(Some(&webview));
    window.present();
}

fn extract_code_from_uri(uri: &str) -> Option<String> {
    let url = url::Url::parse(uri).ok()?;
    url.query_pairs()
        .find(|(k, _)| k == "code_autorisation")
        .map(|(_, v)| v.to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn init(
    client: Arc<Client>,
    app_id: String,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    position_receiver: PositionReceiver,
    controls: Controls,
    database: Arc<Database>,
    exit_sender: ExitSender,
) -> AppResult<()> {
    libadwaita::init().unwrap();

    let application = libadwaita::Application::builder()
        .application_id("io.github.sofusa.qobine")
        .build();

    let is_logged_in = client.credentials_is_set()?;

    let (login_sender, mut login_receiver) = mpsc::unbounded_channel::<String>();
    let (ui_sender, ui_receiver) = mpsc::unbounded_channel::<UiEvent>();
    let ui_receiver = RefCell::new(Some(ui_receiver));

    let app_id_for_window = app_id.clone();

    application.connect_activate({
        let client = client.clone();
        let tracklist_receiver = tracklist_receiver.clone();
        let status_receiver = status_receiver.clone();
        let position_receiver = position_receiver.clone();
        let controls = controls.clone();
        let exit_sender = exit_sender.clone();
        let login_sender = login_sender.clone();
        let ui_sender = ui_sender.clone();

        move |app| {
            if app.active_window().is_some() {
                return;
            }

            if !is_logged_in {
                let oauth_url = format!(
                    "https://www.qobuz.com/signin/oauth?ext_app_id={app_id_for_window}&redirect_url=http://localhost"
                );

                oauth_login_window(app, &oauth_url, login_sender.clone());
            }

            
            let ui_receiver = ui_receiver
                .borrow_mut()
                .take()
                .expect("activate called more than once");

            build_ui(
                app,
                tracklist_receiver.clone(),
                status_receiver.clone(),
                position_receiver.clone(),
                controls.clone(),
                client.clone(),
                exit_sender.clone(),
                ui_sender.clone(),
                ui_receiver
            );
        }
    });

    let client_clone = client.clone();
    let database_clone = database.clone();
    let app_id_for_exchange = app_id.clone();
    let ui_sender = ui_sender.clone();

    glib::MainContext::default().spawn_local(async move {
        if is_logged_in {
            ui_sender.send(UiEvent::FavoritesChanged).unwrap();
            return;
        }

        let result: AppResult<()> = async {
            let Some(code) = login_receiver.recv().await else {
                return Err(Error::Login {
                    message: "Error receiving login token".to_string(),
                });
            };

            let oauth = exchange_oauth_code(&code, &app_id_for_exchange).await?;
            let credentials: Credentials = oauth.into();
            client_clone.set_credentials(credentials.clone())?;
            database_clone.set_credentials(credentials).await?;
            ui_sender.send(UiEvent::FavoritesChanged).unwrap();

            Ok(())
        }
        .await;

        if let Err(err) = result {
            tracing::error!("login flow failed: {:?}", err);
        }
    });

    application.run();

    exit_sender.send(true).unwrap();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_ui(
    app: &Application,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    position_receiver: PositionReceiver,
    controls: Controls,
    client: Arc<Client>,
    exit_sender: ExitSender,
    ui_sender: mpsc::UnboundedSender<UiEvent>,
    ui_receiver: mpsc::UnboundedReceiver<UiEvent>,
) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Qobuz Player")
        .default_width(800)
        .default_height(1000)
        .build();

    let tabs = adw::ViewStack::builder().vexpand(true).build();

    let view_switcher = adw::ViewSwitcher::builder()
        .stack(&tabs)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();

    let header = adw::HeaderBar::builder()
        .title_widget(&view_switcher)
        .build();

    let root_toolbar = adw::ToolbarView::new();
    root_toolbar.add_top_bar(&header);
    root_toolbar.set_content(Some(&tabs));

    let root_page = adw::NavigationPage::builder()
        .title("Qobuz Player")
        .child(&root_toolbar)
        .build();

    let app_nav = adw::NavigationView::new();
    app_nav.add(&root_page);

    let detail_pages: Rc<RefCell<Vec<Rc<dyn DetailPage>>>> = Rc::new(RefCell::new(Vec::new()));

    {
        let detail_pages = detail_pages.clone();
        app_nav.connect_popped(move |_nav, popped_page| {
            let popped_ptr = popped_page.as_ptr() as usize;

            detail_pages.borrow_mut().retain(|p| {
                let page_ptr = p.page().as_ptr() as usize;
                page_ptr != popped_ptr
            });
        });
    }

    let callback_handles = Rc::new(build_callbacks(
        app_nav.clone(),
        controls.clone(),
        client.clone(),
        detail_pages.clone(),
        tracklist_receiver.clone(),
        ui_sender.clone(),
    ));

    let on_open_album = callback_handles.open_album.clone();
    let on_open_artist = callback_handles.open_artist.clone();
    let on_open_playlist = callback_handles.open_playlist.clone();

    let library_page = LibraryPage::new(
        client.clone(),
        on_open_album.clone(),
        on_open_artist.clone(),
        on_open_playlist.clone(),
    );

    tabs.add_titled(library_page.widget(), Some("library"), "Library")
        .set_icon_name(Some("audio-x-generic-symbolic"));

    let search_page = SearchPage::new(
        client.clone(),
        on_open_album.clone(),
        on_open_artist.clone(),
        on_open_playlist.clone(),
    );

    tabs.add_titled(search_page.widget(), Some("search"), "Search")
        .set_icon_name(Some("system-search-symbolic"));

    let now_playing = NowPlayingBar::new(
        controls,
        on_open_album.clone(),
        on_open_artist.clone(),
        on_open_playlist.clone(),
    );

    let vbox = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .build();
    vbox.append(&app_nav);
    vbox.append(&now_playing.revealer);

    window.set_content(Some(&vbox));

    window.present();

    let tracklist_value = tracklist_receiver.borrow().clone();
    update_now_playing(&now_playing, &tracklist_value);

    setup_tracklist_listener(
        ui_sender,
        ui_receiver,
        tracklist_receiver,
        status_receiver,
        position_receiver,
        now_playing,
        library_page,
        detail_pages,
        callback_handles,
        exit_sender,
    );
}

#[allow(clippy::too_many_arguments)]
fn setup_tracklist_listener(
    sender: mpsc::UnboundedSender<UiEvent>,
    mut receiver: mpsc::UnboundedReceiver<UiEvent>,
    mut tracklist_receiver: TracklistReceiver,
    mut status_receiver: StatusReceiver,
    mut position_receiver: PositionReceiver,
    now_playing_bar: NowPlayingBar,
    library_page: LibraryPage,
    detail_pages: Rc<RefCell<Vec<Rc<dyn DetailPage>>>>,
    callback_handles: Rc<CallbackHandles>,
    exit_sender: ExitSender,
) {
    let mut exit_receiver = exit_sender.subscribe();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(_) = tracklist_receiver.changed() => {
                    let tracklist = tracklist_receiver.borrow_and_update().clone();
                    sender.send(UiEvent::Tracklist(tracklist)).unwrap();
                }

                Ok(_) = status_receiver.changed() => {
                    let status = *status_receiver.borrow_and_update();
                    sender.send(UiEvent::Status(status)).unwrap();
                }

                Ok(_) = position_receiver.changed() => {
                    let position = *position_receiver.borrow_and_update();
                    sender.send(UiEvent::Position(position)).unwrap();
                }
                Ok(exit) = exit_receiver.recv() => {
                    if exit {
                        break;
                    }
                }
            }
        }
    });

    glib::MainContext::default().spawn_local(async move {
        let _keepalive = callback_handles;

        while let Some(update) = receiver.recv().await
        {
            match update {
                UiEvent::Tracklist(tracklist) => {
                    update_now_playing(&now_playing_bar, &tracklist);

                    if let Some(entity) = tracklist.current_playing_entity() {
                        for page in detail_pages.borrow().iter() {
                            page.update_current_playing(entity.clone());
                        }
                    }
                }
                UiEvent::Status(status) => {
                    update_now_playing_button_icon(&status, &now_playing_bar.play_button);
                }
                UiEvent::Position(duration) => {
                    update_progress(&now_playing_bar, &duration);
                }
                UiEvent::FavoritesChanged => {
                    library_page.reload();
                }
            }
        }
    });
}

enum UiEvent {
    Tracklist(Tracklist),
    Status(Status),
    Position(Duration),
    FavoritesChanged,
}
