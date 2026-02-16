use std::path::{Path, PathBuf};

use std::time::{Duration, Instant};

use crate::action::{EditorAction, EditorEffect};
use crate::buffer::Buffer;
use crate::editor::Editor;
use crate::layout::{LayoutNode, SplitDirection};
use crate::lsp::LspIncoming;
use crate::picker::{Picker, PickerEvent};
use crate::text_grid;
use crate::ui;
use crate::ui::scrollbar::ScrollbarMarker;
use crate::ui::toast::{ToastLevel, ToastManager};
use crate::vim::VimLayer;

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

const SCROLL_SPEED: f32 = 0.5;

pub struct Remax {
    editor: Editor,
    vim: VimLayer,
    picker: Option<Picker>,
    toast_manager: ToastManager,
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
                if self.picker.is_some() {
                    return self.handle_picker_key(&key, &modifiers, text.as_deref());
                }

                debug!(
                    mode = %self.vim.mode(),
                    ?key,
                    ?modified_key,
                    ?modifiers,
                    ?text,
                    "key event"
                );

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

                for action in actions {
                    match action {
                        EditorAction::OpenBufferPicker | EditorAction::OpenFilePicker { .. } => {
                            self.open_picker(action);
                            return Task::none();
                        }
                        action => match self.editor.execute(action) {
                            EditorEffect::Task(t) => {
                                self.editor.update_search_cache();
                                self.editor.ensure_cursor_visible();
                                return t;
                            }
                            EditorEffect::None => {}
                        },
                    }
                }
                self.editor.update_search_cache();
                self.editor.ensure_cursor_visible();
                self.editor.ensure_syntax_current();
            }
            Message::ScrollLines { delta, window_id } => {
                let ws = self.editor.workspace_mut();
                if window_id < ws.windows.len() {
                    let buf_id = ws.windows[window_id].buffer_id;
                    let old_scroll = ws.windows[window_id].scroll_y;
                    let cursor = ws.windows[window_id].cursor;
                    let total = self.editor.buffers[buf_id].total_lines();
                    let (cur_line, cur_col) = self.editor.buffers[buf_id].cursor_position(cursor);

                    let ws = self.editor.workspace_mut();
                    ws.windows[window_id].scroll_lines(delta, SCROLL_SPEED, total);
                    let new_scroll = ws.windows[window_id].scroll_y;
                    let scroll_delta = new_scroll as isize - old_scroll as isize;
                    if scroll_delta != 0 {
                        let new_line = (cur_line as isize + scroll_delta).max(0) as usize;
                        let new_cursor =
                            self.editor.buffers[buf_id].cursor_from_position(new_line, cur_col);
                        self.editor.workspace_mut().windows[window_id].cursor = new_cursor;
                    }
                }
            }
            Message::ScrollCols { delta, window_id } => {
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
            Message::ScrollbarJump { line, window_id } => {
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
            Message::ScrollbarDrag { scroll_y, window_id } => {
                let ws = self.editor.workspace_mut();
                if window_id < ws.windows.len() {
                    let buf_id = ws.windows[window_id].buffer_id;
                    let total = self.editor.buffers[buf_id].total_lines();
                    let max_scroll = total.saturating_sub(1);
                    self.editor.workspace_mut().windows[window_id].scroll_y =
                        scroll_y.min(max_scroll);
                }
            }
            Message::MouseClick { x, y, window_id } => {
                let ws = self.editor.workspace_mut();
                ws.active_window = window_id;
                if window_id < ws.windows.len() {
                    let win = &ws.windows[window_id];
                    let line = win.scroll_y + (y / text_grid::LINE_HEIGHT) as usize;
                    let col = win.scroll_x
                        + ((x - text_grid::GUTTER_WIDTH - 8.0).max(0.0) / text_grid::CHAR_WIDTH)
                            as usize;
                    let buf_id = win.buffer_id;
                    let new_cursor = self.editor.buffers[buf_id].cursor_from_position(line, col);
                    self.editor.workspace_mut().windows[window_id].cursor = new_cursor;
                    self.editor.ensure_cursor_visible();
                }
            }
            Message::ViewportResized {
                lines,
                cols,
                window_id,
            } => {
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
                                    _ => {
                                        debug!(method = %pending.method, "response matched pending request");
                                    }
                                }
                            }
                        }
                        LspIncoming::Notification { method, params } => {
                            debug!(?method, ?params, "got notification");
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

        let (mr, mg, mb) = self.vim.mode_color();
        let mode_label = text(format!(" {} ", self.vim.mode()))
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

        let modeline = container(modeline_row)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.15, 0.15, 0.2,
                ))),
                ..Default::default()
            })
            .width(Length::Fill)
            .padding([2, 0]);

        let bottom_section: Element<'_, Message> = if let Some(picker) = &self.picker {
            picker.view()
        } else {
            let cmdline_text = self
                .vim
                .status_line_override()
                .unwrap_or_else(|| self.editor.status_message.clone());

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

        stack![main_ui, self.toast_manager.view()].into()
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

                row![
                    container(grid).width(Length::Fill).height(Length::Fill),
                    sb,
                ]
                .into()
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
