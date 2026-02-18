use std::path::{Path, PathBuf};

use tracing::{debug, error, info};

use crate::action::{EditorAction, EditorEffect};
use crate::buffer::Buffer;
use crate::core_actions::{execute_core, search_next_in, search_prev_in};
use crate::diagnostics::DiagnosticStore;
use crate::layout::SplitDirection;
use crate::registers::Registers;
use crate::syntax::SyntaxState;
use crate::syntax::loader::Loader;
use crate::workspace::Workspace;

pub struct Editor {
    pub buffers: Vec<Buffer>,
    pub syntax_states: Vec<Option<SyntaxState>>,
    pub diagnostics: DiagnosticStore,
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
            diagnostics: DiagnosticStore::new(),
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

    pub fn buffer(&self) -> &Buffer {
        let buf_id = self.workspace().window().buffer_id;
        &self.buffers[buf_id]
    }

    pub fn cursor(&self) -> usize {
        self.workspace().cursor()
    }

    fn push_jump(&mut self) {
        let (win, _) = current_ref!(self);
        self.jump_list.push(win.buffer_id, win.cursor);
    }

    fn jump_backward(&mut self) {
        let (win, _) = current_ref!(self);
        let cur_buf = win.buffer_id;
        let cur_cursor = win.cursor;
        if let Some((buf_id, cursor)) = self.jump_list.backward(cur_buf, cur_cursor)
            && buf_id < self.buffers.len()
        {
            let len_chars = self.buffers[buf_id].len_chars();
            self.workspaces[self.active_workspace].switch_buffer(buf_id, len_chars);
            self.workspaces[self.active_workspace].window_mut().cursor =
                self.buffers[buf_id].clamp_cursor(cursor);
        }
    }

    fn jump_forward(&mut self) {
        if let Some((buf_id, cursor)) = self.jump_list.forward()
            && buf_id < self.buffers.len()
        {
            let len_chars = self.buffers[buf_id].len_chars();
            self.workspaces[self.active_workspace].switch_buffer(buf_id, len_chars);
            self.workspaces[self.active_workspace].window_mut().cursor =
                self.buffers[buf_id].clamp_cursor(cursor);
        }
    }

    pub fn ensure_cursor_visible(&mut self) {
        let (win, buf) = current_win_mut!(self);
        win.ensure_cursor_visible(buf);
    }

    pub fn update_search_cache(&mut self) {
        let pattern = self.search_pattern.clone();
        let (win, buf) = current_win_mut!(self);
        win.update_search_cache(buf, &pattern);
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

                    self.workspace()
                        .lsp_manager
                        .notify_did_open(&self.buffers[buf_id], &content);
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
        self.notify_lsp_did_close(&self.buffers[buf_id]);
        self.buffers.remove(buf_id);
        if buf_id < self.syntax_states.len() {
            self.syntax_states.remove(buf_id);
        }
        self.diagnostics.remove_buffer(buf_id);
        let buf_count = self.buffers.len();
        for ws in &mut self.workspaces {
            ws.fix_buffer_ids_after_remove(buf_id, buf_count);
        }
        let new_buf_id = self.workspace().window().buffer_id;
        self.status_message = format!("\"{}\"", self.buffers[new_buf_id].name());
    }

    pub fn execute(&mut self, action: EditorAction) -> EditorEffect {
        let (_, buf) = current_ref!(self);
        let version_before = buf.version();

        // Try the portable core execution first
        {
            let (win, buf, _) = current_mut!(self);
            if execute_core(
                win,
                buf,
                &mut self.registers,
                &mut self.status_message,
                &action,
            ) {
                let (_, buf) = current_ref!(self);
                if buf.version() != version_before {
                    self.notify_lsp_did_change();
                }
                return EditorEffect::None;
            }
        }

        // Editor-specific actions (search with jump, save, quit, LSP, workspace ops, etc.)
        match action {
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
                self.status_message.clear();
                let (win, _, _) = current_mut!(self);
                win.search_matches.clear();
                win.search_cached_pattern.clear();
            }
            EditorAction::Save => {
                let (_, buf, _) = current_mut!(self);
                match buf.save() {
                    Ok(()) => {
                        info!("file saved");
                        self.notify_lsp_did_save();
                        self.status_message = String::from("Written");
                    }
                    Err(e) => {
                        error!(?e, "failed to save");
                        self.status_message = format!("Error: {}", e);
                    }
                }
            }
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
            EditorAction::WriteQuit => {
                let (_, buf, _) = current_mut!(self);
                match buf.save() {
                    Ok(()) => {
                        info!("file saved");
                        self.notify_lsp_did_save();
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
            }
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
            EditorAction::LspGotoDefinition => {
                self.push_jump();
                let (win, buf) = current_ref!(self);
                let cursor = win.cursor;
                if self.workspaces[self.active_workspace]
                    .lsp_manager
                    .goto_definition(buf, cursor)
                    .is_none()
                {
                    self.status_message = String::from("LSP not ready");
                }
            }
            EditorAction::LspReferences => {
                self.push_jump();
                let (win, buf) = current_ref!(self);
                let cursor = win.cursor;
                if self.workspaces[self.active_workspace]
                    .lsp_manager
                    .find_references(buf, cursor)
                    .is_none()
                {
                    self.status_message = String::from("LSP not ready");
                }
            }
            EditorAction::LspImplementation => {
                self.push_jump();
                let (win, buf) = current_ref!(self);
                let cursor = win.cursor;
                if self.workspaces[self.active_workspace]
                    .lsp_manager
                    .goto_implementation(buf, cursor)
                    .is_none()
                {
                    self.status_message = String::from("LSP not ready");
                }
            }
            EditorAction::LspDeclaration => {
                self.push_jump();
                let (win, buf) = current_ref!(self);
                let cursor = win.cursor;
                if self.workspaces[self.active_workspace]
                    .lsp_manager
                    .goto_declaration(buf, cursor)
                    .is_none()
                {
                    self.status_message = String::from("LSP not ready");
                }
            }
            EditorAction::JumpBackward => {
                self.jump_backward();
            }
            EditorAction::JumpForward => {
                self.jump_forward();
            }
            EditorAction::OpenBufferPicker
            | EditorAction::OpenFilePicker { .. }
            | EditorAction::OpenDiagnosticsPicker
            | EditorAction::LspHover
            | EditorAction::ShowDiagnosticHover => {}
            EditorAction::SwitchToBuffer(buf_id) => {
                if buf_id < self.buffers.len() {
                    let len_chars = self.buffers[buf_id].len_chars();
                    self.workspace_mut().switch_buffer(buf_id, len_chars);
                }
            }
            EditorAction::OpenFileAtPosition { path, line, col } => {
                self.push_jump();
                self.open_file(&path);
                let (win, buf) = current_win_mut!(self);
                win.cursor = buf.clamp_cursor(buf.cursor_from_position(line, col));
            }
            _ => {} // already handled by execute_core
        }
        let (_, buf) = current_ref!(self);
        if buf.version() != version_before {
            self.notify_lsp_did_change();
        }
        EditorEffect::None
    }

    fn notify_lsp_did_change(&self) {
        let (_, buf) = current_ref!(self);
        self.workspaces[self.active_workspace]
            .lsp_manager
            .notify_did_change(buf);
    }

    fn notify_lsp_did_save(&self) {
        let (_, buf) = current_ref!(self);
        self.workspaces[self.active_workspace]
            .lsp_manager
            .notify_did_save(buf);
    }

    fn notify_lsp_did_close(&self, buf: &Buffer) {
        self.workspaces[self.active_workspace]
            .lsp_manager
            .notify_did_close(buf);
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.buffers.iter().any(|b| b.is_modified())
    }

    fn search_next(&mut self, count: usize) {
        if self.search_pattern.is_empty() {
            self.status_message = String::from("No search pattern");
            return;
        }
        self.update_search_cache();
        let (win, _) = current_ref!(self);
        if win.search_matches.is_empty() {
            self.status_message = format!("/{} [0/0]", self.search_pattern);
            return;
        }
        let cursor_before = win.cursor;
        let (win, _, _) = current_mut!(self);
        search_next_in(win, &self.search_pattern, count);

        let (win, _) = current_ref!(self);
        let total = win.search_matches.len();
        let current = win
            .search_matches
            .iter()
            .position(|&m| m == win.cursor)
            .map(|i| i + 1)
            .unwrap_or(0);
        let wrapped = if win.cursor <= cursor_before && count > 0 {
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
        let (win, _) = current_ref!(self);
        if win.search_matches.is_empty() {
            self.status_message = format!("?{} [0/0]", self.search_pattern);
            return;
        }
        let cursor_before = win.cursor;
        let (win, _, _) = current_mut!(self);
        search_prev_in(win, &self.search_pattern, count);

        let (win, _) = current_ref!(self);
        let total = win.search_matches.len();
        let current = win
            .search_matches
            .iter()
            .position(|&m| m == win.cursor)
            .map(|i| i + 1)
            .unwrap_or(0);
        let wrapped = if win.cursor >= cursor_before && count > 0 {
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
