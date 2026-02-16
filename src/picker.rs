use iced::keyboard;
use iced::widget::{Space, column, container, rich_text, row, span, text};
use iced::{Element, Length, Theme};
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

    pub fn view<M: 'static>(&self) -> Element<'_, M> {
        let input_row = container(
            row![
                text(format!(" {} ", self.title))
                    .size(13)
                    .color(iced::Color::from_rgb(0.6, 0.7, 0.9)),
                container(
                    text(if self.query.is_empty() {
                        String::from("  Type to filter…")
                    } else {
                        format!("  {}", self.query)
                    })
                    .size(14)
                    .color(if self.query.is_empty() {
                        iced::Color::from_rgb(0.4, 0.4, 0.4)
                    } else {
                        iced::Color::from_rgb(0.95, 0.95, 0.8)
                    }),
                )
                .width(Length::Fill),
                text(format!("{}/{} ", self.filtered.len(), self.items.len()))
                    .size(13)
                    .color(iced::Color::from_rgb(0.45, 0.45, 0.45)),
            ]
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding([3, 2])
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgb(
                0.13, 0.13, 0.17,
            ))),
            ..Default::default()
        });

        let separator = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.25, 0.25, 0.3,
                ))),
                ..Default::default()
            });

        let mut items_col = column![];
        let mut current_group: Option<&str> = None;
        for (i, &item_idx) in self.visible_items() {
            let item = &self.items[item_idx];

            if let Some(group) = &item.group {
                if current_group != Some(group.as_str()) {
                    current_group = Some(group.as_str());
                    let header = container(
                        text(format!("  {}", group))
                            .size(12)
                            .color(iced::Color::from_rgb(0.5, 0.6, 0.8)),
                    )
                    .width(Length::Fill)
                    .padding([2, 4]);
                    items_col = items_col.push(header);
                }
            }

            let is_selected = i == self.selected;
            let indicator = if is_selected { " ▸ " } else { "   " };

            let display_text = text(format!("{}{}", indicator, item.display))
                .size(14)
                .color(if is_selected {
                    iced::Color::from_rgb(1.0, 1.0, 1.0)
                } else {
                    iced::Color::from_rgb(0.75, 0.75, 0.75)
                });

            let item_row = if !item.detail_spans.is_empty() {
                let dim: f32 = if is_selected { 1.0 } else { 0.55 };
                let spans: Vec<iced::widget::text::Span<'_, (), _>> = item
                    .detail_spans
                    .iter()
                    .map(|ds| {
                        span(ds.text.as_str())
                            .color(iced::Color::from_rgba(
                                ds.color.r * dim,
                                ds.color.g * dim,
                                ds.color.b * dim,
                                1.0,
                            ))
                            .size(13)
                    })
                    .collect();
                row![
                    display_text,
                    Space::new().width(Length::Fixed(12.0)),
                    rich_text(spans).font(iced::Font::MONOSPACE),
                ]
                .align_y(iced::Alignment::Center)
            } else if let Some(detail) = &item.detail {
                row![
                    display_text,
                    Space::new().width(Length::Fixed(12.0)),
                    text(detail.as_str()).size(13).color(if is_selected {
                        iced::Color::from_rgb(0.55, 0.6, 0.7)
                    } else {
                        iced::Color::from_rgb(0.38, 0.38, 0.42)
                    }),
                ]
                .align_y(iced::Alignment::Center)
            } else {
                row![display_text].align_y(iced::Alignment::Center)
            };

            let row_widget = container(item_row).width(Length::Fill).padding([2, 4]);
            let row_widget = if is_selected {
                row_widget.style(|_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgb(
                        0.2, 0.28, 0.42,
                    ))),
                    ..Default::default()
                })
            } else {
                row_widget
            };
            items_col = items_col.push(row_widget);
        }

        column![input_row, separator, items_col].into()
    }
}
