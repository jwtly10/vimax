use std::path::{Path, PathBuf};

use std::time::{Duration, Instant};

use crate::action::{EditorAction, EditorEffect};
use crate::buffer::Buffer;
use crate::completions::{CompletionEvent, CompletionState};
use crate::editor::Editor;
use crate::layout::{LayoutNode, SplitDirection};
use crate::lsp::LspIncoming;
use crate::picker::{Picker, PickerEvent};
use crate::text_grid;
use crate::ui;
use crate::ui::floating_panel::{FloatingPanel, PanelEvent, PanelKind, PanelPosition, PANEL_WINDOW_ID};
use crate::ui::hover::extract_hover_text;
use crate::ui::scrollbar::ScrollbarMarker;
use crate::ui::toast::{ToastLevel, ToastManager};
use crate::vim::VimLayer;
use crate::vim::mode::VimMode;

use iced::advanced::subscription::{self, Recipe};
use iced::futures::SinkExt;
use iced::futures::stream::BoxStream;
use iced::keyboard;
use iced::widget::{Space, column, container, row, stack, text};
use iced::{Element, Length, Subscription, Task, Theme, event, window};
use lsp_types::notification::{DidOpenTextDocument, Initialized};
use lsp_types::{DidOpenTextDocumentParams, InitializedParams};
use smol::channel::Receiver;
use tracing::{debug, info};

const SCROLL_SPEED: f32 = 0.8;
const COMPLETION_DEBOUNCE_MS: u64 = 150;

pub struct Remax {
    editor: Editor,
    vim: VimLayer,
    picker: Option<Picker>,
    toast_manager: ToastManager,
    completions: Option<CompletionState>,
    completion_debounce: Option<Instant>,
    floating_panel: Option<FloatingPanel>,
    show_inline_diagnostics: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    KeyEvent {
        key: keyboard::Key,
        modified_key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        text: Option<smol_str::SmolStr>,
    },
    WindowCloseRequested(window::Id),
    ScrollLines {
        delta: f32,
        window_id: usize,
    },
    ScrollCols {
        delta: f32,
        window_id: usize,
    },
    MouseClick {
        x: f32,
        y: f32,
        window_id: usize,
    },
    ViewportResized {
        lines: usize,
        cols: usize,
        window_id: usize,
    },
    Lsp {
        server_id: usize,
        message: String,
    },
    ScrollbarJump {
        line: usize,
        window_id: usize,
    },
    ScrollbarDrag {
        scroll_y: usize,
        window_id: usize,
    },
    ToastTick(Instant),
    DismissToast(usize),
    CompletionTick(Instant),
}

struct LspSubscription {
    server_id: usize,
    rx: Receiver<String>,
}

impl Recipe for LspSubscription {
    type Output = Message;

    fn hash(&self, state: &mut subscription::Hasher) {
        use std::hash::Hash;
        self.server_id.hash(state);
    }

    fn stream(self: Box<Self>, _input: subscription::EventStream) -> BoxStream<'static, Message> {
        let rx = self.rx;
        let server_id = self.server_id;
        Box::pin(iced::stream::channel(100, async move |mut output| {
            while let Ok(message) = rx.recv().await {
                match output.send(Message::Lsp { server_id, message }).await {
                    Ok(_) => {}
                    Err(e) => {
                        debug!(error = ?e, server_id, "LSP subscription output channel closed");
                        break;
                    }
                }
            }
        }))
    }
}
impl Remax {
    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    pub fn boot() -> (Self, Task<Message>) {
        let args = parse_args();
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        info!(?cwd, "editor booted");

        let mut editor = Editor::new(cwd);
        if let Some(file_path) = args.first() {
            let path = std::path::Path::new(file_path);
            debug!(?path, "opening init file from args");
            editor.open_file(path);
        } else {
            debug!("no file args, creating scratch buffer");
            // TODO: assert may not hold up when we start persisting session
            debug_assert!(
                editor.buffers.is_empty(),
                "buffers should technically always be empty on init"
            );
            let scratch_buf = create_scratch_buffer();
            editor.buffers.push(scratch_buf);
            // TODO: we should abstract around the buffer list
            // so we never have to worry about this -
            // if we create a buffer, it should have it's own syntax by default - or None
            editor.syntax_states.push(None);
        }

        (
            Self {
                editor,
                vim: VimLayer::new(),
                picker: None,
                toast_manager: ToastManager::new(),
                completions: None,
                completion_debounce: None,
                floating_panel: None,
                show_inline_diagnostics: true,
            },
            Task::none(),
        )
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            event::listen_with(|event, _status, _id| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    modified_key,
                    modifiers,
                    text,
                    ..
                }) => Some(Message::KeyEvent {
                    key,
                    modified_key,
                    modifiers,
                    text,
                }),
                _ => None,
            }),
            window::close_requests().map(Message::WindowCloseRequested),
        ];

        for workspace in &self.editor.workspaces {
            for server in workspace.lsp_manager.servers.iter() {
                subs.push(subscription::from_recipe(LspSubscription {
                    server_id: server.id,
                    rx: server.rx.clone(),
                }))
            }
        }

        if self.toast_manager.has_active_toasts() {
            subs.push(
                iced::time::every(Duration::from_millis(16))
                    .map(|_| Message::ToastTick(Instant::now())),
            );
        }

        if self.completion_debounce.is_some() {
            subs.push(
                iced::time::every(Duration::from_millis(16))
                    .map(|_| Message::CompletionTick(Instant::now())),
            );
        }

        Subscription::batch(subs)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowCloseRequested(_id) => {
                if self.editor.has_unsaved_changes() {
                    self.editor.status_message =
                        String::from("Unsaved changes! Use :qa! to force quit, or :wq to save");
                } else {
                    self.editor.status_message =
                        String::from("Use :qa! to quit, or :q to close current window");
                }
                return Task::none();
            }
            Message::KeyEvent {
                key,
                modified_key,
                modifiers,
                text,
            } => {
                // 1. Panel focused → panel handles ALL keys
                if let Some(panel) = &mut self.floating_panel
                    && panel.focused
                {
                    let event = panel.panel_buffer.handle_key(
                        &key,
                        &modified_key,
                        &modifiers,
                        text.as_deref(),
                    );
                    match event {
                        PanelEvent::Dismiss => {
                            self.floating_panel = None;
                        }
                        PanelEvent::YankToRegister(t) => {
                            self.editor.registers.unnamed = t.clone();
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                let _ = clipboard.set_text(t);
                            }
                        }
                        PanelEvent::Noop => {}
                    }
                    return Task::none();
                }

                // 2. Picker hijacks
                if self.picker.is_some() {
                    return self.handle_picker_key(&key, &modifiers, text.as_deref());
                }

                // 3. Completions hijacks
                if self.completions.is_some() {
                    let intercept = Self::is_completion_key(&key, &modifiers);
                    if intercept {
                        return self.handle_completion_key(&key, &modifiers);
                    }
                }

                debug!(
                    mode = %self.vim.mode(),
                    ?key,
                    ?modified_key,
                    ?modifiers,
                    ?text,
                    "editor key event"
                );

                // 4. Vim processes the key
                let actions = {
                    let buffer = self.editor.buffer();
                    let cursor = self.editor.cursor();
                    self.vim.handle_key(
                        &key,
                        &modified_key,
                        &modifiers,
                        text.as_deref(),
                        buffer,
                        cursor,
                    )
                };

                // 5. Panel unfocused → check for trigger re-press or dismiss
                if self.floating_panel.is_some() {
                    if actions.is_empty() {
                        // Pending multi-key sequence (e.g. 'g' in 'gt'), wait
                        return Task::none();
                    }

                    let panel_kind = self.floating_panel.as_ref().unwrap().kind;
                    let is_retrigger = actions.iter().any(|a| matches!(
                        (a, panel_kind),
                        (EditorAction::LspHover, PanelKind::LspHover)
                            | (EditorAction::ShowDiagnosticUnderCursor, PanelKind::Diagnostic)
                    ));

                    if is_retrigger {
                        self.floating_panel.as_mut().unwrap().focus();
                        return Task::none();
                    }

                    // Dismiss and fall through to process actions normally
                    self.floating_panel = None;
                }

                // 6. Process actions normally
                for action in &actions {
                    match action {
                        EditorAction::OpenBufferPicker
                        | EditorAction::OpenFilePicker { .. }
                        | EditorAction::OpenDiagnosticsPicker => {
                            self.completions = None;
                            self.open_picker(action.clone());
                            return Task::none();
                        }
                        EditorAction::ShowDiagnosticUnderCursor => {
                            self.show_diagnostic_hover();
                            return Task::none();
                        }
                        EditorAction::ToggleInlineDiagnostics => {
                            self.show_inline_diagnostics = !self.show_inline_diagnostics;
                            let state = if self.show_inline_diagnostics {
                                "on"
                            } else {
                                "off"
                            };
                            self.editor.status_message =
                                format!("Inline diagnostics: {}", state);
                            return Task::none();
                        }
                        action => match self.editor.execute(action.clone()) {
                            EditorEffect::Task(t) => {
                                self.editor.update_search_cache();
                                self.editor.ensure_cursor_visible();
                                return t;
                            }
                            EditorEffect::None => {}
                        },
                    }
                }

                self.finalize_completions(&actions);

                self.editor.update_search_cache();
                self.editor.ensure_cursor_visible();
                self.editor.ensure_syntax_current();
            }
            Message::ScrollLines { delta, window_id } => {
                if window_id == PANEL_WINDOW_ID {
                    if let Some(panel) = &mut self.floating_panel {
                        let total = panel.panel_buffer.buffer.total_lines();
                        panel.panel_buffer.window.scroll_lines(delta, SCROLL_SPEED, total);
                    }
                } else {
                    let ws = self.editor.workspace_mut();
                    if window_id < ws.windows.len() {
                        let buf_id = ws.windows[window_id].buffer_id;
                        let old_scroll = ws.windows[window_id].scroll_y;
                        let cursor = ws.windows[window_id].cursor;
                        let total = self.editor.buffers[buf_id].total_lines();
                        let (cur_line, cur_col) =
                            self.editor.buffers[buf_id].cursor_position(cursor);

                        let ws = self.editor.workspace_mut();
                        ws.windows[window_id].scroll_lines(delta, SCROLL_SPEED, total);
                        let new_scroll = ws.windows[window_id].scroll_y;
                        let scroll_delta = new_scroll as isize - old_scroll as isize;
                        if scroll_delta != 0 {
                            let new_line = (cur_line as isize + scroll_delta).max(0) as usize;
                            let new_cursor = self.editor.buffers[buf_id]
                                .cursor_from_position(new_line, cur_col);
                            self.editor.workspace_mut().windows[window_id].cursor = new_cursor;
                        }
                    }
                }
            }
            Message::ScrollCols { delta, window_id } => {
                if window_id == PANEL_WINDOW_ID {
                    if let Some(panel) = &mut self.floating_panel {
                        let max_len = panel.panel_buffer.buffer.max_line_len();
                        panel
                            .panel_buffer
                            .window
                            .scroll_cols(delta, SCROLL_SPEED, max_len);
                    }
                } else {
                    let ws = self.editor.workspace_mut();
                    if window_id < ws.windows.len() {
                        let buf_id = ws.windows[window_id].buffer_id;
                        let max_len = self.editor.buffers[buf_id].max_line_len();
                        self.editor.workspace_mut().windows[window_id].scroll_cols(
                            delta,
                            SCROLL_SPEED,
                            max_len,
                        );
                    }
                }
            }
            Message::ScrollbarJump { line, window_id } => {
                if window_id == PANEL_WINDOW_ID {
                    if let Some(panel) = &mut self.floating_panel {
                        let new_cursor =
                            panel.panel_buffer.buffer.cursor_from_position(line, 0);
                        panel.panel_buffer.window.cursor = new_cursor;
                        panel
                            .panel_buffer
                            .window
                            .ensure_cursor_visible(&panel.panel_buffer.buffer);
                    }
                } else {
                    let ws = self.editor.workspace_mut();
                    ws.active_window = window_id;
                    if window_id < ws.windows.len() {
                        let buf_id = ws.windows[window_id].buffer_id;
                        let (_, cur_col) = self.editor.buffers[buf_id]
                            .cursor_position(self.editor.workspace().windows[window_id].cursor);
                        let new_cursor =
                            self.editor.buffers[buf_id].cursor_from_position(line, cur_col);
                        self.editor.workspace_mut().windows[window_id].cursor = new_cursor;
                        self.editor.ensure_cursor_visible();
                    }
                }
            }
            Message::ScrollbarDrag {
                scroll_y,
                window_id,
            } => {
                if window_id == PANEL_WINDOW_ID {
                    if let Some(panel) = &mut self.floating_panel {
                        let total = panel.panel_buffer.buffer.total_lines();
                        let max_scroll = total.saturating_sub(1);
                        panel.panel_buffer.window.scroll_y = scroll_y.min(max_scroll);
                    }
                } else {
                    let ws = self.editor.workspace_mut();
                    if window_id < ws.windows.len() {
                        let buf_id = ws.windows[window_id].buffer_id;
                        let total = self.editor.buffers[buf_id].total_lines();
                        let max_scroll = total.saturating_sub(1);
                        self.editor.workspace_mut().windows[window_id].scroll_y =
                            scroll_y.min(max_scroll);
                    }
                }
            }
            Message::MouseClick { x, y, window_id } => {
                if window_id == PANEL_WINDOW_ID {
                    if let Some(panel) = &mut self.floating_panel {
                        let win = &panel.panel_buffer.window;
                        let line =
                            win.scroll_y + (y / text_grid::LINE_HEIGHT) as usize;
                        let col = win.scroll_x
                            + ((x - text_grid::GUTTER_WIDTH - 8.0).max(0.0)
                                / text_grid::CHAR_WIDTH) as usize;
                        let new_cursor =
                            panel.panel_buffer.buffer.cursor_from_position(line, col);
                        panel.panel_buffer.window.cursor = new_cursor;
                        panel
                            .panel_buffer
                            .window
                            .ensure_cursor_visible(&panel.panel_buffer.buffer);
                        // Auto-focus the panel on click
                        panel.focused = true;
                    }
                } else {
                    let ws = self.editor.workspace_mut();
                    ws.active_window = window_id;
                    if window_id < ws.windows.len() {
                        let win = &ws.windows[window_id];
                        let line = win.scroll_y + (y / text_grid::LINE_HEIGHT) as usize;
                        let col = win.scroll_x
                            + ((x - text_grid::GUTTER_WIDTH - 8.0).max(0.0)
                                / text_grid::CHAR_WIDTH)
                                as usize;
                        let buf_id = win.buffer_id;
                        let new_cursor =
                            self.editor.buffers[buf_id].cursor_from_position(line, col);
                        self.editor.workspace_mut().windows[window_id].cursor = new_cursor;
                        self.editor.ensure_cursor_visible();
                    }
                }
            }
            Message::ViewportResized {
                lines,
                cols,
                window_id,
            } => {
                if window_id == PANEL_WINDOW_ID {
                    if let Some(panel) = &mut self.floating_panel {
                        let win = &mut panel.panel_buffer.window;
                        win.visible_lines = lines;
                        win.visible_cols = cols;
                    }
                } else {
                    let mut resized = false;
                    {
                        let ws = self.editor.workspace_mut();
                        if window_id < ws.windows.len() {
                            let win = &mut ws.windows[window_id];
                            if win.visible_lines != lines || win.visible_cols != cols {
                                win.visible_lines = lines;
                                win.visible_cols = cols;
                                resized = true;
                            }
                        }
                    }
                    if resized {
                        self.editor.ensure_cursor_visible();
                    }
                }
            }
            Message::CompletionTick(now) => {
                if let Some(debounce_start) = self.completion_debounce
                    && now.duration_since(debounce_start).as_millis()
                        >= COMPLETION_DEBOUNCE_MS as u128
                {
                    self.completion_debounce = None;
                    self.fire_completion_request();
                }
            }
            Message::ToastTick(now) => {
                self.toast_manager.tick(now);
            }
            Message::DismissToast(id) => {
                self.toast_manager.dismiss(id);
            }
            Message::Lsp { server_id, message } => {
                if let Some(incoming) = self.editor.workspace().lsp_manager.handle_message(&message)
                {
                    match incoming {
                        LspIncoming::Response { id, result } => {
                            debug!(id, "got lsp response");
                            if let Some(pending) = self
                                .editor
                                .workspace_mut()
                                .lsp_manager
                                .take_pending_request(id)
                            {
                                match pending.method.as_str() {
                                    "initialize" => {
                                        debug!(?result, "server initialized");
                                        let capabilities: lsp_types::ServerCapabilities =
                                            serde_json::from_value(result["capabilities"].clone())
                                                .unwrap();
                                        debug!(?capabilities, "server capabilities");
                                        if let Some(server) = self
                                            .editor
                                            .workspace_mut()
                                            .lsp_manager
                                            .servers
                                            .iter_mut()
                                            .find(|s| s.id == server_id)
                                        {
                                            server.capabilities = Some(capabilities);

                                            if server
                                                .send_notification::<Initialized>(
                                                    InitializedParams {},
                                                )
                                                .is_ok()
                                            {
                                                server.initialized = true;
                                                self.toast_manager.push(
                                                    format!(
                                                        "{} LSP Ready",
                                                        server.language.display_name()
                                                    ),
                                                    None,
                                                    ToastLevel::Success,
                                                    Duration::from_secs(3),
                                                );
                                                let server_lang = server.language;

                                                for buf in &self.editor.buffers {
                                                    if buf.language() != Some(server_lang) {
                                                        continue;
                                                    }
                                                    if let Some(file_path) = buf.file_path()
                                                        && let Some(uri) = crate::lsp::path_to_uri(
                                                            Path::new(file_path),
                                                        )
                                                    {
                                                        let text = buf.rope().to_string();
                                                        if let Some(server) = self
                                                            .editor
                                                            .workspace()
                                                            .lsp_manager
                                                            .get_inited_server_for_language(
                                                                server_lang,
                                                            )
                                                        {
                                                            server.send_notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
                                                                text_document: lsp_types::TextDocumentItem {
                                                                    uri,
                                                                    language_id: server_lang.id().to_string(),
                                                                    version: buf.version() as i32,
                                                                    text,
                                                                },
                                                            }).ok();
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    "textDocument/definition" => {
                                        debug!(?result, "definition response");
                                        self.handle_lsp_locations(result, "Definition");
                                    }
                                    "textDocument/references" => {
                                        debug!(?result, "references response");
                                        self.handle_lsp_locations(result, "References");
                                    }
                                    "textDocument/implementation" => {
                                        debug!(?result, "implementation response");
                                        self.handle_lsp_locations(result, "Implementation");
                                    }
                                    "textDocument/declaration" => {
                                        debug!(?result, "declaration response");
                                        self.handle_lsp_locations(result, "Declaration");
                                    }
                                    "textDocument/completion" => {
                                        debug!("completion response received");
                                        self.handle_completion_response(result);
                                    }
                                    "textDocument/hover" => {
                                        debug!(?result, "hover response received");
                                        self.handle_hover_response(result);
                                    }
                                    _ => {
                                        debug!(method = %pending.method, "response matched pending request");
                                    }
                                }
                            }
                        }
                        LspIncoming::Notification { method, params } => {
                            debug!(?method, ?params, "got notification");
                            if method == "textDocument/publishDiagnostics"
                                && let Some((buf_id, diags)) =
                                    crate::diagnostics::parse_publish_diagnostics(
                                        &params,
                                        &self.editor.buffers,
                                    )
                            {
                                debug!(buf_id, count = diags.len(), "setting diagnostics");
                                self.editor.diagnostics.set_for_buffer(buf_id, diags);
                            }
                        }
                        LspIncoming::ServerRequest { id, method, params } => {
                            debug!(?id, ?method, ?params, "got server request");
                        }
                        LspIncoming::Error { id, error } => {
                            debug!(id, ?error, "got error response");
                            let error_msg = error
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown error")
                                .to_string();
                            self.toast_manager.push(
                                "LSP Error",
                                Some(error_msg),
                                ToastLevel::Error,
                                Duration::from_secs(5),
                            );
                        }
                    }
                }
                debug!(server_id, message, "LSP message received in update");
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let ws = self.editor.workspace();
        let active_win_id = ws.active_window;

        let editor_area = self.build_layout_view(&ws.layout, ws, active_win_id);

        let active_buf = self.editor.buffer();
        let active_win = ws.window();
        let (cursor_line, cursor_col) = active_buf.cursor_position(active_win.cursor);

        // Show panel's vim mode when panel is focused
        let panel_focused = self
            .floating_panel
            .as_ref()
            .is_some_and(|p| p.focused);
        let (mr, mg, mb) = if panel_focused {
            self.floating_panel.as_ref().unwrap().panel_buffer.vim.mode_color()
        } else {
            self.vim.mode_color()
        };
        let mode_display = if panel_focused {
            self.floating_panel.as_ref().unwrap().panel_buffer.vim.mode()
        } else {
            self.vim.mode()
        };
        let mode_label = text(format!(" {} ", mode_display))
            .size(14)
            .color(iced::Color::from_rgb(mr, mg, mb));

        let modified_indicator = if active_buf.is_modified() { "[+]" } else { "" };

        let buffer_name = text(format!(" {} {}", active_buf.name(), modified_indicator))
            .size(14)
            .color(iced::Color::from_rgb(0.8, 0.8, 0.8));

        let position = text(format!(" {}:{} ", cursor_line + 1, cursor_col + 1))
            .size(14)
            .color(iced::Color::from_rgb(0.6, 0.6, 0.6));

        let mut modeline_row = row![mode_label, buffer_name, Space::new().width(Length::Fill),]
            .align_y(iced::Alignment::Center);

        let active_buf_id = active_win.buffer_id;
        let (error_count, warning_count) = self.editor.diagnostics.counts_for_buffer(active_buf_id);
        if error_count > 0 || warning_count > 0 {
            let mut diag_parts = row![].align_y(iced::Alignment::Center);
            if error_count > 0 {
                diag_parts = diag_parts.push(
                    text(format!("E:{} ", error_count))
                        .size(13)
                        .color(iced::Color::from_rgb(1.0, 0.3, 0.3)),
                );
            }
            if warning_count > 0 {
                diag_parts = diag_parts.push(
                    text(format!("W:{} ", warning_count))
                        .size(13)
                        .color(iced::Color::from_rgb(1.0, 0.8, 0.2)),
                );
            }
            modeline_row = modeline_row.push(diag_parts);
        }

        for (lang_name, initialized) in ws.lsp_manager.server_statuses() {
            let dot_color = if initialized {
                iced::Color::from_rgb(0.3, 0.8, 0.3)
            } else {
                iced::Color::from_rgb(0.8, 0.7, 0.2)
            };
            let indicator = row![
                text("●").size(10).color(dot_color),
                text(format!(" {} ", lang_name))
                    .size(13)
                    .color(iced::Color::from_rgb(0.6, 0.6, 0.6)),
            ]
            .align_y(iced::Alignment::Center);
            modeline_row = modeline_row.push(indicator);
        }

        modeline_row = modeline_row.push(position);

        let sb_w = ui::scrollbar::SCROLLBAR_WIDTH;
        let modeline = container(modeline_row)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.15, 0.15, 0.2,
                ))),
                ..Default::default()
            })
            .width(Length::Fill)
            .padding(iced::Padding {
                top: 2.0,
                bottom: 2.0,
                left: 0.0,
                right: sb_w,
            });

        let bottom_section: Element<'_, Message> = if let Some(picker) = &self.picker {
            picker.view()
        } else {
            let cmdline_text = if panel_focused {
                self.floating_panel
                    .as_ref()
                    .unwrap()
                    .panel_buffer
                    .vim
                    .status_line_override()
                    .unwrap_or_else(|| self.editor.status_message.clone())
            } else {
                self.vim
                    .status_line_override()
                    .unwrap_or_else(|| self.editor.status_message.clone())
            };

            container(
                text(format!(" {}", cmdline_text))
                    .size(14)
                    .color(iced::Color::from_rgb(0.8, 0.8, 0.8)),
            )
            .width(Length::Fill)
            .padding([2, 0])
            .into()
        };

        let content = column![editor_area, modeline, bottom_section];

        let main_ui = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.1, 0.1, 0.12,
                ))),
                ..Default::default()
            });

        let mut layers = stack![main_ui];

        if let Some(comp) = &self.completions {
            let active_win = ws.window();
            let active_buf = self.editor.buffer();
            let (cursor_line, cursor_col) = active_buf.cursor_position(active_win.cursor);

            let pixel_x = text_grid::GUTTER_WIDTH
                + 8.0
                + (cursor_col as isize - active_win.scroll_x as isize).max(0) as f32
                    * text_grid::CHAR_WIDTH;
            let pixel_y = ((cursor_line as isize - active_win.scroll_y as isize + 1).max(0) as f32)
                * text_grid::LINE_HEIGHT;

            let popup_height = comp.visible_count() as f32 * text_grid::LINE_HEIGHT;
            let viewport_height = active_win.visible_lines as f32 * text_grid::LINE_HEIGHT;

            // Flip above cursor if near bottom
            let final_y = if pixel_y + popup_height > viewport_height {
                ((cursor_line as isize - active_win.scroll_y as isize) as f32
                    * text_grid::LINE_HEIGHT
                    - popup_height)
                    .max(0.0)
            } else {
                pixel_y
            };

            let overlay = container(column![
                Space::new().height(Length::Fixed(final_y)),
                row![Space::new().width(Length::Fixed(pixel_x)), comp.view()]
            ])
            .width(Length::Fill)
            .height(Length::Fill);

            layers = layers.push(overlay);
        }

        if let Some(panel) = &self.floating_panel {
            let viewport_width = active_win.visible_cols as f32 * text_grid::CHAR_WIDTH
                + text_grid::GUTTER_WIDTH
                + 8.0;
            let viewport_height = active_win.visible_lines as f32 * text_grid::LINE_HEIGHT;
            layers = layers.push(panel.view(viewport_width, viewport_height));
        }

        layers = layers.push(self.toast_manager.view());
        layers.into()
    }

    fn is_completion_key(key: &keyboard::Key, modifiers: &keyboard::Modifiers) -> bool {
        use iced::keyboard::Key;
        use iced::keyboard::key::Named;
        match key {
            Key::Named(Named::Escape)
            | Key::Named(Named::Enter)
            | Key::Named(Named::Tab)
            | Key::Named(Named::ArrowDown)
            | Key::Named(Named::ArrowUp) => true,
            Key::Character(ch) if modifiers.control() => {
                matches!(ch.as_str(), "n" | "p")
            }
            _ => false,
        }
    }

    fn handle_completion_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
    ) -> Task<Message> {
        let event = if let Some(comp) = self.completions.as_mut() {
            comp.handle_key(key, modifiers)
        } else {
            return Task::none();
        };
        match event {
            CompletionEvent::Accept {
                insert_text,
                prefix_len,
            } => {
                self.completions = None;
                self.completion_debounce = None;
                self.editor.execute(EditorAction::LspApplyCompletion {
                    delete_backward: prefix_len,
                    insert_text,
                });
                self.editor.ensure_cursor_visible();
                self.editor.ensure_syntax_current();
            }
            CompletionEvent::Dismiss => {
                self.completions = None;
                self.completion_debounce = None;
            }
            CompletionEvent::Noop => {}
        }
        Task::none()
    }

    fn finalize_completions(&mut self, actions: &[EditorAction]) {
        if self.vim.mode() != VimMode::Insert {
            self.completions = None;
            self.completion_debounce = None;
            return;
        }

        for action in actions {
            match action {
                EditorAction::InsertChar(ch) => {
                    if let Some(comp) = self.completions.as_mut() {
                        comp.push_char(*ch);
                        if comp.is_empty() {
                            self.completions = None;
                        }
                    } else if self.should_trigger_completion(*ch) {
                        self.completion_debounce = Some(Instant::now());
                    }
                }
                EditorAction::DeleteCharBackward => {
                    if let Some(comp) = self.completions.as_mut()
                        && (!comp.pop_char() || comp.prefix_len() == 0 || comp.is_empty())
                    {
                        self.completions = None;
                    }
                }
                _ => {}
            }
        }
    }

    fn should_trigger_completion(&self, ch: char) -> bool {
        if ch.is_alphanumeric() || ch == '_' {
            return true;
        }
        if let Some(server) = self
            .editor
            .workspace()
            .lsp_manager
            .servers
            .iter()
            .find(|s| s.initialized)
            && let Some(caps) = &server.capabilities
            && let Some(provider) = &caps.completion_provider
            && let Some(triggers) = &provider.trigger_characters
        {
            let ch_str = ch.to_string();
            return triggers.contains(&ch_str);
        }
        false
    }

    fn fire_completion_request(&mut self) {
        let ws_idx = self.editor.active_workspace;
        let win = self.editor.workspaces[ws_idx].window();
        let cursor = win.cursor;
        let buf_id = win.buffer_id;
        let buf = &self.editor.buffers[buf_id];
        self.editor.workspaces[ws_idx]
            .lsp_manager
            .request_completions(buf, cursor);
    }

    fn handle_hover_response(&mut self, result: serde_json::Value) {
        if result.is_null() {
            self.editor.status_message = String::from("No hover info");
            return;
        }
        let ws = self.editor.workspace();
        let win = ws.window();
        let buf = &self.editor.buffers[win.buffer_id];
        let (cursor_line, cursor_col) = buf.cursor_position(win.cursor);

        let lines = extract_hover_text(&result);
        let content = lines.join("\n");

        self.floating_panel = Some(FloatingPanel::new(
            &content,
            "Hover",
            PanelKind::LspHover,
            PanelPosition::AtCursor {
                line: cursor_line,
                col: cursor_col,
                scroll_y: win.scroll_y,
                scroll_x: win.scroll_x,
            },
        ));
    }

    fn show_diagnostic_hover(&mut self) {
        let ws = self.editor.workspace();
        let win = ws.window();
        let buf_id = win.buffer_id;
        let buf = &self.editor.buffers[buf_id];
        let (cursor_line, cursor_col) = buf.cursor_position(win.cursor);

        let diags = self.editor.diagnostics.get_for_buffer(buf_id);
        let line_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.line == cursor_line)
            .collect();

        if line_diags.is_empty() {
            self.editor.status_message = String::from("No diagnostics on this line");
            return;
        }

        // Format diagnostics as plain text for the panel buffer
        let mut text_lines = Vec::new();
        for d in &line_diags {
            let icon = match d.severity {
                crate::diagnostics::Severity::Error => "E",
                crate::diagnostics::Severity::Warning => "W",
                crate::diagnostics::Severity::Info => "I",
                crate::diagnostics::Severity::Hint => "H",
            };
            for (i, msg_line) in d.message.lines().enumerate() {
                if i == 0 {
                    text_lines.push(format!("[{}] {}", icon, msg_line));
                } else {
                    text_lines.push(format!("    {}", msg_line));
                }
            }
            if let Some(src) = &d.source {
                text_lines.push(format!("    [{}]", src));
            }
        }
        let content = text_lines.join("\n");

        self.floating_panel = Some(FloatingPanel::new(
            &content,
            "Diagnostics",
            PanelKind::Diagnostic,
            PanelPosition::AtCursor {
                line: cursor_line,
                col: cursor_col,
                scroll_y: win.scroll_y,
                scroll_x: win.scroll_x,
            },
        ));
    }

    fn handle_completion_response(&mut self, result: serde_json::Value) {
        // Only show completions if we're still in insert mode
        if self.vim.mode() != VimMode::Insert {
            return;
        }

        let cursor = self.editor.cursor();
        let buf = self.editor.buffer();

        // Compute prefix: text from last non-identifier char to cursor
        let rope = buf.rope();
        let line = rope.char_to_line(cursor);
        let line_start = rope.line_to_char(line);
        let line_text: String = rope.slice(line_start..cursor).chars().collect();

        let prefix_start = line_text
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| i + 1)
            .unwrap_or(0);
        let prefix = &line_text[prefix_start..];
        let trigger_offset = line_start + prefix_start;

        if let Some(state) = crate::completions::parse_lsp_response(&result, trigger_offset, prefix)
        {
            self.completions = Some(state);
        } else {
            self.completions = None;
        }
    }

    fn handle_lsp_locations(&mut self, result: serde_json::Value, label: &str) {
        let cwd = self.editor.workspace().cwd.clone();
        let mut locations = crate::picker::sources::parse_lsp_locations(&result);
        // FIXME: Don't match on label
        // we do this because if we try to reference something like Option<> this returns 1000's
        // of references which is very slow and not a good UX. It's a rustanalyzer thing, happens in VSCode too
        if label == "References" {
            locations.retain(|loc| Path::new(loc.uri.path().as_str()).starts_with(&cwd));
        }

        match locations.len() {
            0 => {
                self.editor.status_message = format!("No {} found", label);
            }
            1 => {
                let loc = &locations[0];
                self.editor.status_message = format!(
                    "{}: {}:{}:{}",
                    label,
                    loc.uri.path(),
                    loc.range.start.line + 1,
                    loc.range.start.character + 1,
                );
                let path = PathBuf::from(loc.uri.path().to_string());
                let line = loc.range.start.line as usize;
                let col = loc.range.start.character as usize;
                self.editor
                    .execute(EditorAction::OpenFileAtPosition { path, line, col });
                self.editor.ensure_cursor_visible();
            }
            _ => {
                locations.sort_by(|a, b| {
                    a.uri
                        .path()
                        .as_str()
                        .cmp(b.uri.path().as_str())
                        .then(a.range.start.line.cmp(&b.range.start.line))
                });
                let restore = Some(self.editor.workspace().window().buffer_id);
                self.picker = Some(crate::picker::sources::lsp_location_picker(
                    &locations,
                    label,
                    &cwd,
                    &self.editor.buffers,
                    &self.editor.syntax_states,
                    &self.editor.loader,
                    restore,
                ));
            }
        }
    }

    fn open_picker(&mut self, action: EditorAction) {
        let restore = Some(self.editor.workspace().window().buffer_id);
        let cwd = self.editor.workspace().cwd.clone();
        match action {
            EditorAction::OpenBufferPicker => {
                self.picker = Some(crate::picker::sources::buffer_picker(
                    &self.editor.buffers,
                    &cwd,
                    restore,
                ));
                // Preview the first selected buffer
                if let Some(picker) = &self.picker
                    && let Some(item) = picker.selected_item()
                    && let Some(action) = item.preview_action.clone()
                {
                    self.editor.execute(action);
                }
            }
            EditorAction::OpenFilePicker {
                show_ignored,
                max_results,
            } => {
                self.picker = Some(crate::picker::sources::file_picker(
                    &cwd,
                    show_ignored,
                    max_results,
                    restore,
                ));
            }
            EditorAction::OpenDiagnosticsPicker => {
                self.picker = Some(crate::picker::sources::diagnostics_picker(
                    &self.editor.buffers,
                    &self.editor.diagnostics,
                    &cwd,
                    restore,
                ));
            }
            _ => {}
        }
    }

    fn handle_picker_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        text: Option<&str>,
    ) -> Task<Message> {
        let event = match self.picker.as_mut() {
            Some(picker) => picker.handle_key(key, modifiers, text),
            None => return Task::none(),
        };
        match event {
            PickerEvent::Select(action) => {
                self.picker = None;
                self.editor.execute(action);
                self.editor.ensure_cursor_visible();
            }
            PickerEvent::Cancel(restore) => {
                if let Some(buf_id) = restore {
                    let len_chars = self.editor.buffers[buf_id].len_chars();
                    self.editor.workspace_mut().switch_buffer(buf_id, len_chars);
                }
                self.picker = None;
            }
            PickerEvent::PreviewChanged(Some(action)) => {
                self.editor.execute(action);
            }
            PickerEvent::PreviewChanged(None) | PickerEvent::Noop => {}
        }
        Task::none()
    }

    fn build_scrollbar_markers(
        &self,
        win: &crate::window::Window,
        buffer: &Buffer,
    ) -> Vec<ScrollbarMarker> {
        let mut markers = Vec::new();
        for &char_pos in &win.search_matches {
            if char_pos < buffer.len_chars() {
                markers.push(ScrollbarMarker {
                    line: buffer.char_to_line(char_pos),
                    color: iced::Color::from_rgba(0.9, 0.7, 0.2, 0.9),
                });
            }
        }
        // Add diagnostic markers
        for diag in self.editor.diagnostics.get_for_buffer(win.buffer_id) {
            let color = match diag.severity {
                crate::diagnostics::Severity::Error => iced::Color::from_rgba(1.0, 0.3, 0.3, 0.9),
                crate::diagnostics::Severity::Warning => iced::Color::from_rgba(1.0, 0.8, 0.2, 0.9),
                _ => iced::Color::from_rgba(0.3, 0.7, 1.0, 0.7),
            };
            markers.push(ScrollbarMarker {
                line: diag.line,
                color,
            });
        }
        markers
    }

    fn build_layout_view<'a>(
        &'a self,
        node: &LayoutNode,
        ws: &'a crate::workspace::Workspace,
        active_win_id: usize,
    ) -> Element<'a, Message> {
        match node {
            LayoutNode::Leaf(win_id) => {
                let win = &ws.windows[*win_id];
                let buffer = &self.editor.buffers[win.buffer_id];
                let is_active = *win_id == active_win_id;

                let highlights =
                    if let Some(Some(state)) = self.editor.syntax_states.get(win.buffer_id) {
                        let rope = buffer.rope();
                        let start_byte = rope.line_to_byte(win.scroll_y) as u32;
                        let end_line = (win.scroll_y + win.visible_lines + 2).min(rope.len_lines());
                        let end_byte = if end_line < rope.len_lines() {
                            rope.line_to_byte(end_line) as u32
                        } else {
                            rope.len_bytes() as u32
                        };
                        state.highlights_for_range(rope, &self.editor.loader, start_byte, end_byte)
                    } else {
                        Vec::new()
                    };

                let visible_diags = self.editor.diagnostics.for_line_range(
                    win.buffer_id,
                    win.scroll_y,
                    win.scroll_y + win.visible_lines + 2,
                );

                let grid = text_grid::text_grid(
                    buffer,
                    win.cursor,
                    win.scroll_y,
                    win.scroll_x,
                    win.selection,
                    &win.search_matches,
                    if is_active {
                        self.editor.search_len()
                    } else {
                        0
                    },
                    *win_id,
                    is_active,
                    highlights,
                    visible_diags,
                    self.show_inline_diagnostics,
                );

                let markers = self.build_scrollbar_markers(win, buffer);
                let sb = ui::scrollbar::scrollbar(
                    win.scroll_y,
                    win.visible_lines,
                    buffer.total_lines(),
                    markers,
                    *win_id,
                    is_active,
                );

                row![container(grid).width(Length::Fill).height(Length::Fill), sb,].into()
            }
            LayoutNode::Split {
                direction,
                children,
                ..
            } => {
                let first = self.build_layout_view(&children[0], ws, active_win_id);
                let second = self.build_layout_view(&children[1], ws, active_win_id);

                let separator_color = iced::Color::from_rgb(0.3, 0.3, 0.35);

                match direction {
                    SplitDirection::Vertical => {
                        let sep = container(Space::new())
                            .width(Length::Fixed(1.0))
                            .height(Length::Fill)
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(iced::Background::Color(separator_color)),
                                ..Default::default()
                            });
                        row![first, sep, second].into()
                    }
                    SplitDirection::Horizontal => {
                        let sep = container(Space::new())
                            .width(Length::Fill)
                            .height(Length::Fixed(1.0))
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(iced::Background::Color(separator_color)),
                                ..Default::default()
                            });
                        column![first, sep, second].into()
                    }
                }
            }
        }
    }
}

fn parse_args() -> Vec<String> {
    let args = std::env::args();
    if args.len() > 1 {
        info!(?args, "launch args");
        args.skip(1).collect()
    } else {
        Vec::new()
    }
}

fn create_scratch_buffer() -> Buffer {
    let mut buffer = Buffer::new();
    buffer.set_name("*scratch*");
    buffer.insert_str(
        0,
        "Welcome to remax.\n\nPress 'i' to enter insert mode.\nPress 'Esc' to return to normal mode.\nUse h/j/k/l to navigate.",
    );
    buffer
}
