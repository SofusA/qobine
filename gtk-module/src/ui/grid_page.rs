use std::cell::Ref;
use std::marker::PhantomData;
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

    _marker: PhantomData<T>,
}

impl<T: 'static> Clone for GridPage<T> {
    fn clone(&self) -> Self {
        Self {
            widget: self.widget.clone(),
            store: self.store.clone(),
            filter: self.filter.clone(),
            query: Rc::clone(&self.query),
            filter_model: self.filter_model.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T: 'static> GridPage<T> {
    pub fn new<M, B, A>(
        min_columns: u32,
        max_columns: u32,
        alignment: gtk::Align,
        matches_query: M,
        build_tile: B,
        on_activate: A,
    ) -> Self
    where
        M: Fn(&T, &str) -> bool + 'static,
        B: Fn(&T) -> gtk::Widget + 'static,
        A: Fn(&T) + 'static,
    {
        let store = gio::ListStore::new::<BoxedAnyObject>();
        let query = Rc::new(RefCell::new(String::new()));

        /*
         * The query is stored by GridPage and captured by the filter.
         * matches_query is moved directly into the filter callback.
         */
        let query_for_filter = Rc::clone(&query);

        let filter = gtk::CustomFilter::new(move |object| {
            let Some(boxed) = object.downcast_ref::<BoxedAnyObject>() else {
                return false;
            };

            let item: Ref<'_, T> = boxed.borrow();
            let query = query_for_filter.borrow();
            let normalized_query = query.trim().to_lowercase();

            normalized_query.is_empty() || matches_query(&item, &normalized_query)
        });

        let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));

        let selection_model = gtk::NoSelection::new(Some(filter_model.clone()));

        let factory = gtk::SignalListItemFactory::new();

        factory.connect_setup(|_, object| {
            let Some(list_item) = object.downcast_ref::<gtk::ListItem>() else {
                return;
            };

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

        /*
         * build_tile is moved directly into the bind callback.
         */
        factory.connect_bind(move |_, object| {
            let Some(list_item) = object.downcast_ref::<gtk::ListItem>() else {
                return;
            };

            let Some(wrapper) = list_item.child().and_downcast::<gtk::Box>() else {
                return;
            };

            let Some(boxed) = list_item.item().and_downcast::<BoxedAnyObject>() else {
                return;
            };

            while let Some(child) = wrapper.first_child() {
                wrapper.remove(&child);
            }

            let item: Ref<'_, T> = boxed.borrow();
            let tile = build_tile(&item);

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

        /*
         * GridPage retains the original filter model, while this signal
         * callback owns another GTK reference-counted handle.
         */
        let filter_model_for_activate = filter_model.clone();

        grid.connect_activate(move |_grid, position| {
            let Some(object) = filter_model_for_activate.item(position) else {
                return;
            };

            let Ok(boxed) = object.downcast::<BoxedAnyObject>() else {
                return;
            };

            let item: Ref<'_, T> = boxed.borrow();

            on_activate(&item);
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
            _marker: PhantomData,
        }
    }

    pub const fn widget(&self) -> &gtk::ScrolledWindow {
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
