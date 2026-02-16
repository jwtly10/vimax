mod edit;
mod lsp;
mod motion;
mod window;

use std::path::{Path, PathBuf};

use lsp_types::notification::DidOpenTextDocument;
use lsp_types::{DidOpenTextDocumentParams, TextDocumentItem};
use tracing::{debug, error, info};

use crate::action::{EditorAction, EditorEffect};
use crate::buffer::Buffer;
use crate::lsp::path_to_uri;
use crate::registers::Registers;
use crate::syntax::loader::Loader;
use crate::syntax::SyntaxState;
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

struct JumpList {
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

    pub fn window_mut(&mut self) -> &mut crate::window::Window {
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

    pub(crate) fn push_jump(&mut self) {
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
                let version = buffer.version();
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

                    self.workspace_mut()
                        .lsp_manager
                        .start_server(lang, cwd.as_path());

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
                                    version: version as i32,
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

    /// The central dispatch point for all editor actions.
    /// Each match arm delegates to a method in a domain-specific file.
    pub fn execute(&mut self, action: EditorAction) -> EditorEffect {
        match action {
            // Motion (editor/motion.rs)
            EditorAction::MoveCursor { motion, count } => self.move_cursor(&motion, count),
            EditorAction::SetCursor(pos) => self.execute_set_cursor(pos),
            EditorAction::SearchNext { count } => self.execute_search_next(count),
            EditorAction::SearchPrev { count } => self.execute_search_prev(count),
            EditorAction::SetSearchPattern(pattern) => {
                self.search_pattern = pattern;
            }
            EditorAction::ClearSearch => self.execute_clear_search(),
            EditorAction::JumpBackward => self.jump_backward(),
            EditorAction::JumpForward => self.jump_forward(),

            // Editing (editor/edit.rs)
            EditorAction::InsertChar(ch) => self.execute_insert_char(ch),
            EditorAction::InsertNewline => self.execute_insert_newline(),
            EditorAction::InsertTab => self.execute_insert_tab(),
            EditorAction::DeleteTillEndOfLine => self.execute_delete_till_eol(),
            EditorAction::DeleteCharForward { count } => self.execute_delete_char_forward(count),
            EditorAction::DeleteCharBackward => self.execute_delete_char_backward(),
            EditorAction::DeleteLine { count } => self.execute_delete_line(count),
            EditorAction::DeleteRange(range) => self.execute_delete_range(range),
            EditorAction::ChangeRange(range) => self.execute_change_range(range),
            EditorAction::YankRange(range) => self.execute_yank_range(range),
            EditorAction::ReplaceChar(ch) => self.execute_replace_char(ch),
            EditorAction::Paste { before } => self.execute_paste(before),
            EditorAction::Undo => self.execute_undo(),
            EditorAction::Redo => self.execute_redo(),
            EditorAction::StartEditGroup => self.execute_start_edit_group(),
            EditorAction::FinishEditGroup => self.execute_finish_edit_group(),
            EditorAction::SystemCopy => self.system_copy(false),
            EditorAction::SystemCut => self.system_copy(true),
            EditorAction::SystemPaste => self.system_paste(),

            // Window management (editor/window.rs)
            EditorAction::VSplit => self.execute_vsplit(),
            EditorAction::HSplit => self.execute_hsplit(),
            EditorAction::CloseWindow => return self.execute_close_window(),
            EditorAction::FocusLeft => self.execute_focus_left(),
            EditorAction::FocusRight => self.execute_focus_right(),
            EditorAction::FocusUp => self.execute_focus_up(),
            EditorAction::FocusDown => self.execute_focus_down(),

            // File/buffer operations
            EditorAction::Save => return self.execute_save(),
            EditorAction::Quit { force } => return self.execute_quit(force),
            EditorAction::ForceQuitApp => return EditorEffect::Task(iced::exit()),
            EditorAction::WriteQuit => return self.execute_write_quit(),
            EditorAction::OpenFile(path) => {
                self.push_jump();
                self.open_file(&path);
            }
            EditorAction::NextBuffer => self.next_buffer(),
            EditorAction::PrevBuffer => self.prev_buffer(),
            EditorAction::CloseBuffer => self.close_buffer(),

            // State setters
            EditorAction::SetMode(mode) => self.mode_display = mode,
            EditorAction::SetStatusMessage(msg) => self.status_message = msg,
            EditorAction::SetSelection(sel) => self.window_mut().selection = sel,
            EditorAction::UpdateVisualSelection { anchor, mode } => {
                self.execute_update_visual_selection(anchor, mode)
            }

            // LSP (editor/lsp.rs)
            EditorAction::LspGotoDefinition => self.execute_lsp_goto_definition(),

            // Picker is handled at the app layer
            EditorAction::OpenPicker(_) => {}
        }
        EditorEffect::None
    }

    fn execute_save(&mut self) -> EditorEffect {
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
        EditorEffect::None
    }

    fn execute_quit(&mut self, force: bool) -> EditorEffect {
        if force {
            return EditorEffect::Task(iced::exit());
        }
        self.status_message =
            String::from("Use :qa! to force quit all, or :wq to save and close");
        EditorEffect::None
    }

    fn execute_write_quit(&mut self) -> EditorEffect {
        match self.buffer_mut().save() {
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
        }
        EditorEffect::None
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
}
