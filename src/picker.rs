pub struct PickerItem {
    pub id: usize,
    pub label: String,
}

pub struct Picker {
    pub items: Vec<PickerItem>,
    pub query: String,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub title: String,
}

impl Picker {
    pub fn new(title: &str, items: Vec<PickerItem>) -> Self {
        let filtered: Vec<usize> = (0..items.len()).collect();
        Self {
            items,
            query: String::new(),
            filtered,
            selected: 0,
            title: title.to_string(),
        }
    }

    pub fn update_filter(&mut self) {
        let query_lower = self.query.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                query_lower.is_empty() || item.label.to_lowercase().contains(&query_lower)
            })
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() && self.selected < self.filtered.len() - 1 {
            self.selected += 1;
        }
    }

    pub fn type_char(&mut self, ch: char) {
        self.query.push(ch);
        self.update_filter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.update_filter();
    }

    pub fn selected_item(&self) -> Option<&PickerItem> {
        self.filtered.get(self.selected).map(|&i| &self.items[i])
    }

    pub fn status_line(&self) -> String {
        format!(
            "{} ({}/{}): {}",
            self.title,
            self.filtered.len(),
            self.items.len(),
            self.query
        )
    }
}
