use iced::widget::{column, container, row};
use iced::{Element, Length, Theme};

use crate::action::EditorAction;
use crate::app::Message;
use crate::buffer::Buffer;
use crate::core_actions::{execute_core, search_next_in, search_prev_in};
use crate::diagnostics::{Diagnostic, Severity};
use crate::registers::Registers;
use crate::syntax::SyntaxState;
use crate::text_grid;
use crate::ui;
use crate::ui::scrollbar::ScrollbarMarker;
use crate::vim::mode::VimMode;
use crate::window::Window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoPanelKind {
    Diagnostic,
    LspHover,
}

pub struct InfoPanel {
    pub buffer: Buffer,
    pub window: Window,
    pub syntax_state: Option<SyntaxState>,
    pub pinned: bool,
    pub kind: InfoPanelKind,
    pub search_pattern: String,
    pub status_message: String,
}

const INFO_PANEL_WINDOW_ID: usize = 9999;

impl InfoPanel {
    pub fn new(content: &str, kind: InfoPanelKind) -> Self {
        let mut buffer = Buffer::from_str(content, "*info*", std::path::Path::new("*info*"), true);
        buffer.set_name(match kind {
            InfoPanelKind::Diagnostic => "*diagnostics*",
            InfoPanelKind::LspHover => "*hover*",
        });
        let window = Window::new(0);
        Self {
            buffer,
            window,
            syntax_state: None,
            pinned: false,
            kind,
            search_pattern: String::new(),
            status_message: String::new(),
        }
    }

    pub fn pin(&mut self) {
        self.pinned = true;
    }

    pub fn view(&self, is_focused: bool, mode: VimMode) -> Element<'_, Message> {
        let highlights = if let Some(state) = &self.syntax_state {
            let rope = self.buffer.rope();
            let start_byte = rope.line_to_byte(self.window.scroll_y) as u32;
            let end_line =
                (self.window.scroll_y + self.window.visible_lines + 2).min(rope.len_lines());
            let end_byte = if end_line < rope.len_lines() {
                rope.line_to_byte(end_line) as u32
            } else {
                rope.len_bytes() as u32
            };
            state.highlights_for_range(
                rope,
                &crate::syntax::loader::Loader::new(),
                start_byte,
                end_byte,
            )
        } else {
            Vec::new()
        };

        let grid = text_grid::text_grid(
            &self.buffer,
            self.window.cursor,
            self.window.scroll_y,
            self.window.scroll_x,
            self.window.selection,
            &self.window.search_matches,
            0,
            INFO_PANEL_WINDOW_ID,
            is_focused,
            highlights,
            &[],
            mode,
        );

        let markers = self.build_scrollbar_markers();
        let sb = ui::scrollbar::scrollbar(
            self.window.scroll_y,
            self.window.visible_lines,
            self.buffer.total_lines(),
            markers,
            INFO_PANEL_WINDOW_ID,
            is_focused,
        );

        let separator_color = iced::Color::from_rgb(0.3, 0.3, 0.35);
        let sep = container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(move |_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(separator_color)),
                ..Default::default()
            });

        let panel_content = row![container(grid).width(Length::Fill).height(Length::Fill), sb,];

        column![sep, panel_content]
            .height(Length::FillPortion(3))
            .into()
    }

    fn build_scrollbar_markers(&self) -> Vec<ScrollbarMarker> {
        Vec::new()
    }

    pub fn scroll_lines(&mut self, delta: f32, speed: f32) {
        let total = self.buffer.total_lines();
        self.window.scroll_lines(delta, speed, total);
    }

    pub fn scroll_to(&mut self, line: usize) {
        let total = self.buffer.total_lines();
        let max_scroll = total.saturating_sub(1);
        self.window.scroll_y = line.min(max_scroll);
    }

    pub fn execute_actions(&mut self, actions: &[EditorAction], registers: &mut Registers) {
        for action in actions {
            if execute_core(
                &mut self.window,
                &mut self.buffer,
                registers,
                &mut self.status_message,
                action,
            ) {
                continue;
            }
            // Some actions are local specific in this info_panel buffer impl
            match action {
                EditorAction::SetSearchPattern(pattern) => {
                    self.search_pattern = pattern.clone();
                    self.window
                        .update_search_cache(&self.buffer, &self.search_pattern);
                }
                EditorAction::SearchNext { count } => {
                    self.window
                        .update_search_cache(&self.buffer, &self.search_pattern);
                    search_next_in(&mut self.window, &self.search_pattern, *count);
                }
                EditorAction::SearchPrev { count } => {
                    self.window
                        .update_search_cache(&self.buffer, &self.search_pattern);
                    search_prev_in(&mut self.window, &self.search_pattern, *count);
                }
                EditorAction::ClearSearch => {
                    self.search_pattern.clear();
                    self.window.search_matches.clear();
                    self.window.search_cached_pattern.clear();
                }
                _ => {}
            }
        }
        self.window.ensure_cursor_visible(&self.buffer);
    }

    pub fn resize(&mut self, lines: usize, cols: usize) {
        self.window.visible_lines = lines;
        self.window.visible_cols = cols;
    }
}

pub fn format_diagnostics(diagnostics: &[Diagnostic]) -> String {
    let mut lines = Vec::new();
    for diag in diagnostics {
        let severity_label = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Hint => "hint",
        };
        let source = diag.source.as_deref().unwrap_or("unknown");
        lines.push(format!("[{}] {}", severity_label, source));
        lines.push(diag.message.clone());
        lines.push(String::new());
    }
    if lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

pub fn parse_hover_response(result: &serde_json::Value) -> Option<String> {
    let hover: lsp_types::Hover = serde_json::from_value(result.clone()).ok()?;
    let text = match hover.contents {
        lsp_types::HoverContents::Scalar(marked) => extract_marked_string(marked),
        lsp_types::HoverContents::Array(items) => items
            .into_iter()
            .map(extract_marked_string)
            .collect::<Vec<_>>()
            .join("\n\n"),
        lsp_types::HoverContents::Markup(markup) => markup.value,
    };
    if text.is_empty() { None } else { Some(text) }
}

fn extract_marked_string(ms: lsp_types::MarkedString) -> String {
    match ms {
        lsp_types::MarkedString::String(s) => s,
        lsp_types::MarkedString::LanguageString(ls) => ls.value,
    }
}
