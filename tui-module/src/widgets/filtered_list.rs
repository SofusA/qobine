use ratatui::widgets::TableState;

#[derive(Default)]
pub struct FilteredListState<T> {
    filter: Vec<T>,
    all_items: Vec<T>,
    pub state: TableState,
}

impl<T> FilteredListState<T>
where
    T: Clone,
{
    pub fn new(list: Vec<T>) -> Self {
        Self {
            filter: list.clone(),
            all_items: list,
            state: Default::default(),
        }
    }

    pub fn filter(&self) -> &[T] {
        &self.filter
    }

    pub fn all_items(&self) -> &[T] {
        &self.all_items
    }

    pub fn set_all_items(&mut self, items: Vec<T>) {
        self.all_items = items.clone();
        self.filter = items;
    }

    pub fn set_filter(&mut self, items: Vec<T>) {
        self.filter = items;
    }

    pub fn remove_at_index(&mut self, index: usize) {
        if index >= self.all_items.len() {
            return;
        }

        self.all_items.remove(index);
        self.filter = self.all_items.clone();
    }

    pub fn move_index_to_new_index(&mut self, index: usize, new_index: usize) {
        if index >= self.all_items.len() || new_index >= self.all_items.len() {
            return;
        }

        let item = self.all_items.remove(index);
        self.all_items.insert(new_index, item);

        self.filter = self.all_items.clone();
    }
}
