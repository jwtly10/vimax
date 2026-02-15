use std::path::{Path, PathBuf};

use crate::action::{EditorAction, EditorEffect, PickerKind};
use crate::buffer::Buffer;
use crate::editor::Editor;
use crate::layout::{LayoutNode, SplitDirection};
use crate::lsp::{lsp_position_to_offset, LspIncoming};
use crate::picker::{Location, Picker, PickerItem};
use crate::text_grid;
use crate::vim::VimLayer;

use iced::advanced::subscription::{self, Recipe};
use iced::futures::stream::BoxStream;
use iced::futures::SinkExt;
use iced::keyboard;
use iced::widget::Space;
use iced::widget::{column, container, row, text};
use iced::{event, window, Element, Length, Subscription, Task, Theme};
use lsp_types::notification::{DidOpenTextDocument, Initialized};
use lsp_types::{DidOpenTextDocumentParams, InitializedParams};
use smol::channel::Receiver;
use tracing::{debug, info};

const SCROLL_SPEED: f32 = 0.5;

pub struct Remax {
    editor: Editor,
    vim: VimLayer,
    picker: Option<Picker>,
    active_picker_type: Option<PickerKind>,
    picker_restore_buffer: Option<usize>,
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
                active_picker_type: None,
                picker_restore_buffer: None,
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
                        EditorAction::OpenPicker(kind) => {
                            self.open_picker(kind);
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

        let modeline = container(
            row![
                mode_label,
                buffer_name,
                Space::new().width(Length::Fill),
                position
            ]
            .align_y(iced::Alignment::Center),
        )
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgb(
                0.15, 0.15, 0.2,
            ))),
            ..Default::default()
        })
        .width(Length::Fill)
        .padding([2, 0]);

        let bottom_section: Element<'_, Message> = if let Some(picker) = &self.picker {
            let prompt = container(
                text(format!(" {}", picker.status_line()))
                    .size(14)
                    .color(iced::Color::from_rgb(0.9, 0.9, 0.5)),
            )
            .width(Length::Fill)
            .padding([2, 0]);

            let mut items_col = column![];
            for (i, &item_idx) in picker.visible_items() {
                let item = &picker.items[item_idx];
                let is_selected = i == picker.selected;
                let label = if is_selected {
                    format!(" > {}", item.label)
                } else {
                    format!("   {}", item.label)
                };
                let text_color = if is_selected {
                    iced::Color::from_rgb(1.0, 1.0, 1.0)
                } else {
                    iced::Color::from_rgb(0.6, 0.6, 0.6)
                };
                let row_widget = container(text(label).size(14).color(text_color))
                    .width(Length::Fill)
                    .padding([1, 3]);
                let row_widget = if is_selected {
                    row_widget.style(|_theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgb(
                            0.25, 0.35, 0.5,
                        ))),
                        ..Default::default()
                    })
                } else {
                    row_widget
                };
                items_col = items_col.push(row_widget);
            }

            column![prompt, items_col].into()
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

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.1, 0.1, 0.12,
                ))),
                ..Default::default()
            })
            .into()
    }

    fn parse_lsp_locations(&self, result: &serde_json::Value) -> Vec<lsp_types::Location> {
        if result.is_null() {
            return Vec::new();
        }
        if result.get("uri").is_some() {
            if let Ok(loc) = serde_json::from_value::<lsp_types::Location>(result.clone()) {
                return vec![loc];
            }
        }
        if let Some(arr) = result.as_array() {
            if arr.is_empty() {
                return Vec::new();
            }
            if arr[0].get("targetUri").is_some() {
                return arr
                    .iter()
                    .filter_map(|v| serde_json::from_value::<lsp_types::LocationLink>(v.clone()).ok())
                    .map(|l| lsp_types::Location {
                        uri: l.target_uri,
                        range: l.target_selection_range,
                    })
                    .collect();
            }
            return arr
                .iter()
                .filter_map(|v| serde_json::from_value::<lsp_types::Location>(v.clone()).ok())
                .collect();
        }
        Vec::new()
    }

    fn jump_to_lsp_location(&mut self, loc: &lsp_types::Location) {
        let path_str = loc.uri.path().to_string();
        let path = std::path::Path::new(&path_str);
        self.editor.open_file(path);
        let buf = self.editor.buffer();
        let offset = lsp_position_to_offset(buf.rope(), &loc.range.start);
        self.editor.window_mut().cursor = buf.clamp_cursor(offset);
        self.editor.ensure_cursor_visible();
    }

    fn handle_lsp_locations(&mut self, result: serde_json::Value, label: &str) {
        let cwd = self.editor.workspace().cwd.clone();
        let locations: Vec<_> = self
            .parse_lsp_locations(&result)
            .into_iter()
            .filter(|loc| {
                let path = Path::new(loc.uri.path().as_str());
                path.starts_with(&cwd)
            })
            .collect();
        match locations.len() {
            0 => {
                self.editor.status_message = format!("No {} found", label);
            }
            1 => {
                let loc = &locations[0];
                let path_str = loc.uri.path().to_string();
                self.editor.status_message = format!(
                    "{}: {}:{}:{}",
                    label,
                    path_str,
                    loc.range.start.line + 1,
                    loc.range.start.character + 1,
                );
                self.jump_to_lsp_location(loc);
            }
            _ => {
                let cwd = self.editor.workspace().cwd.clone();
                let items: Vec<PickerItem> = locations
                    .iter()
                    .enumerate()
                    .map(|(i, loc)| {
                        let path = PathBuf::from(loc.uri.path().to_string());
                        let display_path = path
                            .strip_prefix(&cwd)
                            .unwrap_or(&path)
                            .display()
                            .to_string();
                        let line = loc.range.start.line as usize;
                        let col = loc.range.start.character as usize;
                        PickerItem {
                            id: i,
                            label: format!("{}:{}:{}", display_path, line + 1, col + 1),
                            location: Some(Location { path, line, col }),
                        }
                    })
                    .collect();
                self.active_picker_type = Some(PickerKind::Locations);
                self.picker_restore_buffer =
                    Some(self.editor.workspace().window().buffer_id);
                self.picker = Some(Picker::new(label, items));
            }
        }
    }

    fn open_picker(&mut self, kind: PickerKind) {
        self.active_picker_type = Some(kind);
        match kind {
            PickerKind::Buffers => {
                let buf_list = self.editor.buffer_list();
                let items: Vec<PickerItem> = buf_list
                    .into_iter()
                    .map(|(id, label)| PickerItem { id, label, location: None })
                    .collect();
                self.picker_restore_buffer = Some(self.editor.workspace().window().buffer_id);
                self.picker = Some(Picker::new("Buffers", items));
                if !self.picker.as_ref().unwrap().items.is_empty() {
                    self.preview_selected_buffer();
                }
            }
            PickerKind::ProjectFiles {
                show_ignored,
                max_results,
            } => {
                let cwd = &self.editor.workspace().cwd;
                let files = ignore::WalkBuilder::new(cwd)
                    .git_ignore(!show_ignored)
                    .git_exclude(!show_ignored)
                    .filter_entry(|entry| {
                        // TODO: this will be configurable
                        // there are some dirs we just never want to look at
                        let custom_ignores = [".git", "target", "node_modules", "dist", "build"];
                        let file_name = entry.file_name().to_string_lossy();
                        if custom_ignores.contains(&file_name.as_ref()) {
                            return false;
                        }

                        true
                    })
                    .build()
                    .filter_map(|entry| entry.ok())
                    .take(max_results)
                    .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
                    .map(|entry| {
                        let path = entry.path();
                        let label = path.strip_prefix(cwd).unwrap_or(path).display().to_string();
                        debug_assert!(!label.is_empty(), "file label should not be empty");
                        PickerItem { id: 0, label, location: None }
                    })
                    .collect::<Vec<_>>();
                self.picker_restore_buffer = Some(self.editor.workspace().window().buffer_id);
                self.picker = Some(Picker::new("Project Files", files));
            }
            PickerKind::Locations => {
                // Locations picker is opened directly by handle_lsp_locations, not via open_picker
            }
        }
    }

    // TODO: only for some picker implementations do we want to preview
    // things like rg cwd search - we don't
    fn preview_selected_buffer(&mut self) {
        if let Some(picker) = &self.picker
            && let Some(item) = picker.selected_item()
        {
            let buf_id = item.id;
            let len_chars = self.editor.buffers[buf_id].len_chars();
            self.editor.workspace_mut().switch_buffer(buf_id, len_chars);
        }
    }

    fn handle_picker_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        text: Option<&str>,
    ) -> Task<Message> {
        match key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                if let Some(buf_id) = self.picker_restore_buffer.take() {
                    let len_chars = self.editor.buffers[buf_id].len_chars();
                    self.editor.workspace_mut().switch_buffer(buf_id, len_chars);
                }
                self.picker = None;
                self.active_picker_type = None;
            }
            keyboard::Key::Named(keyboard::key::Named::Enter) => {
                if let Some(picker) = &self.picker
                    && let Some(item) = picker.selected_item()
                {
                    match self.active_picker_type {
                        Some(PickerKind::ProjectFiles { .. }) => {
                            self.editor.open_file(&PathBuf::from(&item.label));
                        }
                        Some(PickerKind::Locations) => {
                            if let Some(loc) = &item.location {
                                let path = loc.path.clone();
                                let line = loc.line;
                                let col = loc.col;
                                self.editor.open_file(&path);
                                let buf = self.editor.buffer();
                                let offset = buf.cursor_from_position(line, col);
                                self.editor.window_mut().cursor = buf.clamp_cursor(offset);
                                self.editor.ensure_cursor_visible();
                            }
                        }
                        _ => {}
                    }
                }

                self.picker = None;
                self.active_picker_type = None;
                self.picker_restore_buffer = None;
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                if let Some(picker) = &mut self.picker {
                    picker.move_up();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                if let Some(picker) = &mut self.picker {
                    picker.move_down();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            keyboard::Key::Character(c) if modifiers.control() && c.as_str() == "p" => {
                if let Some(picker) = &mut self.picker {
                    picker.move_up();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            keyboard::Key::Character(c) if modifiers.control() && c.as_str() == "n" => {
                if let Some(picker) = &mut self.picker {
                    picker.move_down();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                if let Some(picker) = &mut self.picker {
                    picker.backspace();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            _ => {
                if let Some(t) = text {
                    if let Some(picker) = &mut self.picker {
                        for ch in t.chars() {
                            if !ch.is_control() {
                                picker.type_char(ch);
                            }
                        }
                    }
                    if let Some(PickerKind::Buffers) = self.active_picker_type {
                        self.preview_selected_buffer();
                    }
                }
            }
        }
        Task::none()
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

                container(grid)
                    .width(Length::Fill)
                    .height(Length::Fill)
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
