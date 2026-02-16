use std::path::{Path, PathBuf};

use lsp_types::notification::{DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument};
use lsp_types::request::{GotoDeclaration, GotoDefinition, GotoImplementation, References, Request};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionParams, ReferenceContext, ReferenceParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, VersionedTextDocumentIdentifier,
};
use tracing::{debug, error, info};

use crate::action::{EditorAction, EditorEffect, Motion, Range};
use crate::buffer::Buffer;
use crate::layout::SplitDirection;
use crate::lsp::{offset_to_lsp_position, path_to_uri};
use crate::registers::Registers;
use crate::syntax::loader::Loader;
use crate::syntax::SyntaxState;
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
        if let Some((buf_id, cursor)) = self.jump_list.backward(cur_buf, cur_cursor) {
            if buf_id < self.buffers.len() {
                let len_chars = self.buffers[buf_id].len_chars();
                self.workspaces[self.active_workspace].switch_buffer(buf_id, len_chars);
                self.workspaces[self.active_workspace].window_mut().cursor =
                    self.buffers[buf_id].clamp_cursor(cursor);
            }
        }
    }

    fn jump_forward(&mut self) {
        if let Some((buf_id, cursor)) = self.jump_list.forward() {
            if buf_id < self.buffers.len() {
                let len_chars = self.buffers[buf_id].len_chars();
                self.workspaces[self.active_workspace].switch_buffer(buf_id, len_chars);
                self.workspaces[self.active_workspace].window_mut().cursor =
                    self.buffers[buf_id].clamp_cursor(cursor);
            }
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
        self.notify_lsp_did_close(&self.buffers[buf_id]);
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
        let (_, buf) = current_ref!(self);
        let version_before = buf.version();
        match action {
            EditorAction::MoveCursor { motion, count } => {
                self.move_cursor(&motion, count);
            }
            EditorAction::SetCursor(pos) => {
                let (win, buf) = current_win_mut!(self);
                win.cursor = buf.clamp_cursor(pos);
            }
            EditorAction::InsertChar(ch) => {
                let (win, buf, _) = current_mut!(self);
                win.cursor = buf.insert_char(win.cursor, ch);
            }
            EditorAction::InsertNewline => {
                let (win, buf, _) = current_mut!(self);
                let cursor = win.cursor;
                let line_idx = buf.char_to_line(cursor);
                let line_start = buf.line_to_char(line_idx);
                let indent_width = buf.indent_width() as usize;
                let one_indent = if buf.use_tabs() {
                    "\t".to_string()
                } else {
                    " ".repeat(indent_width)
                };

                let leading_ws: String = buf
                    .rope()
                    .line(line_idx)
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .collect();

                let line_to_cursor: String = buf
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

                win.cursor = buf.insert_str(cursor, &format!("\n{}", new_indent));
            }
            EditorAction::InsertTab => {
                let (win, buf, _) = current_mut!(self);
                let tab_str = if buf.use_tabs() {
                    "\t".repeat(buf.indent_width() as usize)
                } else {
                    " ".repeat(buf.indent_width() as usize)
                };
                win.cursor = buf.insert_str(win.cursor, &tab_str);
            }
            EditorAction::DeleteTillEndOfLine => {
                let (win, buf, _) = current_mut!(self);
                win.cursor = buf.delete_till_eol(win.cursor);
            }
            EditorAction::DeleteCharForward { count } => {
                let (win, buf, _) = current_mut!(self);
                for _ in 0..count {
                    win.cursor = buf.delete_char_forward(win.cursor);
                }
            }
            EditorAction::DeleteCharBackward => {
                let (win, buf, _) = current_mut!(self);
                win.cursor = buf.delete_char_backward(win.cursor);
            }
            EditorAction::DeleteLine { count } => {
                let (win, buf, _) = current_mut!(self);
                for _ in 0..count {
                    win.cursor = buf.delete_line(win.cursor);
                }
            }
            EditorAction::DeleteRange(Range { start, end }) => {
                let (win, buf, _) = current_mut!(self);
                let (new_cursor, deleted) = buf.delete_range(win.cursor, start, end);
                win.cursor = new_cursor;
                self.registers.unnamed = deleted;
            }
            EditorAction::ChangeRange(Range { start, end }) => {
                let (win, buf, _) = current_mut!(self);
                buf.start_edit_group(win.cursor);
                let (new_cursor, deleted) = buf.delete_range(win.cursor, start, end);
                win.cursor = new_cursor;
                self.registers.unnamed = deleted;
            }
            EditorAction::YankRange(Range { start, end }) => {
                let (_, buf) = current_ref!(self);
                self.registers.unnamed = buf.yank_range(start, end);
            }
            EditorAction::ReplaceChar(ch) => {
                let (win, buf, _) = current_mut!(self);
                win.cursor = buf.replace_char(win.cursor, ch);
            }
            EditorAction::Paste { before } => {
                let text = self.registers.unnamed.clone();
                let (win, buf, _) = current_mut!(self);
                win.cursor = if before {
                    buf.paste_before(win.cursor, &text)
                } else {
                    buf.paste_after(win.cursor, &text)
                };
            }
            EditorAction::Undo => {
                let (win, buf, _) = current_mut!(self);
                if let Some(new_cursor) = buf.undo() {
                    win.cursor = new_cursor;
                }
            }
            EditorAction::Redo => {
                let (win, buf, _) = current_mut!(self);
                if let Some(new_cursor) = buf.redo() {
                    win.cursor = new_cursor;
                }
            }
            EditorAction::StartEditGroup => {
                let (win, buf, _) = current_mut!(self);
                buf.start_edit_group(win.cursor);
            }
            EditorAction::FinishEditGroup => {
                let (_, buf, _) = current_mut!(self);
                buf.finish_edit_group();
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
            EditorAction::SetStatusMessage(msg) => {
                self.status_message = msg;
            }
            EditorAction::SetSelection(sel) => {
                let (win, _, _) = current_mut!(self);
                win.selection = sel;
            }
            EditorAction::UpdateVisualSelection { anchor, mode } => {
                let (win, buf) = current_win_mut!(self);
                let cursor = win.cursor;
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
                win.selection = Some(selection);
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
                self.send_lsp_position_request::<GotoDefinition>("LSP not ready");
            }
            EditorAction::LspReferences => {
                self.push_jump();
                let (win, buf) = current_ref!(self);
                let cursor = win.cursor;
                if let Some(file_path) = buf.file_path() {
                    let position = offset_to_lsp_position(buf.rope(), cursor);
                    let path = Path::new(file_path);
                    if let Some(lang) = buf.language()
                        && let Some(uri) = crate::lsp::path_to_uri(path)
                    {
                        let ws = &self.workspaces[self.active_workspace];
                        if let Some(server) = ws.lsp_manager.get_inited_server_for_language(lang) {
                            let server_id = server.id;
                            self.workspaces[self.active_workspace]
                                .lsp_manager
                                .send_request::<References>(
                                    server_id,
                                    ReferenceParams {
                                        text_document_position: TextDocumentPositionParams {
                                            text_document: TextDocumentIdentifier { uri },
                                            position,
                                        },
                                        work_done_progress_params: Default::default(),
                                        partial_result_params: Default::default(),
                                        context: ReferenceContext {
                                            include_declaration: true,
                                        },
                                    },
                                );
                        } else {
                            self.status_message = String::from("LSP not ready");
                        }
                    }
                }
            }
            EditorAction::LspImplementation => {
                self.push_jump();
                self.send_lsp_position_request::<GotoImplementation>("LSP not ready");
            }
            EditorAction::LspDeclaration => {
                self.push_jump();
                self.send_lsp_position_request::<GotoDeclaration>("LSP not ready");
            }
            EditorAction::JumpBackward => {
                self.jump_backward();
            }
            EditorAction::JumpForward => {
                self.jump_forward();
            }
            EditorAction::OpenBufferPicker | EditorAction::OpenFilePicker { .. } => {}
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
        }
        let (_, buf) = current_ref!(self);
        if buf.version() != version_before {
            self.notify_lsp_did_change();
        }
        EditorEffect::None
    }

    fn send_lsp_position_request<R: Request<Params = GotoDefinitionParams>>(
        &mut self,
        not_ready_msg: &str,
    ) {
        let (win, buf) = current_ref!(self);
        let cursor = win.cursor;
        if let Some(file_path) = buf.file_path() {
            let position = offset_to_lsp_position(buf.rope(), cursor);
            let path = Path::new(file_path);
            if let Some(lang) = buf.language()
                && let Some(uri) = crate::lsp::path_to_uri(path)
            {
                let ws = &self.workspaces[self.active_workspace];
                if let Some(server) = ws.lsp_manager.get_inited_server_for_language(lang) {
                    let server_id = server.id;
                    self.workspaces[self.active_workspace]
                        .lsp_manager
                        .send_request::<R>(
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
                    self.status_message = String::from(not_ready_msg);
                }
            }
        }
    }

    fn notify_lsp_did_change(&self) {
        let (_, buf) = current_ref!(self);
        let lang = match buf.language() {
            Some(l) => l,
            None => return,
        };
        let file_path = match buf.file_path() {
            Some(p) => p,
            None => return,
        };
        let ws = &self.workspaces[self.active_workspace];
        let server = match ws.lsp_manager.get_inited_server_for_language(lang) {
            Some(s) => s,
            None => return,
        };
        let uri = match path_to_uri(Path::new(file_path)) {
            Some(u) => u,
            None => return,
        };
        server
            .send_notification::<DidChangeTextDocument>(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri,
                    version: buf.version() as i32,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: buf.rope().to_string(),
                }],
            })
            .ok();
    }

    fn notify_lsp_did_save(&self) {
        let (_, buf) = current_ref!(self);
        let lang = match buf.language() {
            Some(l) => l,
            None => return,
        };
        let file_path = match buf.file_path() {
            Some(p) => p,
            None => return,
        };
        let ws = &self.workspaces[self.active_workspace];
        let server = match ws.lsp_manager.get_inited_server_for_language(lang) {
            Some(s) => s,
            None => return,
        };
        let uri = match path_to_uri(Path::new(file_path)) {
            Some(u) => u,
            None => return,
        };
        server
            .send_notification::<DidSaveTextDocument>(DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
                text: Some(buf.rope().to_string()),
            })
            .ok();
    }

    fn notify_lsp_did_close(&self, buf: &Buffer) {
        let lang = match buf.language() {
            Some(l) => l,
            None => return,
        };
        let file_path = match buf.file_path() {
            Some(p) => p,
            None => return,
        };
        let ws = &self.workspaces[self.active_workspace];
        let server = match ws.lsp_manager.get_inited_server_for_language(lang) {
            Some(s) => s,
            None => return,
        };
        let uri = match path_to_uri(Path::new(file_path)) {
            Some(u) => u,
            None => return,
        };
        server
            .send_notification::<DidCloseTextDocument>(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
            })
            .ok();
    }

    fn system_copy(&mut self, cut: bool) {
        let (win, buf, _) = current_mut!(self);
        let text = if let Some((start, end)) = win.selection {
            let yanked = buf.yank_range(start, end);
            if cut {
                let (new_cursor, _) = buf.delete_range(win.cursor, start, end);
                win.cursor = new_cursor;
                win.selection = None;
            }
            yanked
        } else {
            let cursor = win.cursor;
            let line = buf.char_to_line(cursor);
            let line_start = buf.line_to_char(line);
            let line_end = if line + 1 < buf.total_lines() {
                buf.line_to_char(line + 1)
            } else {
                buf.len_chars()
            };
            let yanked = buf.yank_range(line_start, line_end);
            if cut {
                let (new_cursor, _) = buf.delete_range(cursor, line_start, line_end);
                win.cursor = new_cursor;
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
        let (win, buf, _) = current_mut!(self);
        win.cursor = buf.insert_str(win.cursor, &text);
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.buffers.iter().any(|b| b.is_modified())
    }

    fn move_cursor(&mut self, motion: &Motion, count: usize) {
        let (win, buffer) = current_ref!(self);
        let cursor = win.cursor;

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

        let (win, _, _) = current_mut!(self);
        win.cursor = new_cursor;
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
        let mut cursor = win.cursor;
        for _ in 0..count {
            let next = win.search_matches.iter().find(|&&m| m > cursor).or(win.search_matches.first());
            if let Some(&pos) = next {
                cursor = pos;
            }
        }

        let total = win.search_matches.len();
        let current = win.search_matches
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
        let (win, _, _) = current_mut!(self);
        win.cursor = cursor;
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
        let mut cursor = win.cursor;
        for _ in 0..count {
            let prev = win.search_matches
                .iter()
                .rev()
                .find(|&&m| m < cursor)
                .or(win.search_matches.last());
            if let Some(&pos) = prev {
                cursor = pos;
            }
        }

        let total = win.search_matches.len();
        let current = win.search_matches
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
        let (win, _, _) = current_mut!(self);
        win.cursor = cursor;
    }
}
