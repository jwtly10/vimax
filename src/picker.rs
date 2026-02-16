use iced::keyboard;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::action::EditorAction;

#[derive(Debug, Clone)]
pub struct DetailSpan {
    pub text: String,
    pub color: iced::Color,
}

pub enum PickerEvent {
    Select(EditorAction),
    Cancel(Option<usize>),
    PreviewChanged(Option<EditorAction>),
    Noop,
}

#[derive(Debug, Clone)]
pub struct PickerItem {
    pub match_text: String,
    pub display: String,
    pub detail: Option<String>,
    pub detail_spans: Vec<DetailSpan>,
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

    /// Handles any key events into the Picker process
    /// and returns what action should be taken by the caller (selecting an item, canceling, etc.)
    pub fn handle_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        text: Option<&str>,
    ) -> PickerEvent {
        match key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                PickerEvent::Cancel(self.restore_buffer)
            }
            keyboard::Key::Named(keyboard::key::Named::Enter) => {
                if let Some(item) = self.selected_item() {
                    PickerEvent::Select(item.action.clone())
                } else {
                    PickerEvent::Cancel(self.restore_buffer)
                }
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                self.move_up();
                self.preview_event()
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                self.move_down();
                self.preview_event()
            }
            keyboard::Key::Character(c) if modifiers.control() && c.as_str() == "p" => {
                self.move_up();
                self.preview_event()
            }
            keyboard::Key::Character(c) if modifiers.control() && c.as_str() == "n" => {
                self.move_down();
                self.preview_event()
            }
            keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                self.backspace();
                self.preview_event()
            }
            _ => {
                // General key presses
                if let Some(t) = text {
                    let mut typed = false;
                    for ch in t.chars() {
                        if !ch.is_control() {
                            self.type_char(ch);
                            typed = true;
                        }
                    }
                    if typed {
                        self.preview_event()
                    } else {
                        PickerEvent::Noop
                    }
                } else {
                    PickerEvent::Noop
                }
            }
        }
    }

    fn preview_event(&self) -> PickerEvent {
        let action = self
            .selected_item()
            .and_then(|item| item.preview_action.clone());
        PickerEvent::PreviewChanged(action)
    }
}
