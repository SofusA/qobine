use std::sync::Arc;

use futures::executor::block_on;
use glib::clone;
use gtk::gio;
use gtk::prelude::*;
use gtk4 as gtk;

use adw::prelude::*;
use libadwaita as adw;
use qobuz_player_controls::ExitSender;
use qobuz_player_controls::VolumeReceiver;
use qobuz_player_controls::controls::Controls;
use qobuz_player_player::AudioQuality;
use qobuz_player_player::database::Configuration;
use qobuz_player_player::database::Database;
use tokio::sync::mpsc;

use crate::UiEventSender;

pub fn build_preferences_menu(
    app: &adw::Application,
    controls: Controls,
    database: Arc<Database>,
    volume_receiver: VolumeReceiver,
    exit_sender: ExitSender,
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
    ui_event_sender: UiEventSender,
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
        move |_, _| {
            if let Err(error) = ui_event_sender.send(crate::UiEvent::Exit) {
                tracing::error!("Error sending ui event: {error}");
            };
        }
    });

    let preferences_action = gio::SimpleAction::new("preferences", None);

    preferences_action.connect_activate({
        clone!(
            #[weak]
            app,
            #[strong]
            controls,
            #[weak]
            database,
            #[strong]
            volume_receiver,
            #[strong]
            exit_sender,
            #[strong]
            audio_cache_ttl_sender,
            move |_, _| {
                show_preferences_dialog(
                    &app,
                    controls.clone(),
                    database,
                    volume_receiver.clone(),
                    exit_sender.clone(),
                    audio_cache_ttl_sender.clone(),
                );
            }
        )
    });
    app.add_action(&preferences_action);
    app.add_action(&quit_action);

    button
}

fn show_preferences_dialog(
    app: &adw::Application,
    controls: Controls,
    database: Arc<Database>,
    volume_receiver: VolumeReceiver,
    exit_sender: ExitSender,
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
) {
    let dialog = adw::PreferencesDialog::new();

    dialog.add(&preferences_page(
        app,
        controls,
        database,
        volume_receiver,
        exit_sender,
        audio_cache_ttl_sender,
    ));
    dialog.present(app.active_window().as_ref());
}

fn preferences_page(
    app: &adw::Application,
    controls: Controls,
    database: Arc<Database>,
    volume_receiver: VolumeReceiver,
    exit_sender: ExitSender,
    audio_cache_ttl_sender: mpsc::UnboundedSender<u32>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    page.set_title("Preferences");

    let configuration = block_on(database.get_configuration()).unwrap();

    page.add(&cache_group(
        app,
        controls.clone(),
        audio_cache_ttl_sender,
        &configuration,
    ));

    page.add(&audio_group(
        controls.clone(),
        volume_receiver,
        &configuration,
    ));

    page.add(&queue_group(controls, &configuration));

    page.add(&logout_group(exit_sender, database));

    page
}

fn cache_group(
    app: &adw::Application,
    controls: Controls,
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
            0 => 0,
            1 => 1,
            2 => 720,
            3 => 2160,
            _ => 0,
        };
        audio_cache_ttl_sender.send(hours).unwrap();
    });

    row
}

fn audio_group(
    controls: Controls,
    volume_receiver: VolumeReceiver,
    configuration: &Configuration,
) -> adw::PreferencesGroup {
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
                3 => AudioQuality::HIFI192,
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
        let controls = controls.clone();
        move |row| {
            controls.set_use_file_based_streaming(row.is_active());
        }
    });

    group.add(&file_based_streaming);

    let volume = adw::ActionRow::new();
    volume.set_title("Volume");

    let initial_value = volume_receiver.borrow();
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.1);
    scale.set_value(*initial_value as f64);
    scale.set_hexpand(true);

    scale.connect_value_changed(clone!(move |s| {
        controls.set_volume(s.value() as f32);
    }));

    volume.add_suffix(&scale);
    volume.set_activatable_widget(Some(&scale));

    group.add(&volume);
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
                exit_sender.send(true).unwrap();
            }
        });
    });

    group.add(&logout);
    group
}
