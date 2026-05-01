use libadwaita as adw;

use adw::prelude::*;
use gtk4 as gtk;

pub struct DetailHeaderParts {
    pub header_section: gtk::Box,
    pub cover: gtk::Image,
}

pub fn build_detail_header<F>(
    cover_pixel_size: i32,
    text_rows: Vec<gtk::Widget>,
    buttons: Vec<gtk::Widget>,
    make_favorite_button: F,
) -> DetailHeaderParts
where
    F: FnOnce() -> gtk::Widget,
{
    let cover = gtk::Image::builder().pixel_size(cover_pixel_size).build();

    let cover_frame = gtk::Frame::builder()
        .valign(gtk::Align::End)
        .css_classes(vec!["card"])
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
    button_box.append(&make_favorite_button());
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
    }
}

pub struct DetailScaffoldParts {
    pub stack: gtk::Stack,
    pub content: gtk::Box,
    pub tracks_list: gtk::ListBox,
}

pub fn build_detail_scaffold(header_section: &impl IsA<gtk::Widget>) -> DetailScaffoldParts {
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
