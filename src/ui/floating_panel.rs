use crate::action::{EditorAction, Motion, Range};
use crate::app::Message;
use crate::buffer::Buffer;
use crate::text_grid;
use crate::vim::mode::VimMode;
use crate::vim::VimLayer;
use crate::window::Window;

use iced::keyboard;
use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length, Theme};

/// Sentinel window_id used for message routing to the panel's internal window.
pub const PANEL_WINDOW_ID: usize = usize::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    LspHover,
    Diagnostic,
}

#[derive(Debug, Clone, Copy)]
pub enum PanelPosition {
    /// Anchored near cursor (default for hover/diagnostics)
    AtCursor {
        line: usize,
        col: usize,
        scroll_y: usize,
        scroll_x: usize,
    },
}

pub enum PanelEvent {
    Dismiss,
    YankToRegister(String),
    Noop,
}

// ---------------------------------------------------------------------------
// PanelBuffer — read-only navigable buffer with its own VimLayer
// ---------------------------------------------------------------------------

pub struct PanelBuffer {
    pub buffer: Buffer,
    pub window: Window,
    pub vim: VimLayer,
    pub search_pattern: String,
}

impl PanelBuffer {
    pub fn new(content: &str, name: &str) -> Self {
        let buffer = Buffer::new_readonly(content, name);
        let window = Window::new(0);
        let vim = VimLayer::new();
        Self {
            buffer,
            window,
            vim,
            search_pattern: String::new(),
        }
    }

    pub fn handle_key(
        &mut self,
        key: &keyboard::Key,
        modified_key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        text: Option<&str>,
    ) -> PanelEvent {
        // Escape always dismisses, regardless of mode
        if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape))
            && self.vim.mode() == VimMode::Normal
        {
            return PanelEvent::Dismiss;
        }

        // 'q' in Normal mode dismisses
        if self.vim.mode() == VimMode::Normal
            && let keyboard::Key::Character(c) = key
            && c.as_str() == "q"
            && !modifiers.control()
        {
            return PanelEvent::Dismiss;
        }

        let actions =
            self.vim
                .handle_key(key, modified_key, modifiers, text, &self.buffer, self.window.cursor);

        // Block insert and command modes — force back to Normal
        if matches!(self.vim.mode(), VimMode::Insert | VimMode::Command) {
            self.vim.force_mode(VimMode::Normal);
        }

        let mut event = PanelEvent::Noop;
        for action in actions {
            if let PanelEvent::YankToRegister(t) = self.execute_action(action) {
                event = PanelEvent::YankToRegister(t);
            }
        }
        event
    }

    fn execute_action(&mut self, action: EditorAction) -> PanelEvent {
        match action {
            EditorAction::MoveCursor { motion, count } => {
                self.move_cursor(&motion, count);
            }
            EditorAction::SetCursor(pos) => {
                self.window.cursor = self.buffer.clamp_cursor(pos);
            }
            EditorAction::SetSearchPattern(pattern) => {
                self.search_pattern = pattern;
            }
            EditorAction::SearchNext { count } => {
                self.search_next(count);
            }
            EditorAction::SearchPrev { count } => {
                self.search_prev(count);
            }
            EditorAction::ClearSearch => {
                self.search_pattern.clear();
                self.window.search_matches.clear();
                self.window.search_cached_pattern.clear();
            }
            EditorAction::SetSelection(sel) => {
                self.window.selection = sel;
            }
            EditorAction::UpdateVisualSelection { anchor, mode } => {
                let cursor = self.window.cursor;
                let selection = if mode == VimMode::VisualLine {
                    let anchor_line = self.buffer.char_to_line(anchor);
                    let cursor_line = self.buffer.char_to_line(cursor);
                    let (start_line, end_line) = if anchor_line <= cursor_line {
                        (anchor_line, cursor_line)
                    } else {
                        (cursor_line, anchor_line)
                    };
                    let start = self.buffer.line_to_char(start_line);
                    let end = if end_line + 1 < self.buffer.total_lines() {
                        self.buffer.line_to_char(end_line + 1)
                    } else {
                        self.buffer.len_chars()
                    };
                    (start, end)
                } else if anchor <= cursor {
                    (anchor, cursor + 1)
                } else {
                    (cursor, anchor + 1)
                };
                self.window.selection = Some(selection);
            }
            EditorAction::YankRange(Range { start, end }) => {
                let yanked = self.buffer.yank_range(start, end);
                return PanelEvent::YankToRegister(yanked);
            }
            EditorAction::SetMode(_) => {
                // Already handled by vim layer
            }
            // All other actions silently ignored (no editing in read-only panel)
            _ => {}
        }
        self.window.ensure_cursor_visible(&self.buffer);
        PanelEvent::Noop
    }

    fn move_cursor(&mut self, motion: &Motion, count: usize) {
        let cursor = self.window.cursor;
        let new_cursor = match motion {
            Motion::Left => {
                let mut c = cursor;
                for _ in 0..count {
                    c = self.buffer.move_left(c);
                }
                c
            }
            Motion::Right => {
                let mut c = cursor;
                for _ in 0..count {
                    c = self.buffer.move_right(c);
                }
                c
            }
            Motion::Up => {
                let mut c = cursor;
                for _ in 0..count {
                    c = self.buffer.move_up(c);
                }
                c
            }
            Motion::Down => {
                let mut c = cursor;
                for _ in 0..count {
                    c = self.buffer.move_down(c);
                }
                c
            }
            Motion::WordForward => {
                let mut c = cursor;
                for _ in 0..count {
                    c = self.buffer.move_word_forward(c);
                }
                c
            }
            Motion::WordBackward => {
                let mut c = cursor;
                for _ in 0..count {
                    c = self.buffer.move_word_backward(c);
                }
                c
            }
            Motion::WordEnd => {
                let mut c = cursor;
                for _ in 0..count {
                    c = self.buffer.move_word_end(c);
                }
                c
            }
            Motion::LineStart => self.buffer.move_to_line_start(cursor),
            Motion::LineEnd => self.buffer.move_to_line_end(cursor),
            Motion::FirstNonWhitespace => self.buffer.move_to_first_non_whitespace(cursor),
            Motion::FileStart => self.buffer.move_to_start(),
            Motion::FileEnd => self.buffer.move_to_end(),
            Motion::FindChar {
                ch,
                forward,
                stop_before,
            } => {
                let mut c = cursor;
                for _ in 0..count {
                    c = self.buffer.find_char_on_line(c, *ch, *forward, *stop_before);
                }
                c
            }
            Motion::HalfPageDown => {
                let half = (self.window.visible_lines / 2).max(1) * count;
                let mut c = cursor;
                for _ in 0..half {
                    c = self.buffer.move_down(c);
                }
                c
            }
            Motion::HalfPageUp => {
                let half = (self.window.visible_lines / 2).max(1) * count;
                let mut c = cursor;
                for _ in 0..half {
                    c = self.buffer.move_up(c);
                }
                c
            }
        };
        self.window.cursor = new_cursor;
        self.window.ensure_cursor_visible(&self.buffer);
    }

    fn search_next(&mut self, count: usize) {
        if self.search_pattern.is_empty() {
            return;
        }
        self.window
            .update_search_cache(&self.buffer, &self.search_pattern);
        if self.window.search_matches.is_empty() {
            return;
        }
        let mut cursor = self.window.cursor;
        for _ in 0..count {
            let next = self
                .window
                .search_matches
                .iter()
                .find(|&&m| m > cursor)
                .or(self.window.search_matches.first());
            if let Some(&pos) = next {
                cursor = pos;
            }
        }
        self.window.cursor = cursor;
        self.window.ensure_cursor_visible(&self.buffer);
    }

    fn search_prev(&mut self, count: usize) {
        if self.search_pattern.is_empty() {
            return;
        }
        self.window
            .update_search_cache(&self.buffer, &self.search_pattern);
        if self.window.search_matches.is_empty() {
            return;
        }
        let mut cursor = self.window.cursor;
        for _ in 0..count {
            let prev = self
                .window
                .search_matches
                .iter()
                .rev()
                .find(|&&m| m < cursor)
                .or(self.window.search_matches.last());
            if let Some(&pos) = prev {
                cursor = pos;
            }
        }
        self.window.cursor = cursor;
        self.window.ensure_cursor_visible(&self.buffer);
    }
}

// ---------------------------------------------------------------------------
// FloatingPanel — positioned, titled wrapper around PanelBuffer
// ---------------------------------------------------------------------------

pub struct FloatingPanel {
    pub panel_buffer: PanelBuffer,
    pub title: String,
    pub kind: PanelKind,
    pub position: PanelPosition,
    pub focused: bool,
}

impl FloatingPanel {
    pub fn new(content: &str, title: &str, kind: PanelKind, position: PanelPosition) -> Self {
        Self {
            panel_buffer: PanelBuffer::new(content, title),
            title: title.to_string(),
            kind,
            position,
            focused: false,
        }
    }

    pub fn focus(&mut self) {
        self.focused = true;
    }

    pub fn view(
        &self,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Element<'_, Message> {
        let PanelPosition::AtCursor {
            line,
            col,
            scroll_y,
            scroll_x,
        } = self.position;

        // Panel dimensions
        let max_panel_width = (viewport_width * 0.6).min(700.0);
        let content_lines = self.panel_buffer.buffer.total_lines();
        let max_panel_height = viewport_height * 0.4;
        let panel_height = (content_lines as f32 * text_grid::LINE_HEIGHT)
            .min(max_panel_height)
            .max(text_grid::LINE_HEIGHT * 2.0);

        // Title bar
        let title_height: f32 = 24.0;
        let focus_hint = if self.focused { "" } else { " (press again to focus)" };
        let title_bar = container(
            text(format!(" {}{}", self.title, focus_hint))
                .size(12)
                .color(iced::Color::from_rgb(0.7, 0.7, 0.8)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(title_height))
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgb(
                0.12, 0.12, 0.17,
            ))),
            ..Default::default()
        });

        // Text grid content
        let grid = text_grid::text_grid(
            &self.panel_buffer.buffer,
            self.panel_buffer.window.cursor,
            self.panel_buffer.window.scroll_y,
            self.panel_buffer.window.scroll_x,
            self.panel_buffer.window.selection,
            &self.panel_buffer.window.search_matches,
            self.panel_buffer.search_pattern.len(),
            PANEL_WINDOW_ID,
            self.focused,
            Vec::new(),
            &[],
            false,
        );

        let content = container(grid)
            .width(Length::Fill)
            .height(Length::Fixed(panel_height));

        let border_color = if self.focused {
            iced::Color::from_rgb(0.4, 0.5, 0.7)
        } else {
            iced::Color::from_rgb(0.3, 0.35, 0.45)
        };

        let panel = container(column![title_bar, content])
            .width(Length::Fixed(max_panel_width))
            .height(Length::Fixed(panel_height + title_height))
            .style(move |_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.1, 0.1, 0.14,
                ))),
                border: iced::Border {
                    width: 1.0,
                    color: border_color,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

        // Position relative to cursor
        let pixel_x = text_grid::GUTTER_WIDTH
            + 8.0
            + (col as isize - scroll_x as isize).max(0) as f32 * text_grid::CHAR_WIDTH;
        let pixel_y =
            ((line as isize - scroll_y as isize + 1).max(0) as f32) * text_grid::LINE_HEIGHT;

        // Flip above cursor if not enough room below
        let total_height = panel_height + title_height;
        let final_y = if pixel_y + total_height > viewport_height {
            ((line as isize - scroll_y as isize) as f32 * text_grid::LINE_HEIGHT - total_height)
                .max(0.0)
        } else {
            pixel_y
        };

        container(column![
            Space::new().height(Length::Fixed(final_y)),
            row![Space::new().width(Length::Fixed(pixel_x)), panel]
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
