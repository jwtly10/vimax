use std::path::{Path, PathBuf};

use lsp_types::notification::DidOpenTextDocument;
use lsp_types::request::GotoDefinition;
use lsp_types::{
    DidOpenTextDocumentParams, GotoDefinitionParams, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams,
};
use tracing::{debug, error, info};

use crate::action::{EditorAction, EditorEffect, Motion, Range};
use crate::buffer::Buffer;
use crate::layout::SplitDirection;
use crate::lsp::{offset_to_lsp_position, path_to_uri};
use crate::registers::Registers;
use crate::syntax::SyntaxState;
use crate::syntax::loader::Loader;
use crate::vim::mode::VimMode;
use crate::window::Window;
use crate::workspace::Workspace;

pub struct Editor {
    pub buffers: Vec<Buffer>,
    pub syntax_states: Vec<Option<SyntaxState>>,
    pub loader: Loader,
    pub workspaces: Vec<Workspace>,
    pub active_workspace: usize,
    pub registers: Registers,
    pub search_pattern: String,
    pub mode_display: String,
    pub status_message: String,
    jump_list: JumpList,
}

pub struct JumpList {
    entries: Vec<(usize, usize)>,
    pos: usize,
}

impl JumpList {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            pos: 0,
        }
    }

    fn push(&mut self, buffer_id: usize, cursor: usize) {
        self.entries.truncate(self.pos);
        self.entries.push((buffer_id, cursor));
        self.pos = self.entries.len();
    }

    fn backward(&mut self, current_buf: usize, current_cursor: usize) -> Option<(usize, usize)> {
        if self.pos == self.entries.len() && !self.entries.is_empty() {
            self.entries.push((current_buf, current_cursor));
        }
        if self.pos > 0 {
            self.pos -= 1;
            Some(self.entries[self.pos])
        } else {
            None
        }
    }

    fn forward(&mut self) -> Option<(usize, usize)> {
        if self.pos + 1 < self.entries.len() {
            self.pos += 1;
            Some(self.entries[self.pos])
        } else {
            None
        }
    }
}

impl Editor {
    pub fn new(cwd: PathBuf) -> Self {
        let workspace = Workspace::new(0, cwd);
        let loader = Loader::new();

        Self {
            buffers: Vec::new(),
            syntax_states: Vec::new(),
            loader,
            workspaces: vec![workspace],
            active_workspace: 0,
            registers: Registers::new(),
            search_pattern: String::new(),
            mode_display: String::from("NORMAL"),
            status_message: String::new(),
            jump_list: JumpList::new(),
        }
    }

    fn create_syntax_for_buffer(buffer: &Buffer, loader: &Loader) -> Option<SyntaxState> {
        let ext = buffer
            .file_path()
            .and_then(|p| std::path::Path::new(p).extension())
            .and_then(|e| e.to_str());

        if let Some(ext) = ext
            && let Some(lang) = loader.language_for_extension(ext)
        {
            return SyntaxState::new(buffer.rope(), lang, loader);
        }
        None
    }

    pub fn ensure_syntax_current(&mut self) {
        let ws = &self.workspaces[self.active_workspace];
        for win in &ws.windows {
            let buf_id = win.buffer_id;
            if buf_id < self.syntax_states.len()
                && let Some(state) = &mut self.syntax_states[buf_id]
            {
                let version = self.buffers[buf_id].version();
                state.ensure_parsed(self.buffers[buf_id].rope(), version, &self.loader);
            }
        }
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspaces[self.active_workspace]
    }

    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace]
    }

    pub fn window_mut(&mut self) -> &mut Window {
        self.workspaces[self.active_workspace].window_mut()
    }

    pub fn buffer(&self) -> &Buffer {
        let buf_id = self.workspace().window().buffer_id;
        &self.buffers[buf_id]
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        let buf_id = self.workspace().window().buffer_id;
        &mut self.buffers[buf_id]
    }

    pub fn cursor(&self) -> usize {
        self.workspace().cursor()
    }

    fn push_jump(&mut self) {
        let buf_id = self.workspace().window().buffer_id;
        let cursor = self.cursor();
        self.jump_list.push(buf_id, cursor);
    }

    fn jump_backward(&mut self) {
        let cur_buf = self.workspace().window().buffer_id;
        let cur_cursor = self.cursor();
        if let Some((buf_id, cursor)) = self.jump_list.backward(cur_buf, cur_cursor) {
            if buf_id < self.buffers.len() {
                let len_chars = self.buffers[buf_id].len_chars();
                self.workspace_mut().switch_buffer(buf_id, len_chars);
                self.window_mut().cursor = self.buffers[buf_id].clamp_cursor(cursor);
            }
        }
    }

    fn jump_forward(&mut self) {
        if let Some((buf_id, cursor)) = self.jump_list.forward() {
            if buf_id < self.buffers.len() {
                let len_chars = self.buffers[buf_id].len_chars();
                self.workspace_mut().switch_buffer(buf_id, len_chars);
                self.window_mut().cursor = self.buffers[buf_id].clamp_cursor(cursor);
            }
        }
    }

    pub fn ensure_cursor_visible(&mut self) {
        let win = self.workspaces[self.active_workspace].window_mut();
        let buf_id = win.buffer_id;
        win.ensure_cursor_visible(&self.buffers[buf_id]);
    }

    pub fn update_search_cache(&mut self) {
        let win = self.workspaces[self.active_workspace].window_mut();
        let buf_id = win.buffer_id;
        let pattern = self.search_pattern.clone();
        win.update_search_cache(&self.buffers[buf_id], &pattern);
    }

    pub fn search_len(&self) -> usize {
        self.search_pattern.len()
    }

    pub fn open_file(&mut self, path: &Path) {
        let path_str = path.to_string_lossy().to_string();
        if let Some(idx) = self
            .buffers
            .iter()
            .position(|b| b.file_path() == Some(&path_str))
        {
            self.workspace_mut().reset_window_for_buffer(idx);
            self.status_message = format!("\"{}\"", path.display());
            return;
        }

        let buf_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        match std::fs::read_to_string(path) {
            Ok(content) => {
                let buffer = Buffer::from_str(&content, &buf_name, path, false);
                let syntax = Self::create_syntax_for_buffer(&buffer, &self.loader);
                let buf_id = self.buffers.len();
                let lang = buffer.language();
                self.buffers.push(buffer);
                if self.syntax_states.len() <= buf_id {
                    self.syntax_states.resize_with(buf_id + 1, || None);
                }
                self.syntax_states[buf_id] = syntax;
                self.workspace_mut().reset_window_for_buffer(buf_id);
                self.status_message = format!("\"{}\"", path.display());

                if let Some(lang) = lang {
                    debug!(?path_str, ?lang, "buffer has language, starting lsp");

                    let cwd = self.workspace().cwd.clone();

                    // Starts the LSP if not already running
                    self.workspace_mut()
                        .lsp_manager
                        .start_server(lang, cwd.as_path());

                    // If a server is already running for this language, send DidOpenTextDocument notification
                    if let Some(server) = self
                        .workspace()
                        .lsp_manager
                        .get_inited_server_for_language(lang)
                        && let Some(uri) = path_to_uri(path)
                    {
                        server
                            .send_notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
                                text_document: TextDocumentItem {
                                    uri,
                                    language_id: lang.id().to_string(),
                                    version: 0,
                                    text: content.clone(),
                                },
                            })
                            .expect(
                                "Failed to send DidOpenTextDocument notification to LSP server",
                            );
                    }
                }
            }
            Err(e) => {
                self.status_message = format!("Error opening file: {}", e);
            }
        }
    }

    pub fn next_buffer(&mut self) {
        if self.buffers.len() <= 1 {
            return;
        }
        let current = self.workspace().window().buffer_id;
        let next = (current + 1) % self.buffers.len();
        let len_chars = self.buffers[next].len_chars();
        self.workspace_mut().switch_buffer(next, len_chars);
        self.status_message = format!("\"{}\"", self.buffers[next].name());
    }

    pub fn prev_buffer(&mut self) {
        if self.buffers.len() <= 1 {
            return;
        }
        let current = self.workspace().window().buffer_id;
        let prev = if current == 0 {
            self.buffers.len() - 1
        } else {
            current - 1
        };
        let len_chars = self.buffers[prev].len_chars();
        self.workspace_mut().switch_buffer(prev, len_chars);
        self.status_message = format!("\"{}\"", self.buffers[prev].name());
    }

    pub fn close_buffer(&mut self) {
        if self.buffers.len() <= 1 {
            self.status_message = String::from("Cannot close last buffer");
            return;
        }
        let buf_id = self.workspace().window().buffer_id;
        if self.buffers[buf_id].is_modified() {
            self.status_message = String::from("Unsaved changes! Use :bd! to force close");
            return;
        }
        self.buffers.remove(buf_id);
        if buf_id < self.syntax_states.len() {
            self.syntax_states.remove(buf_id);
        }
        let buf_count = self.buffers.len();
        for ws in &mut self.workspaces {
            ws.fix_buffer_ids_after_remove(buf_id, buf_count);
        }
        let new_buf_id = self.workspace().window().buffer_id;
        self.status_message = format!("\"{}\"", self.buffers[new_buf_id].name());
    }

    pub fn execute(&mut self, action: EditorAction) -> EditorEffect {
        match action {
            EditorAction::MoveCursor { motion, count } => {
                self.move_cursor(&motion, count);
            }
            EditorAction::SetCursor(pos) => {
                self.window_mut().cursor = self.buffer().clamp_cursor(pos);
            }
            EditorAction::InsertChar(ch) => {
                let cursor = self.cursor();
                let new_cursor = self.buffer_mut().insert_char(cursor, ch);
                self.window_mut().cursor = new_cursor;
            }
            EditorAction::InsertNewline => {
                // Basic heuristic approach to detect 'default' indentation
                // TODO: Hopefully handled by LSP
                let cursor = self.cursor();
                let line_idx = self.buffer().char_to_line(cursor);
                let line_start = self.buffer().line_to_char(line_idx);
                let indent_width = self.buffer().indent_width() as usize;
                let use_tabs = self.buffer().use_tabs();
                let one_indent = if use_tabs {
                    "\t".to_string()
                } else {
                    " ".repeat(indent_width)
                };

                let leading_ws: String = self
                    .buffer()
                    .rope()
                    .line(line_idx)
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .collect();

                let line_to_cursor: String = self
                    .buffer()
                    .rope()
                    .slice(line_start..cursor)
                    .chars()
                    .collect();
                let last_significant = line_to_cursor.trim_end().chars().last();

                let new_indent = match last_significant {
                    Some('{') | Some('(') | Some('[') => {
                        format!("{}{}", leading_ws, one_indent)
                    }
                    _ => leading_ws,
                };

                let new_cursor = self
                    .buffer_mut()
                    .insert_str(cursor, &format!("\n{}", new_indent));
                self.window_mut().cursor = new_cursor;
            }
            EditorAction::InsertTab => {
                let cursor = self.cursor();
                let tab_char = match self.buffer().use_tabs() {
                    true => "\t",
                    false => " ",
                };
                let tab_str = tab_char.repeat(self.buffer().indent_width() as usize);

                let new_cursor = self.buffer_mut().insert_str(cursor, tab_str.as_str());
                self.window_mut().cursor = new_cursor;
            }
            EditorAction::DeleteTillEndOfLine => {
                let cursor = self.cursor();
                let new_cursor = self.buffer_mut().delete_till_eol(cursor);
                self.window_mut().cursor = new_cursor;
            }
            EditorAction::DeleteCharForward { count } => {
                let mut cursor = self.cursor();
                for _ in 0..count {
                    cursor = self.buffer_mut().delete_char_forward(cursor);
                }
                self.window_mut().cursor = cursor;
            }
            EditorAction::DeleteCharBackward => {
                let cursor = self.cursor();
                let new_cursor = self.buffer_mut().delete_char_backward(cursor);
                self.window_mut().cursor = new_cursor;
            }
            EditorAction::DeleteLine { count } => {
                let mut cursor = self.cursor();
                for _ in 0..count {
                    cursor = self.buffer_mut().delete_line(cursor);
                }
                self.window_mut().cursor = cursor;
            }
            EditorAction::DeleteRange(Range { start, end }) => {
                let cursor = self.cursor();
                let (new_cursor, deleted) = self.buffer_mut().delete_range(cursor, start, end);
                self.window_mut().cursor = new_cursor;
                self.registers.unnamed = deleted;
            }
            EditorAction::ChangeRange(Range { start, end }) => {
                let cursor = self.cursor();
                self.buffer_mut().start_edit_group(cursor);
                let (new_cursor, deleted) = self.buffer_mut().delete_range(cursor, start, end);
                self.window_mut().cursor = new_cursor;
                self.registers.unnamed = deleted;
            }
            EditorAction::YankRange(Range { start, end }) => {
                let text = self.buffer().yank_range(start, end);
                self.registers.unnamed = text;
            }
            EditorAction::ReplaceChar(ch) => {
                let cursor = self.cursor();
                let new_cursor = self.buffer_mut().replace_char(cursor, ch);
                self.window_mut().cursor = new_cursor;
            }
            EditorAction::Paste { before } => {
                let text = self.registers.unnamed.clone();
                let cursor = self.cursor();
                let new_cursor = if before {
                    self.buffer_mut().paste_before(cursor, &text)
                } else {
                    self.buffer_mut().paste_after(cursor, &text)
                };
                self.window_mut().cursor = new_cursor;
            }
            EditorAction::Undo => {
                if let Some(new_cursor) = self.buffer_mut().undo() {
                    self.window_mut().cursor = new_cursor;
                }
            }
            EditorAction::Redo => {
                if let Some(new_cursor) = self.buffer_mut().redo() {
                    self.window_mut().cursor = new_cursor;
                }
            }
            EditorAction::StartEditGroup => {
                let cursor = self.cursor();
                self.buffer_mut().start_edit_group(cursor);
            }
            EditorAction::FinishEditGroup => {
                self.buffer_mut().finish_edit_group();
            }
            EditorAction::SetSearchPattern(pattern) => {
                self.search_pattern = pattern;
            }
            EditorAction::SearchNext { count } => {
                self.push_jump();
                self.search_next(count);
            }
            EditorAction::SearchPrev { count } => {
                self.push_jump();
                self.search_prev(count);
            }
            EditorAction::ClearSearch => {
                self.search_pattern.clear();
                let win = self.window_mut();
                win.search_matches.clear();
                win.search_cached_pattern.clear();
                self.status_message.clear();
            }
            EditorAction::Save => match self.buffer_mut().save() {
                Ok(()) => {
                    info!("file saved");
                    self.status_message = String::from("Written");
                }
                Err(e) => {
                    error!(?e, "failed to save");
                    self.status_message = format!("Error: {}", e);
                }
            },
            EditorAction::Quit { force } => {
                if force {
                    return EditorEffect::Task(iced::exit());
                }
                self.status_message =
                    String::from("Use :qa! to force quit all, or :wq to save and close");
            }
            EditorAction::ForceQuitApp => {
                return EditorEffect::Task(iced::exit());
            }
            EditorAction::WriteQuit => match self.buffer_mut().save() {
                Ok(()) => {
                    info!("file saved");
                    let ws = &mut self.workspaces[self.active_workspace];
                    if ws.layout.leaf_count() > 1 {
                        ws.close_window();
                    } else {
                        return EditorEffect::Task(iced::exit());
                    }
                }
                Err(e) => {
                    error!(?e, "failed to save");
                    self.status_message = format!("Error: {}", e);
                }
            },
            EditorAction::OpenFile(path) => {
                self.push_jump();
                self.open_file(&path);
            }
            EditorAction::NextBuffer => {
                self.next_buffer();
            }
            EditorAction::PrevBuffer => {
                self.prev_buffer();
            }
            EditorAction::CloseBuffer => {
                self.close_buffer();
            }
            EditorAction::VSplit => {
                self.workspace_mut().split(SplitDirection::Vertical);
            }
            EditorAction::HSplit => {
                self.workspace_mut().split(SplitDirection::Horizontal);
            }
            EditorAction::CloseWindow => {
                let ws = &mut self.workspaces[self.active_workspace];
                if ws.layout.leaf_count() > 1 {
                    ws.close_window();
                } else if self.has_unsaved_changes() {
                    self.status_message = String::from(
                        "Unsaved changes! Use :q! to force quit, or :wq to save and close",
                    );
                } else {
                    return EditorEffect::Task(iced::exit());
                }
            }
            EditorAction::FocusLeft => {
                self.workspace_mut()
                    .focus_direction(SplitDirection::Vertical, false);
            }
            EditorAction::FocusRight => {
                self.workspace_mut()
                    .focus_direction(SplitDirection::Vertical, true);
            }
            EditorAction::FocusUp => {
                self.workspace_mut()
                    .focus_direction(SplitDirection::Horizontal, false);
            }
            EditorAction::FocusDown => {
                self.workspace_mut()
                    .focus_direction(SplitDirection::Horizontal, true);
            }
            EditorAction::SetMode(mode) => {
                self.mode_display = mode;
            }
            EditorAction::SetStatusMessage(msg) => {
                self.status_message = msg;
            }
            EditorAction::SetSelection(sel) => {
                self.window_mut().selection = sel;
            }
            EditorAction::UpdateVisualSelection { anchor, mode } => {
                let cursor = self.cursor();
                let buf = self.buffer();
                let selection = if mode == VimMode::VisualLine {
                    let anchor_line = buf.char_to_line(anchor);
                    let cursor_line = buf.char_to_line(cursor);
                    let (start_line, end_line) = if anchor_line <= cursor_line {
                        (anchor_line, cursor_line)
                    } else {
                        (cursor_line, anchor_line)
                    };
                    let start = buf.line_to_char(start_line);
                    let end = if end_line + 1 < buf.total_lines() {
                        buf.line_to_char(end_line + 1)
                    } else {
                        buf.len_chars()
                    };
                    (start, end)
                } else if anchor <= cursor {
                    (anchor, cursor + 1)
                } else {
                    (cursor, anchor + 1)
                };
                self.window_mut().selection = Some(selection);
            }
            EditorAction::SystemCopy => {
                self.system_copy(false);
            }
            EditorAction::SystemCut => {
                self.system_copy(true);
            }
            EditorAction::SystemPaste => {
                self.system_paste();
            }
            EditorAction::LspGotoDefinition => {
                self.push_jump();
                let buf = self.buffer();
                let cursor = self.cursor();
                if let Some(file_path) = buf.file_path() {
                    let position = offset_to_lsp_position(buf.rope(), cursor);
                    let path = Path::new(file_path);
                    if let Some(lang) = buf.language()
                        && let Some(uri) = crate::lsp::path_to_uri(path)
                    {
                        if let Some(server) = self
                            .workspace()
                            .lsp_manager
                            .get_inited_server_for_language(lang)
                        {
                            let server_id = server.id;
                            self.workspace_mut()
                                .lsp_manager
                                .send_request::<GotoDefinition>(
                                    server_id,
                                    GotoDefinitionParams {
                                        text_document_position_params: TextDocumentPositionParams {
                                            text_document: TextDocumentIdentifier { uri },
                                            position,
                                        },
                                        work_done_progress_params: Default::default(),
                                        partial_result_params: Default::default(),
                                    },
                                );
                        } else {
                            self.status_message = String::from("LSP not ready");
                        }
                    }
                }
            }
            EditorAction::JumpBackward => {
                self.jump_backward();
            }
            EditorAction::JumpForward => {
                self.jump_forward();
            }
            EditorAction::OpenPicker(_) => {
                // Handled by app layer, not editor
            }
        }
        EditorEffect::None
    }

    fn system_copy(&mut self, cut: bool) {
        let win = self.workspace().window();
        let buf_id = win.buffer_id;
        let text = if let Some((start, end)) = win.selection {
            let yanked = self.buffers[buf_id].yank_range(start, end);
            if cut {
                let cursor = win.cursor;
                let (new_cursor, _) = self.buffers[buf_id].delete_range(cursor, start, end);
                self.workspace_mut().window_mut().cursor = new_cursor;
                self.workspace_mut().window_mut().selection = None;
            }
            yanked
        } else {
            let cursor = win.cursor;
            let line = self.buffers[buf_id].char_to_line(cursor);
            let line_start = self.buffers[buf_id].line_to_char(line);
            let line_end = if line + 1 < self.buffers[buf_id].total_lines() {
                self.buffers[buf_id].line_to_char(line + 1)
            } else {
                self.buffers[buf_id].len_chars()
            };
            let yanked = self.buffers[buf_id].yank_range(line_start, line_end);
            if cut {
                let (new_cursor, _) =
                    self.buffers[buf_id].delete_range(cursor, line_start, line_end);
                self.workspace_mut().window_mut().cursor = new_cursor;
            }
            yanked
        };
        self.registers.unnamed = text.clone();
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(text);
        }
    }

    fn system_paste(&mut self) {
        let text = if let Ok(mut clipboard) = arboard::Clipboard::new() {
            clipboard.get_text().unwrap_or_default()
        } else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let cursor = self.cursor();
        let new_cursor = self.buffer_mut().insert_str(cursor, &text);
        self.window_mut().cursor = new_cursor;
    }

    pub fn buffer_list(&self) -> Vec<(usize, String)> {
        self.buffers
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let modified = if b.is_modified() { " [+]" } else { "" };
                (i, format!("{}{}", b.name(), modified))
            })
            .collect()
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.buffers.iter().any(|b| b.is_modified())
    }

    fn move_cursor(&mut self, motion: &Motion, count: usize) {
        let ws = &self.workspaces[self.active_workspace];
        let win = ws.window();
        let buf_id = win.buffer_id;
        let cursor = win.cursor;
        let buffer = &self.buffers[buf_id];

        let new_cursor = match motion {
            Motion::Left => {
                let mut c = cursor;
                for _ in 0..count {
                    c = buffer.move_left(c);
                }
                c
            }
            Motion::Right => {
                let mut c = cursor;
                for _ in 0..count {
                    c = buffer.move_right(c);
                }
                c
            }
            Motion::Up => {
                let mut c = cursor;
                for _ in 0..count {
                    c = buffer.move_up(c);
                }
                c
            }
            Motion::Down => {
                let mut c = cursor;
                for _ in 0..count {
                    c = buffer.move_down(c);
                }
                c
            }
            Motion::WordForward => {
                let mut c = cursor;
                for _ in 0..count {
                    c = buffer.move_word_forward(c);
                }
                c
            }
            Motion::WordBackward => {
                let mut c = cursor;
                for _ in 0..count {
                    c = buffer.move_word_backward(c);
                }
                c
            }
            Motion::WordEnd => {
                let mut c = cursor;
                for _ in 0..count {
                    c = buffer.move_word_end(c);
                }
                c
            }
            Motion::LineStart => buffer.move_to_line_start(cursor),
            Motion::LineEnd => buffer.move_to_line_end(cursor),
            Motion::FirstNonWhitespace => buffer.move_to_first_non_whitespace(cursor),
            Motion::FileStart => buffer.move_to_start(),
            Motion::FileEnd => buffer.move_to_end(),
            Motion::FindChar {
                ch,
                forward,
                stop_before,
            } => {
                let mut c = cursor;
                for _ in 0..count {
                    c = buffer.find_char_on_line(c, *ch, *forward, *stop_before);
                }
                c
            }
            Motion::HalfPageDown => {
                let half = (win.visible_lines / 2).max(1) * count;
                let mut c = cursor;
                for _ in 0..half {
                    c = buffer.move_down(c);
                }
                c
            }
            Motion::HalfPageUp => {
                let half = (win.visible_lines / 2).max(1) * count;
                let mut c = cursor;
                for _ in 0..half {
                    c = buffer.move_up(c);
                }
                c
            }
        };

        self.workspace_mut().window_mut().cursor = new_cursor;
    }

    fn search_next(&mut self, count: usize) {
        if self.search_pattern.is_empty() {
            self.status_message = String::from("No search pattern");
            return;
        }
        self.update_search_cache();
        let win = self.workspace().window();
        if win.search_matches.is_empty() {
            self.status_message = format!("/{} [0/0]", self.search_pattern);
            return;
        }
        let cursor_before = win.cursor;
        let mut cursor = win.cursor;
        let matches = &win.search_matches;
        for _ in 0..count {
            let next = matches.iter().find(|&&m| m > cursor).or(matches.first());
            if let Some(&pos) = next {
                cursor = pos;
            }
        }
        self.workspace_mut().window_mut().cursor = cursor;

        let matches = &self.workspace().window().search_matches;
        let total = matches.len();
        let current = matches
            .iter()
            .position(|&m| m == cursor)
            .map(|i| i + 1)
            .unwrap_or(0);
        let wrapped = if cursor <= cursor_before && count > 0 {
            " [wrapped]"
        } else {
            ""
        };
        self.status_message = format!(
            "/{} [{}/{}]{}",
            self.search_pattern, current, total, wrapped
        );
    }

    fn search_prev(&mut self, count: usize) {
        if self.search_pattern.is_empty() {
            self.status_message = String::from("No search pattern");
            return;
        }
        self.update_search_cache();
        let win = self.workspace().window();
        if win.search_matches.is_empty() {
            self.status_message = format!("?{} [0/0]", self.search_pattern);
            return;
        }
        let cursor_before = win.cursor;
        let mut cursor = win.cursor;
        let matches = &win.search_matches;
        for _ in 0..count {
            let prev = matches
                .iter()
                .rev()
                .find(|&&m| m < cursor)
                .or(matches.last());
            if let Some(&pos) = prev {
                cursor = pos;
            }
        }
        self.workspace_mut().window_mut().cursor = cursor;

        let matches = &self.workspace().window().search_matches;
        let total = matches.len();
        let current = matches
            .iter()
            .position(|&m| m == cursor)
            .map(|i| i + 1)
            .unwrap_or(0);
        let wrapped = if cursor >= cursor_before && count > 0 {
            " [wrapped]"
        } else {
            ""
        };
        self.status_message = format!(
            "?{} [{}/{}]{}",
            self.search_pattern, current, total, wrapped
        );
    }
}
