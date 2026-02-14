use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

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
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(
            self.query.as_str(),
            CaseMatching::Ignore,
            Normalization::Smart,
        );

        // TODO: Could take this off render thread eventually
        let mut scores: Vec<(usize, u32)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                let mut buf = Vec::new();
                let score = pattern.score(Utf32Str::new(&item.label, &mut buf), &mut matcher)?;
                Some((i, score))
            })
            .collect();

        scores.sort_by(|a, b| b.1.cmp(&a.1));
        self.filtered = scores.into_iter().map(|(i, _)| i).collect();

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
