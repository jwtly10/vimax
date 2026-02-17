use iced::widget::{Space, column, container, row, text};
use iced::{Element, Length, Theme};
use tracing::debug;

use crate::app::Message;

const MAX_VISIBLE: usize = 8;
const POPUP_WIDTH: f32 = 340.0;
const ROW_HEIGHT: f32 = 22.0;

#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub kind: Option<CompletionKind>,
    pub detail: Option<String>,
    pub filter_text: String,
    pub insert_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Function,
    Variable,
    Field,
    Type,
    Module,
    Keyword,
    Snippet,
    File,
    Constant,
    Other,
}

impl CompletionKind {
    pub fn badge(&self) -> &'static str {
        match self {
            CompletionKind::Function => "fn",
            CompletionKind::Variable => "var",
            CompletionKind::Field => "fd",
            CompletionKind::Type => "ty",
            CompletionKind::Module => "mod",
            CompletionKind::Keyword => "kw",
            CompletionKind::Snippet => "sn",
            CompletionKind::File => "fi",
            CompletionKind::Constant => "ct",
            CompletionKind::Other => "  ",
        }
    }

    pub fn color(&self) -> iced::Color {
        match self {
            CompletionKind::Function => iced::Color::from_rgb(0.4, 0.8, 0.4),
            CompletionKind::Variable => iced::Color::from_rgb(0.4, 0.6, 1.0),
            CompletionKind::Field => iced::Color::from_rgb(0.4, 0.7, 0.9),
            CompletionKind::Type => iced::Color::from_rgb(0.7, 0.5, 0.9),
            CompletionKind::Module => iced::Color::from_rgb(0.9, 0.6, 0.3),
            CompletionKind::Keyword => iced::Color::from_rgb(0.9, 0.8, 0.3),
            CompletionKind::Snippet => iced::Color::from_rgb(0.6, 0.6, 0.6),
            CompletionKind::File => iced::Color::from_rgb(0.6, 0.8, 0.6),
            CompletionKind::Constant => iced::Color::from_rgb(0.4, 0.8, 0.8),
            CompletionKind::Other => iced::Color::from_rgb(0.5, 0.5, 0.5),
        }
    }

    fn from_lsp(kind: lsp_types::CompletionItemKind) -> Self {
        use lsp_types::CompletionItemKind;
        match kind {
            CompletionItemKind::FUNCTION | CompletionItemKind::METHOD => CompletionKind::Function,
            CompletionItemKind::VARIABLE => CompletionKind::Variable,
            CompletionItemKind::FIELD | CompletionItemKind::PROPERTY => CompletionKind::Field,
            CompletionItemKind::CLASS
            | CompletionItemKind::STRUCT
            | CompletionItemKind::INTERFACE
            | CompletionItemKind::ENUM
            | CompletionItemKind::TYPE_PARAMETER => CompletionKind::Type,
            CompletionItemKind::MODULE => CompletionKind::Module,
            CompletionItemKind::KEYWORD => CompletionKind::Keyword,
            CompletionItemKind::SNIPPET => CompletionKind::Snippet,
            CompletionItemKind::FILE => CompletionKind::File,
            CompletionItemKind::CONSTANT | CompletionItemKind::ENUM_MEMBER => {
                CompletionKind::Constant
            }
            _ => CompletionKind::Other,
        }
    }
}

pub struct CompletionState {
    items: Vec<CompletionItem>,
    filtered: Vec<usize>,
    selected: usize,
    scroll_offset: usize,
    prefix: String,
    #[allow(dead_code)]
    trigger_offset: usize,
}

pub enum CompletionEvent {
    Accept {
        insert_text: String,
        prefix_len: usize,
    },
    Dismiss,
    Noop,
}

impl CompletionState {
    pub fn new(items: Vec<CompletionItem>, trigger_offset: usize, prefix: &str) -> Self {
        let mut state = Self {
            items,
            filtered: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            prefix: prefix.to_string(),
            trigger_offset,
        };
        state.apply_filter();
        state
    }

    #[allow(dead_code)]
    pub fn trigger_offset(&self) -> usize {
        self.trigger_offset
    }

    pub fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }

    pub fn visible_count(&self) -> usize {
        self.filtered.len().min(MAX_VISIBLE)
    }

    pub fn push_char(&mut self, ch: char) {
        self.prefix.push(ch);
        self.apply_filter();
    }

    pub fn pop_char(&mut self) -> bool {
        if self.prefix.pop().is_some() {
            self.apply_filter();
            true
        } else {
            false
        }
    }

    pub fn prefix_len(&self) -> usize {
        self.prefix.len()
    }

    fn apply_filter(&mut self) {
        let prefix_lower = self.prefix.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if prefix_lower.is_empty() {
                    return true;
                }
                item.filter_text.to_lowercase().contains(&prefix_lower)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(i, _)| i)
            .collect();

        // Sort: prefix matches first, then alphabetical
        let prefix_lower_clone = prefix_lower.clone();
        self.filtered.sort_by(|&a, &b| {
            let a_item = &self.items[a];
            let b_item = &self.items[b];
            let a_starts = a_item
                .filter_text
                .to_lowercase()
                .starts_with(&prefix_lower_clone);
            let b_starts = b_item
                .filter_text
                .to_lowercase()
                .starts_with(&prefix_lower_clone);
            b_starts
                .cmp(&a_starts)
                .then(a_item.label.cmp(&b_item.label))
        });

        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn move_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
        self.ensure_selected_visible();
    }

    pub fn move_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.filtered.len() - 1;
        } else {
            self.selected -= 1;
        }
        self.ensure_selected_visible();
    }

    fn ensure_selected_visible(&mut self) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + MAX_VISIBLE {
            self.scroll_offset = self.selected + 1 - MAX_VISIBLE;
        }
    }

    pub fn accept_selected(&self) -> Option<CompletionEvent> {
        let &idx = self.filtered.get(self.selected)?;
        let item = &self.items[idx];
        Some(CompletionEvent::Accept {
            insert_text: item.insert_text.clone(),
            prefix_len: self.prefix.len(),
        })
    }

    pub fn handle_key(
        &mut self,
        key: &iced::keyboard::Key,
        modifiers: &iced::keyboard::Modifiers,
    ) -> CompletionEvent {
        use iced::keyboard::Key;

        match key {
            Key::Named(iced::keyboard::key::Named::Escape) => CompletionEvent::Dismiss,
            Key::Named(iced::keyboard::key::Named::Enter)
            | Key::Named(iced::keyboard::key::Named::Tab) => {
                self.accept_selected().unwrap_or(CompletionEvent::Dismiss)
            }
            Key::Named(iced::keyboard::key::Named::ArrowDown) => {
                self.move_down();
                CompletionEvent::Noop
            }
            Key::Named(iced::keyboard::key::Named::ArrowUp) => {
                self.move_up();
                CompletionEvent::Noop
            }
            Key::Character(ch) if modifiers.control() => match ch.as_str() {
                "n" => {
                    self.move_down();
                    CompletionEvent::Noop
                }
                "p" => {
                    self.move_up();
                    CompletionEvent::Noop
                }
                _ => CompletionEvent::Noop,
            },
            _ => CompletionEvent::Noop,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let visible_end = (self.scroll_offset + MAX_VISIBLE).min(self.filtered.len());
        let visible_range = self.scroll_offset..visible_end;

        let mut rows = column![].spacing(0);

        for (vi, &item_idx) in self.filtered[visible_range].iter().enumerate() {
            let item = &self.items[item_idx];
            let is_selected = vi + self.scroll_offset == self.selected;

            let badge_text = item.kind.as_ref().map(|k| k.badge()).unwrap_or("  ");
            let badge_color = item
                .kind
                .as_ref()
                .map(|k| k.color())
                .unwrap_or(iced::Color::from_rgb(0.5, 0.5, 0.5));

            let badge = text(format!(" {} ", badge_text))
                .size(13)
                .color(badge_color);

            let label = text(&item.label)
                .size(14)
                .color(iced::Color::from_rgb(0.85, 0.85, 0.9));

            let mut item_row = row![badge, label].align_y(iced::Alignment::Center);

            if let Some(detail) = &item.detail {
                item_row = item_row.push(Space::new().width(Length::Fill));
                let detail_display = if detail.len() > 30 {
                    format!("{}...", &detail[..27])
                } else {
                    detail.clone()
                };
                item_row = item_row.push(
                    text(format!(" {} ", detail_display))
                        .size(12)
                        .color(iced::Color::from_rgb(0.5, 0.5, 0.55)),
                );
            }

            let bg = if is_selected {
                iced::Color::from_rgb(0.18, 0.26, 0.4)
            } else {
                iced::Color::from_rgb(0.12, 0.12, 0.16)
            };

            let row_container = container(item_row)
                .width(Length::Fill)
                .height(Length::Fixed(ROW_HEIGHT))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(bg)),
                    ..Default::default()
                })
                .padding(iced::Padding {
                    top: 2.0,
                    bottom: 2.0,
                    left: 2.0,
                    right: 4.0,
                });

            rows = rows.push(row_container);
        }

        let border_color = iced::Color::from_rgb(0.3, 0.35, 0.45);
        container(rows)
            .width(Length::Fixed(POPUP_WIDTH))
            .style(move |_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.12, 0.12, 0.16,
                ))),
                border: iced::Border {
                    width: 1.0,
                    color: border_color,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .into()
    }
}

pub fn parse_lsp_response(
    result: &serde_json::Value,
    trigger_offset: usize,
    prefix: &str,
) -> Option<CompletionState> {
    let items = if result.is_array() {
        serde_json::from_value::<Vec<lsp_types::CompletionItem>>(result.clone()).ok()?
    } else if let Some(items_val) = result.get("items") {
        serde_json::from_value::<Vec<lsp_types::CompletionItem>>(items_val.clone()).ok()?
    } else {
        return None;
    };

    debug!(count = items.len(), "parsed LSP completion items");

    if items.is_empty() {
        return None;
    }

    let completion_items: Vec<CompletionItem> = items
        .into_iter()
        .map(|lsp_item| {
            let kind = lsp_item.kind.map(CompletionKind::from_lsp);
            let filter_text = lsp_item
                .filter_text
                .clone()
                .unwrap_or_else(|| lsp_item.label.clone());
            let insert_text = lsp_item
                .insert_text
                .clone()
                .unwrap_or_else(|| lsp_item.label.clone());
            let insert_text = strip_snippet_placeholders(&insert_text);
            CompletionItem {
                label: lsp_item.label,
                kind,
                detail: lsp_item.detail,
                filter_text,
                insert_text,
            }
        })
        .collect();

    let state = CompletionState::new(completion_items, trigger_offset, prefix);
    if state.is_empty() { None } else { Some(state) }
}

/// Strip basic snippet placeholders: $0, $1, ${1:text} → text
#[allow(clippy::while_let_on_iterator)]
fn strip_snippet_placeholders(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            if let Some(&'{') = chars.peek() {
                chars.next(); // consume '{'
                // Skip digits and optional colon
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Some(&':') = chars.peek() {
                    chars.next(); // consume ':'
                    // Collect default text until '}'
                    let mut depth = 1;
                    while let Some(inner) = chars.next() {
                        if inner == '{' {
                            depth += 1;
                            result.push(inner);
                        } else if inner == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            result.push(inner);
                        } else {
                            result.push(inner);
                        }
                    }
                } else {
                    // ${N} — skip until }
                    while let Some(inner) = chars.next() {
                        if inner == '}' {
                            break;
                        }
                    }
                }
            } else if let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    chars.next(); // skip $N
                } else {
                    result.push(c);
                }
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}
