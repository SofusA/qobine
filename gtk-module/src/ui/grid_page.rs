use std::{cell::RefCell, rc::Rc};

use glib::BoxedAnyObject;
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;

pub struct GridPage<T: 'static> {
    widget: gtk::ScrolledWindow,

    store: gio::ListStore,
    filter: gtk::CustomFilter,
    query: Rc<RefCell<String>>,

    filter_model: gtk::FilterListModel,

    _marker: std::marker::PhantomData<T>,
}

impl<T: 'static> Clone for GridPage<T> {
    fn clone(&self) -> Self {
        Self {
            widget: self.widget.clone(),
            store: self.store.clone(),
            filter: self.filter.clone(),
            query: self.query.clone(),
            filter_model: self.filter_model.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: 'static> GridPage<T> {
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::type_complexity)]
    pub fn new(
        min_columns: u32,
        max_columns: u32,
        alignment: gtk::Align,
        matches_query: Rc<dyn Fn(&T, &str) -> bool>,
        build_tile: Rc<dyn Fn(&T) -> gtk::Widget>,
        on_activate: Rc<dyn Fn(&T)>,
    ) -> Self {
        let store = gio::ListStore::new::<BoxedAnyObject>();
        let query: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

        let query_for_filter = query.clone();
        let matches_query_for_filter = matches_query.clone();
        let filter = gtk::CustomFilter::new(move |obj: &glib::Object| {
            let boxed = obj
                .downcast_ref::<BoxedAnyObject>()
                .expect("Expected BoxedAnyObject in model");

            let item_ref: std::cell::Ref<T> = boxed.borrow();

            let q = query_for_filter.borrow();
            let q = q.trim().to_lowercase();
            if q.is_empty() {
                return true;
            }

            (matches_query_for_filter)(&item_ref, &q)
        });

        let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
        let selection_model = gtk::NoSelection::new(Some(filter_model.clone()));
        let factory = gtk::SignalListItemFactory::new();

        factory.connect_setup(|_, list_item| {
            let list_item = list_item
                .downcast_ref::<gtk::ListItem>()
                .expect("Needs to be a gtk::ListItem");

            list_item.set_activatable(true);

            let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
            wrapper.set_margin_top(6);
            wrapper.set_margin_bottom(6);
            wrapper.set_margin_start(6);
            wrapper.set_margin_end(6);

            wrapper.set_halign(gtk::Align::Center);
            wrapper.set_valign(gtk::Align::Start);

            list_item.set_child(Some(&wrapper));
        });

        let build_tile_for_bind = build_tile.clone();
        factory.connect_bind(move |_, list_item| {
            let list_item = list_item
                .downcast_ref::<gtk::ListItem>()
                .expect("Needs to be a gtk::ListItem");

            let wrapper = list_item
                .child()
                .and_downcast::<gtk::Box>()
                .expect("ListItem child must be gtk::Box");

            while let Some(child) = wrapper.first_child() {
                wrapper.remove(&child);
            }

            let boxed = list_item
                .item()
                .and_downcast::<BoxedAnyObject>()
                .expect("Model item must be BoxedAnyObject");

            let item_ref: std::cell::Ref<T> = boxed.borrow();
            let tile = (build_tile_for_bind)(&item_ref);

            wrapper.set_valign(gtk::Align::Fill);
            wrapper.set_vexpand(true);

            tile.set_valign(alignment);
            tile.set_vexpand(true);

            wrapper.append(&tile);
        });

        let grid = gtk::GridView::new(Some(selection_model), Some(factory));
        grid.set_vexpand(true);
        grid.set_hexpand(true);

        grid.set_min_columns(min_columns);
        grid.set_max_columns(max_columns);

        grid.set_single_click_activate(true);

        let filter_model_for_activate = filter_model.clone();
        let on_activate_for_signal = on_activate.clone();
        grid.connect_activate(move |_grid, pos| {
            if let Some(obj) = filter_model_for_activate.item(pos)
                && let Ok(boxed) = obj.downcast::<BoxedAnyObject>()
            {
                let item_ref: std::cell::Ref<T> = boxed.borrow();
                (on_activate_for_signal)(&item_ref);
            }
        });

        let scroller = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&grid)
            .build();

        Self {
            widget: scroller,
            store,
            filter,
            query,
            filter_model,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.widget
    }

    pub fn load(&mut self, items: Vec<T>) {
        self.clear_store();
        for item in items {
            self.store.append(&BoxedAnyObject::new(item));
        }

        *self.query.borrow_mut() = String::new();
        self.filter.changed(gtk::FilterChange::Different);
    }

    pub fn filter(&self, query: &str) {
        *self.query.borrow_mut() = query.trim().to_string();
        self.filter.changed(gtk::FilterChange::Different);
    }

    pub fn clear(&self) {
        self.clear_store();
        *self.query.borrow_mut() = String::new();
        self.filter.changed(gtk::FilterChange::Different);
    }

    fn clear_store(&self) {
        while self.store.n_items() > 0 {
            self.store.remove(0);
        }
    }
}
