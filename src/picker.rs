use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::action::EditorAction;

#[derive(Debug, Clone)]
pub struct PickerItem {
    pub match_text: String,
    pub display: String,
    pub group: Option<String>,
    pub action: EditorAction,
    pub preview_action: Option<EditorAction>,
}

pub const PICKER_VISIBLE_LIMIT: usize = 10;

pub struct Picker {
    pub items: Vec<PickerItem>,
    pub query: String,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub visible_limit: usize,
    pub title: String,
    pub restore_buffer: Option<usize>,
}

impl Picker {
    pub fn new(title: &str, items: Vec<PickerItem>, restore_buffer: Option<usize>) -> Self {
        let filtered: Vec<usize> = (0..items.len()).collect();
        Self {
            items,
            query: String::new(),
            filtered,
            selected: 0,
            scroll_offset: 0,
            visible_limit: PICKER_VISIBLE_LIMIT,
            title: title.to_string(),
            restore_buffer,
        }
    }

    pub fn update_filter(&mut self) {
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(
            self.query.as_str(),
            CaseMatching::Ignore,
            Normalization::Smart,
        );

        let mut scores: Vec<(usize, u32)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                let mut buf = Vec::new();
                let score =
                    pattern.score(Utf32Str::new(&item.match_text, &mut buf), &mut matcher)?;
                Some((i, score))
            })
            .collect();

        scores.sort_by(|a, b| b.1.cmp(&a.1));
        self.filtered = scores.into_iter().map(|(i, _)| i).collect();

        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
        self.scroll_offset = self
            .scroll_offset
            .min(self.filtered.len().saturating_sub(self.visible_limit));
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() && self.selected < self.filtered.len() - 1 {
            self.selected += 1;
            if self.selected >= self.scroll_offset + self.visible_limit {
                self.scroll_offset = self.selected - self.visible_limit + 1;
            }
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

    pub fn visible_items(&self) -> impl Iterator<Item = (usize, &usize)> {
        let end = (self.scroll_offset + self.visible_limit).min(self.filtered.len());
        self.filtered[self.scroll_offset..end]
            .iter()
            .enumerate()
            .map(move |(i, idx)| (self.scroll_offset + i, idx))
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
