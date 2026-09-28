use std::cell::{Ref, RefCell};
use std::cmp::Ordering;
use std::marker::PhantomData;
use std::rc::Rc;

use glib::BoxedAnyObject;
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FavoriteSort {
    #[default]
    DateAdded,
    Alphabetical,
}

pub struct GridPage<T: 'static> {
    widget: gtk::ScrolledWindow,
    store: gio::ListStore,
    sorter: gtk::CustomSorter,
    sort: Rc<RefCell<FavoriteSort>>,
    filter: gtk::CustomFilter,
    query: Rc<RefCell<String>>,
    filter_model: gtk::FilterListModel,
    sort_model: gtk::SortListModel,
    _marker: PhantomData<T>,
}

impl<T: 'static> Clone for GridPage<T> {
    fn clone(&self) -> Self {
        Self {
            widget: self.widget.clone(),
            store: self.store.clone(),
            sorter: self.sorter.clone(),
            sort: Rc::clone(&self.sort),
            filter: self.filter.clone(),
            query: Rc::clone(&self.query),
            filter_model: self.filter_model.clone(),
            sort_model: self.sort_model.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T: 'static> GridPage<T> {
    pub fn new<M, B, A, S>(
        min_columns: u32,
        max_columns: u32,
        alignment: gtk::Align,
        matches_query: M,
        alphabetical_compare: S,
        build_tile: B,
        on_activate: A,
    ) -> Self
    where
        M: Fn(&T, &str) -> bool + 'static,
        B: Fn(&T) -> gtk::Widget + 'static,
        A: Fn(&T) + 'static,
        S: Fn(&T, &T) -> Ordering + 'static,
    {
        let store = gio::ListStore::new::<BoxedAnyObject>();
        let query = Rc::new(RefCell::new(String::new()));
        let sort = Rc::new(RefCell::new(FavoriteSort::DateAdded));

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

        let sort_for_sorter = Rc::clone(&sort);

        let sorter = gtk::CustomSorter::new(move |a, b| {
            if *sort_for_sorter.borrow() == FavoriteSort::DateAdded {
                return gtk::Ordering::Equal;
            }

            let Some(a) = a.downcast_ref::<BoxedAnyObject>() else {
                return gtk::Ordering::Equal;
            };

            let Some(b) = b.downcast_ref::<BoxedAnyObject>() else {
                return gtk::Ordering::Equal;
            };

            let a: Ref<'_, T> = a.borrow();
            let b: Ref<'_, T> = b.borrow();

            match alphabetical_compare(&a, &b) {
                Ordering::Less => gtk::Ordering::Smaller,
                Ordering::Equal => gtk::Ordering::Equal,
                Ordering::Greater => gtk::Ordering::Larger,
            }
        });

        let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));

        let sort_model = gtk::SortListModel::new(Some(filter_model.clone()), Some(sorter.clone()));

        let selection_model = gtk::NoSelection::new(Some(sort_model.clone()));
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

        let sort_model_for_activate = sort_model.clone();

        grid.connect_activate(move |_grid, position| {
            let Some(object) = sort_model_for_activate.item(position) else {
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
            sorter,
            sort,
            filter,
            query,
            filter_model,
            sort_model,
            _marker: PhantomData,
        }
    }

    pub const fn widget(&self) -> &gtk::ScrolledWindow {
        &self.widget
    }

    pub fn load(&mut self, items: Vec<T>) {
        self.store.remove_all();

        for item in items {
            self.store.append(&BoxedAnyObject::new(item));
        }

        *self.query.borrow_mut() = String::new();
        self.filter.changed(gtk::FilterChange::Different);
    }

    pub fn set_sort(&self, sort: FavoriteSort) {
        if *self.sort.borrow() == sort {
            return;
        }

        *self.sort.borrow_mut() = sort;
        self.sorter.changed(gtk::SorterChange::Different);
    }

    pub fn filter(&self, query: &str) {
        *self.query.borrow_mut() = query.trim().to_string();
        self.filter.changed(gtk::FilterChange::Different);
    }

    pub fn clear(&self) {
        self.store.remove_all();
        *self.query.borrow_mut() = String::new();
        self.filter.changed(gtk::FilterChange::Different);
    }
}
