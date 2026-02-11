use tracing::{error, info};

use crate::action::{BufferQuery, EditorAction, EditorEffect, Motion, Range};
use crate::buffer::Buffer;
use crate::registers::Registers;
use crate::vim::mode::VimMode;
use crate::viewport::Viewport;

pub struct Editor {
    pub buffers: Vec<Buffer>,
    pub active_buffer: usize,
    pub viewport: Viewport,
    pub registers: Registers,
    pub mode_display: String,
    pub status_message: String,
    pub selection: Option<(usize, usize)>,
    pub search_pattern: String,
    search_matches: Vec<usize>,
    search_version: u64,
    search_cached_pattern: String,
}

impl Editor {
    pub fn new(buffer: Buffer) -> Self {
        Self {
            buffers: vec![buffer],
            active_buffer: 0,
            viewport: Viewport::new(),
            registers: Registers::new(),
            mode_display: String::from("NORMAL"),
            status_message: String::new(),
            selection: None,
            search_pattern: String::new(),
            search_matches: Vec::new(),
            search_version: u64::MAX,
            search_cached_pattern: String::new(),
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffers[self.active_buffer]
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.active_buffer]
    }

    pub fn ensure_cursor_visible(&mut self) {
        let (cursor_line, cursor_col) = self.buffer().cursor_position();
        let total = self.buffer().total_lines();
        self.viewport
            .ensure_cursor_visible(cursor_line, cursor_col, total);
    }

    pub fn update_search_cache(&mut self) {
        let version = self.buffer().version();
        if self.search_pattern == self.search_cached_pattern
            && version == self.search_version
        {
            return;
        }
        self.search_matches = self.buffer().find_all(&self.search_pattern);
        self.search_cached_pattern = self.search_pattern.clone();
        self.search_version = version;
    }

    pub fn search_matches(&self) -> &[usize] {
        &self.search_matches
    }

    pub fn search_len(&self) -> usize {
        self.search_pattern.len()
    }

    pub fn execute(&mut self, action: EditorAction) -> EditorEffect {
        match action {
            EditorAction::MoveCursor { motion, count } => {
                self.move_cursor(&motion, count);
            }
            EditorAction::SetCursor(pos) => {
                self.buffer_mut().set_cursor(pos);
            }
            EditorAction::InsertChar(ch) => {
                self.buffer_mut().insert_char(ch);
            }
            EditorAction::InsertNewline => {
                self.buffer_mut().insert_char('\n');
            }
            EditorAction::InsertTab => {
                self.buffer_mut().insert_str("    ");
            }
            EditorAction::DeleteCharForward { count } => {
                for _ in 0..count {
                    self.buffer_mut().delete_char_forward();
                }
            }
            EditorAction::DeleteCharBackward => {
                self.buffer_mut().delete_char_backward();
            }
            EditorAction::DeleteLine { count } => {
                for _ in 0..count {
                    self.buffer_mut().delete_line();
                }
            }
            EditorAction::DeleteRange(Range { start, end }) => {
                let deleted = self.buffer_mut().delete_range(start, end);
                self.registers.unnamed = deleted;
            }
            EditorAction::ChangeRange(Range { start, end }) => {
                self.buffer_mut().start_edit_group();
                let deleted = self.buffer_mut().delete_range(start, end);
                self.registers.unnamed = deleted;
            }
            EditorAction::YankRange(Range { start, end }) => {
                let text = self.buffer().yank_range(start, end);
                self.registers.unnamed = text;
            }
            EditorAction::ReplaceChar(ch) => {
                self.buffer_mut().replace_char(ch);
            }
            EditorAction::Paste { before } => {
                let text = self.registers.unnamed.clone();
                if before {
                    self.buffer_mut().paste_before(&text);
                } else {
                    self.buffer_mut().paste_after(&text);
                }
            }
            EditorAction::Undo => {
                self.buffer_mut().undo();
            }
            EditorAction::Redo => {
                self.buffer_mut().redo();
            }
            EditorAction::StartEditGroup => {
                self.buffer_mut().start_edit_group();
            }
            EditorAction::FinishEditGroup => {
                self.buffer_mut().finish_edit_group();
            }
            EditorAction::SetSearchPattern(pattern) => {
                self.search_pattern = pattern;
            }
            EditorAction::SearchNext { count } => {
                if self.search_pattern.is_empty() {
                    self.status_message = String::from("No search pattern");
                    return EditorEffect::None;
                }
                self.update_search_cache();
                if self.search_matches.is_empty() {
                    self.status_message =
                        format!("/{} [0/0]", self.search_pattern);
                    return EditorEffect::None;
                }
                let cursor = self.buffer().cursor();
                for _ in 0..count {
                    let cur = self.buffer().cursor();
                    let next = self
                        .search_matches
                        .iter()
                        .find(|&&m| m > cur)
                        .or(self.search_matches.first());
                    if let Some(&pos) = next {
                        self.buffer_mut().set_cursor(pos);
                    }
                }
                let total = self.search_matches.len();
                let current = self
                    .search_matches
                    .iter()
                    .position(|&m| m == self.buffer().cursor())
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let wrapped = if self.buffer().cursor() <= cursor && count > 0 {
                    " [wrapped]"
                } else {
                    ""
                };
                self.status_message =
                    format!("/{} [{}/{}]{}", self.search_pattern, current, total, wrapped);
            }
            EditorAction::SearchPrev { count } => {
                if self.search_pattern.is_empty() {
                    self.status_message = String::from("No search pattern");
                    return EditorEffect::None;
                }
                self.update_search_cache();
                if self.search_matches.is_empty() {
                    self.status_message =
                        format!("?{} [0/0]", self.search_pattern);
                    return EditorEffect::None;
                }
                let cursor = self.buffer().cursor();
                for _ in 0..count {
                    let cur = self.buffer().cursor();
                    let prev = self
                        .search_matches
                        .iter()
                        .rev()
                        .find(|&&m| m < cur)
                        .or(self.search_matches.last());
                    if let Some(&pos) = prev {
                        self.buffer_mut().set_cursor(pos);
                    }
                }
                let total = self.search_matches.len();
                let current = self
                    .search_matches
                    .iter()
                    .position(|&m| m == self.buffer().cursor())
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let wrapped = if self.buffer().cursor() >= cursor && count > 0 {
                    " [wrapped]"
                } else {
                    ""
                };
                self.status_message =
                    format!("?{} [{}/{}]{}", self.search_pattern, current, total, wrapped);
            }
            EditorAction::ClearSearch => {
                self.search_pattern.clear();
                self.search_matches.clear();
                self.search_cached_pattern.clear();
                self.status_message.clear();
            }
            EditorAction::Save => {
                match self.buffer_mut().save() {
                    Ok(()) => {
                        info!("file saved");
                        self.status_message = String::from("Written");
                    }
                    Err(e) => {
                        error!(?e, "failed to save");
                        self.status_message = format!("Error: {}", e);
                    }
                }
            }
            EditorAction::Quit { force } => {
                if !force && self.buffer().is_modified() {
                    self.status_message = String::from(
                        "Unsaved changes! Use :q! to force quit, or :wq to save and quit",
                    );
                    return EditorEffect::None;
                }
                return EditorEffect::Task(iced::exit());
            }
            EditorAction::WriteQuit => {
                match self.buffer_mut().save() {
                    Ok(()) => return EditorEffect::Task(iced::exit()),
                    Err(e) => {
                        error!(?e, "failed to save");
                        self.status_message = format!("Error: {}", e);
                    }
                }
            }
            EditorAction::SetMode(mode) => {
                self.mode_display = mode;
            }
            EditorAction::SetStatusMessage(msg) => {
                self.status_message = msg;
            }
            EditorAction::SetSelection(sel) => {
                self.selection = sel;
            }
            EditorAction::UpdateVisualSelection { anchor, mode } => {
                let cursor = self.buffer().cursor();
                self.selection = Some(if mode == VimMode::VisualLine {
                    let anchor_line = self.buffer().char_to_line(anchor);
                    let cursor_line = self.buffer().char_to_line(cursor);
                    let (start_line, end_line) = if anchor_line <= cursor_line {
                        (anchor_line, cursor_line)
                    } else {
                        (cursor_line, anchor_line)
                    };
                    let start = self.buffer().line_to_char(start_line);
                    let end = if end_line + 1 < self.buffer().total_lines() {
                        self.buffer().line_to_char(end_line + 1)
                    } else {
                        self.buffer().len_chars()
                    };
                    (start, end)
                } else if anchor <= cursor {
                    (anchor, cursor + 1)
                } else {
                    (cursor, anchor + 1)
                });
            }
        }
        EditorEffect::None
    }

    fn move_cursor(&mut self, motion: &Motion, count: usize) {
        match motion {
            Motion::Left => {
                for _ in 0..count {
                    self.buffer_mut().move_left();
                }
            }
            Motion::Right => {
                for _ in 0..count {
                    self.buffer_mut().move_right();
                }
            }
            Motion::Up => {
                for _ in 0..count {
                    self.buffer_mut().move_up();
                }
            }
            Motion::Down => {
                for _ in 0..count {
                    self.buffer_mut().move_down();
                }
            }
            Motion::WordForward => {
                for _ in 0..count {
                    self.buffer_mut().move_word_forward();
                }
            }
            Motion::WordBackward => {
                for _ in 0..count {
                    self.buffer_mut().move_word_backward();
                }
            }
            Motion::WordEnd => {
                for _ in 0..count {
                    self.buffer_mut().move_word_end();
                }
            }
            Motion::LineStart => {
                self.buffer_mut().move_to_line_start();
            }
            Motion::LineEnd => {
                self.buffer_mut().move_to_line_end();
            }
            Motion::FirstNonWhitespace => {
                self.buffer_mut().move_to_first_non_whitespace();
            }
            Motion::FileStart => {
                self.buffer_mut().move_to_start();
            }
            Motion::FileEnd => {
                self.buffer_mut().move_to_end();
            }
            Motion::FindChar {
                ch,
                forward,
                stop_before,
            } => {
                for _ in 0..count {
                    self.buffer_mut().find_char_on_line(*ch, *forward, *stop_before);
                }
            }
            Motion::HalfPageDown => {
                let half = (self.viewport.visible_lines / 2).max(1) * count;
                for _ in 0..half {
                    self.buffer_mut().move_down();
                }
            }
            Motion::HalfPageUp => {
                let half = (self.viewport.visible_lines / 2).max(1) * count;
                for _ in 0..half {
                    self.buffer_mut().move_up();
                }
            }
        }
    }
}
