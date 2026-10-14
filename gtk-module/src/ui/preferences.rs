use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use cli_module::ParsedDisconnect;
use cli_module::SharedPlaybackConfig;
use futures::executor::block_on;
use gtk::gio;
use gtk::prelude::*;
use gtk4 as gtk;

use adw::prelude::*;
use controls_module::ExitSender;
use controls_module::controls::Controls;
use libadwaita as adw;
use player_module::AudioQuality;
use player_module::database::Configuration;
use player_module::database::Database;
use tokio::sync::mpsc;

use crate::UiEventSender;

pub fn build_preferences_menu(
    app: &adw::Application,
    controls: &Controls,
    database: Arc<Database>,
    exit_sender: &ExitSender,
    audio_cache_ttl_sender: &mpsc::UnboundedSender<u32>,
    ui_event_sender: &UiEventSender,
) -> gtk::MenuButton {
    let menu = gio::Menu::new();
    menu.append(Some("Preferences"), Some("app.preferences"));
    menu.append(Some("Quit"), Some("app.exit"));

    let menu_model = gtk::PopoverMenu::from_model(Some(&menu));

    let button = gtk::MenuButton::new();
    button.set_icon_name("open-menu-symbolic");
    button.set_popover(Some(&menu_model));

    let quit_action = gio::SimpleAction::new("exit", None);

    quit_action.connect_activate({
        let ui_event_sender = ui_event_sender.clone();

        move |_, _| {
            if let Err(error) = ui_event_sender.send(crate::UiEvent::Exit) {
                tracing::error!("Error sending UI event: {error}");
            }
        }
    });

    let preferences_action = gio::SimpleAction::new("preferences", None);

    preferences_action.connect_activate({
        let app = app.clone();
        let controls = controls.clone();
        let exit_sender = exit_sender.clone();
        let audio_cache_ttl_sender = audio_cache_ttl_sender.clone();
        let ui_event_sender = ui_event_sender.clone();

        move |_, _| {
            show_preferences_dialog(
                &app,
                controls.clone(),
                database.clone(),
                exit_sender.clone(),
                audio_cache_ttl_sender.clone(),
                &ui_event_sender,
            );
        }
    });

    app.add_action(&preferences_action);
    app.add_action(&quit_action);

    button
}

fn show_preferences_dialog(
    app: &adw::Application,
    controls: Controls,
    database: Arc<Database>,
    exit_sender: ExitSender,
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
    ui_event_sender: &UiEventSender,
) {
    let dialog = adw::PreferencesDialog::new();

    dialog.add(&preferences_page(
        app,
        controls,
        database,
        exit_sender,
        audio_cache_ttl_sender,
        ui_event_sender,
    ));
    dialog.present(app.active_window().as_ref());
}

fn preferences_page(
    app: &adw::Application,
    controls: Controls,
    database: Arc<Database>,
    exit_sender: ExitSender,
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
    ui_event_sender: &UiEventSender,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    page.set_title("Preferences");

    let Ok(configuration) = block_on(database.get_configuration()) else {
        return page;
    };

    page.add(&cache_group(
        app,
        &controls,
        audio_cache_ttl_sender,
        &configuration,
    ));

    page.add(&audio_group(controls.clone(), &configuration));

    page.add(&queue_group(controls, &configuration));

    page.add(&shared_playback_group(
        &configuration,
        ui_event_sender,
        database.clone(),
    ));
    page.add(&logout_group(exit_sender, database));

    page
}

fn cache_group(
    app: &adw::Application,
    controls: &Controls,
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
    configuration: &Configuration,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Cache");

    let initial_cache_dir = configuration.cache_directory.clone();

    let row = adw::ActionRow::new();
    row.set_title("Cache directory");
    row.set_subtitle_lines(1);

    let initial_subtitle = initial_cache_dir.display().to_string();

    row.set_subtitle(&initial_subtitle);
    row.set_activatable(true);

    row.connect_activated({
        glib::clone!(
            #[weak]
            app,
            #[weak]
            row,
            #[strong]
            initial_cache_dir,
            #[strong]
            controls,
            move |_| {
                let file_dialog = gtk::FileDialog::builder()
                    .title("Select cache directory")
                    .modal(true)
                    .build();

                let gfile = gio::File::for_path(initial_cache_dir.clone());
                file_dialog.set_initial_folder(Some(&gfile));

                file_dialog.select_folder(
                    app.active_window().as_ref(),
                    gio::Cancellable::NONE,
                    glib::clone!(
                        #[weak]
                        row,
                        #[strong]
                        controls,
                        move |result| {
                            if let Ok(folder) = result
                                && let Some(path) = folder.path()
                            {
                                row.set_subtitle(&path.display().to_string());
                                controls.set_audio_cache_directory(path);
                            }
                        }
                    ),
                );
            }
        )
    });

    group.add(&row);
    group.add(&cache_ttl_row(audio_cache_ttl_sender, configuration));

    group
}

fn cache_ttl_row(
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
    configuration: &Configuration,
) -> adw::ComboRow {
    let row = adw::ComboRow::new();
    row.set_title("Cache time to live");

    let mut options = vec!["Disabled", "1 hour", "1 month", "3 months"];

    let ttl = configuration.cache_ttl_hours;

    // 1 month = 720 hours
    // 3 moths = 2160 hours

    if ![0, 1, 720, 2160].contains(&ttl) {
        options.push("Other");
    }

    let selected = match ttl {
        0 => 0,
        1 => 1,
        720 => 2,
        2160 => 3,
        _ => 4,
    };

    let model = gtk::StringList::new(&options);

    row.set_model(Some(&model));
    row.set_selected(selected);

    row.connect_selected_notify(move |r| {
        let hours = match r.selected() {
            1 => 1,
            2 => 720,
            3 => 2160,
            _ => 0,
        };
        _ = audio_cache_ttl_sender.send(hours);
    });

    row
}

fn audio_group(controls: Controls, configuration: &Configuration) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Audio");

    let initial_selected = match configuration.max_audio_quality {
        AudioQuality::Mp3 => 0,
        AudioQuality::CD => 1,
        AudioQuality::HIFI96 => 2,
        AudioQuality::HIFI192 => 3,
    };

    let quality = adw::ComboRow::new();
    quality.set_title("Audio quality");

    let model = gtk::StringList::new(&[
        AudioQuality::Mp3.to_label_str(),
        AudioQuality::CD.to_label_str(),
        AudioQuality::HIFI96.to_label_str(),
        AudioQuality::HIFI192.to_label_str(),
    ]);

    quality.set_model(Some(&model));
    quality.set_selected(initial_selected);

    quality.connect_selected_notify({
        let controls = controls.clone();
        move |r| {
            let value = match r.selected() {
                0 => AudioQuality::Mp3,
                1 => AudioQuality::CD,
                2 => AudioQuality::HIFI96,
                _ => AudioQuality::HIFI192,
            };
            controls.set_audio_max_quality(value);
        }
    });

    group.add(&quality);

    let file_based_streaming = adw::SwitchRow::new();
    file_based_streaming.set_title("Use file based streaming");
    file_based_streaming.set_active(configuration.use_file_based_streaming);

    file_based_streaming.connect_active_notify({
        let controls = controls;
        move |row| {
            controls.set_use_file_based_streaming(row.is_active());
        }
    });

    group.add(&file_based_streaming);
    group
}

fn queue_group(controls: Controls, configuration: &Configuration) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Queue");

    let auto_play = adw::SwitchRow::new();
    auto_play.set_title("Add similar tracks to empty queue");
    auto_play.set_active(configuration.auto_play);

    auto_play.connect_active_notify(move |row| {
        controls.set_auto_play(row.is_active());
    });

    group.add(&auto_play);
    group
}

fn logout_group(exit_sender: ExitSender, database: Arc<Database>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();

    let logout = adw::ButtonRow::new();
    logout.set_title("Log out");
    logout.add_css_class("destructive-action");

    logout.connect_activated(move |_| {
        let database = database.clone();
        let exit_sender = exit_sender.clone();

        glib::MainContext::default().spawn_local(async move {
            if database.set_credentials(None).await.is_ok() {
                _ = exit_sender.send(true);
            }
        });
    });

    group.add(&logout);
    group
}

fn shared_playback_group(
    configuration: &Configuration,
    ui_event_sender: &UiEventSender,
    database: Arc<Database>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Shared playback");

    let info = gtk::Button::builder()
        .icon_name("info-outline-symbolic")
        .tooltip_text("Open Disconnect information page")
        .css_classes(["flat"])
        .build();

    info.connect_clicked(move |_| {
        if let Err(error) = gio::AppInfo::launch_default_for_uri(
            "https://github.com/SofusA/qobine/tree/main/disconnect-module",
            None::<&gio::AppLaunchContext>,
        ) {
            tracing::error!("Failed to open information page: {error}");
        }
    });

    group.set_header_suffix(Some(&info));

    /*
     * Shared playback type.
     */

    let playback_type_row = adw::ActionRow::new();
    playback_type_row.set_title("Shared playback type");

    let playback_type = adw::ToggleGroup::new();
    playback_type.set_homogeneous(true);
    playback_type.set_can_shrink(true);
    playback_type.set_valign(gtk::Align::Center);

    let disabled_toggle = adw::Toggle::builder()
        .name("disabled")
        .label("Disabled")
        .build();

    let connect_toggle = adw::Toggle::builder()
        .name("connect")
        .label("Connect")
        .build();

    let disconnect_toggle = adw::Toggle::builder()
        .name("disconnect")
        .label("Disconnect")
        .build();

    playback_type.add(disabled_toggle);
    playback_type.add(connect_toggle);
    playback_type.add(disconnect_toggle);

    let initial_type = if configuration.enable_connect {
        "connect"
    } else if configuration.enable_disconnect {
        "disconnect"
    } else {
        "disabled"
    };

    playback_type.set_active_name(Some(initial_type));
    playback_type_row.add_suffix(&playback_type);

    /*
     * Connect configuration.
     */

    let connect_device_row = adw::EntryRow::new();
    connect_device_row.set_title("Device name");
    connect_device_row.set_text(configuration.device_name.as_deref().unwrap_or_default());

    /*
     * Disconnect configuration.
     */

    let server_row = adw::EntryRow::new();
    server_row.set_title("Server URL");
    server_row.set_text(
        configuration
            .disconnect_server_url
            .as_deref()
            .unwrap_or_default(),
    );

    let password_row = adw::PasswordEntryRow::new();
    password_row.set_title("Password");
    password_row.set_text(
        configuration
            .disconnect_password
            .as_deref()
            .unwrap_or_default(),
    );

    let disconnect_device_row = adw::EntryRow::new();
    disconnect_device_row.set_title("Device name");
    disconnect_device_row.set_text(configuration.device_name.as_deref().unwrap_or_default());

    /*
     * Save button.
     */

    let save = adw::ButtonRow::new();
    save.set_title("Save shared playback configuration");
    save.add_css_class("suggested-action");
    save.set_sensitive(false);

    /*
     * Capture the initially persisted state.
     *
     * All stored values are included, including values belonging to the
     * inactive implementation.
     */

    let initial_connect_device_name = configuration.device_name.clone().unwrap_or_default();

    let initial_server_url = configuration
        .disconnect_server_url
        .clone()
        .unwrap_or_default();

    let initial_password = configuration
        .disconnect_password
        .clone()
        .unwrap_or_default();

    let initial_disconnect_device_name = configuration.device_name.clone().unwrap_or_default();

    let initial_config = match initial_type {
        "connect" => SharedPlaybackConfig::Connect(initial_connect_device_name.clone()),

        "disconnect" => SharedPlaybackConfig::Disconnect(ParsedDisconnect {
            server_url: initial_server_url.clone(),
            password: initial_password.clone(),
            device_name: initial_disconnect_device_name.clone(),
        }),

        _ => SharedPlaybackConfig::None,
    };

    let saved_state = Rc::new(RefCell::new((
        initial_config,
        initial_connect_device_name,
        initial_server_url,
        initial_password,
        initial_disconnect_device_name,
    )));

    /*
     * Read the complete current form state.
     */

    let current_state = {
        let playback_type = playback_type.clone();
        let connect_device_row = connect_device_row.clone();
        let server_row = server_row.clone();
        let password_row = password_row.clone();
        let disconnect_device_row = disconnect_device_row.clone();

        Rc::new(move || {
            let connect_device_name = connect_device_row.text().trim().to_owned();

            let server_url = server_row.text().trim().to_owned();

            let password = password_row.text().to_string();

            let disconnect_device_name = disconnect_device_row.text().trim().to_owned();

            let config = match playback_type.active_name().as_deref() {
                Some("connect") => SharedPlaybackConfig::Connect(connect_device_name.clone()),

                Some("disconnect") => SharedPlaybackConfig::Disconnect(ParsedDisconnect {
                    server_url: server_url.clone(),
                    password: password.clone(),
                    device_name: disconnect_device_name.clone(),
                }),

                _ => SharedPlaybackConfig::None,
            };

            (
                config,
                connect_device_name,
                server_url,
                password,
                disconnect_device_name,
            )
        })
    };

    /*
     * Update row visibility, validate visible fields, and update the Save
     * button.
     */

    let validate = {
        let playback_type = playback_type.clone();
        let connect_device_row = connect_device_row.clone();
        let server_row = server_row.clone();
        let password_row = password_row.clone();
        let disconnect_device_row = disconnect_device_row.clone();
        let save = save.clone();
        let saved_state = saved_state.clone();
        let current_state = current_state.clone();

        Rc::new(move || {
            let active_name = playback_type.active_name();

            let connect_active = active_name.as_deref() == Some("connect");

            let disconnect_active = active_name.as_deref() == Some("disconnect");

            /*
             * Only show inputs belonging to the selected implementation.
             */

            connect_device_row.set_visible(connect_active);

            server_row.set_visible(disconnect_active);
            password_row.set_visible(disconnect_active);
            disconnect_device_row.set_visible(disconnect_active);

            /*
             * Only validate inputs belonging to the selected implementation.
             */

            let connect_name_valid =
                !connect_active || !connect_device_row.text().trim().is_empty();

            let server_valid = !disconnect_active || !server_row.text().trim().is_empty();

            let password_valid = !disconnect_active || !password_row.text().is_empty();

            let disconnect_name_valid =
                !disconnect_active || !disconnect_device_row.text().trim().is_empty();

            connect_device_row.set_css_classes(if connect_name_valid { &[] } else { &["error"] });

            server_row.set_css_classes(if server_valid { &[] } else { &["error"] });

            password_row.set_css_classes(if password_valid { &[] } else { &["error"] });

            disconnect_device_row.set_css_classes(if disconnect_name_valid {
                &[]
            } else {
                &["error"]
            });

            let valid =
                connect_name_valid && server_valid && password_valid && disconnect_name_valid;

            let changed = *saved_state.borrow() != current_state();

            save.set_sensitive(valid && changed);
        })
    };

    playback_type.connect_active_name_notify({
        let validate = validate.clone();

        move |_| {
            validate();
        }
    });

    connect_device_row.connect_changed({
        let validate = validate.clone();

        move |_| {
            validate();
        }
    });

    server_row.connect_changed({
        let validate = validate.clone();

        move |_| {
            validate();
        }
    });

    password_row.connect_changed({
        let validate = validate.clone();

        move |_| {
            validate();
        }
    });

    disconnect_device_row.connect_changed({
        let validate = validate.clone();

        move |_| {
            validate();
        }
    });

    /*
     * Apply and persist the configuration.
     */

    save.connect_activated({
        let playback_type = playback_type.clone();
        let saved_state = saved_state.clone();
        let current_state = current_state.clone();
        let validate = validate.clone();
        let ui_event_sender = ui_event_sender.clone();

        move |_| {
            let (config, connect_device_name, server_url, password, disconnect_device_name) =
                current_state();

            let active_name = playback_type.active_name();

            let enable_connect = active_name.as_deref() == Some("connect");

            let enable_disconnect = active_name.as_deref() == Some("disconnect");

            /*
             * Apply the new runtime configuration immediately.
             */

            if let Err(error) =
                ui_event_sender.send(crate::UiEvent::SharedPlaybackConfig(config.clone()))
            {
                tracing::error!("Failed to apply shared playback configuration: {error}");
                return;
            }

            saved_state.borrow_mut().clone_from(&(
                config,
                connect_device_name.clone(),
                server_url.clone(),
                password.clone(),
                disconnect_device_name.clone(),
            ));

            validate();

            /*
             * Persist the configuration asynchronously.
             */

            let database = Arc::clone(&database);

            glib::MainContext::default().spawn_local(async move {
                /*
                 * Preserve the values for both implementations, even when
                 * they are not currently selected.
                 */

                if let Err(error) = database.set_device_name(&connect_device_name).await {
                    tracing::error!("Failed to save Connect device name: {error}");
                    return;
                }

                if let Err(error) = database
                    .set_disconnect_config(&server_url, &password, &disconnect_device_name)
                    .await
                {
                    tracing::error!("Failed to save Disconnect configuration: {error}");
                    return;
                }

                /*
                 * Disable the inactive implementation before enabling the
                 * selected implementation.
                 */

                if enable_connect {
                    if let Err(error) = database.set_disconnect_enabled(false).await {
                        tracing::error!("Failed to disable Disconnect: {error}");
                        return;
                    }

                    if let Err(error) = database.set_connect_enabled(true).await {
                        tracing::error!("Failed to enable Connect: {error}");
                    }
                } else if enable_disconnect {
                    if let Err(error) = database.set_connect_enabled(false).await {
                        tracing::error!("Failed to disable Connect: {error}");
                        return;
                    }

                    if let Err(error) = database.set_disconnect_enabled(true).await {
                        tracing::error!("Failed to enable Disconnect: {error}");
                    }
                } else {
                    if let Err(error) = database.set_connect_enabled(false).await {
                        tracing::error!("Failed to disable Connect: {error}");
                    }

                    if let Err(error) = database.set_disconnect_enabled(false).await {
                        tracing::error!("Failed to disable Disconnect: {error}");
                    }
                }
            });
        }
    });

    group.add(&playback_type_row);
    group.add(&connect_device_row);
    group.add(&server_row);
    group.add(&password_row);
    group.add(&disconnect_device_row);
    group.add(&save);

    validate();

    group
}
