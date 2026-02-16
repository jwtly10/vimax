mod picker;
mod view;

use crate::action::{EditorAction, EditorEffect, PickerKind};
use crate::buffer::Buffer;
use crate::editor::Editor;
use crate::picker::Picker;
use crate::vim::VimLayer;

use iced::advanced::subscription::{self, Recipe};
use iced::futures::stream::BoxStream;
use iced::futures::SinkExt;
use iced::keyboard;
use iced::{event, window, Subscription, Task, Theme};
use smol::channel::Receiver;
use tracing::{debug, info};

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
            debug_assert!(
                editor.buffers.is_empty(),
                "buffers should technically always be empty on init"
            );
            let scratch_buf = create_scratch_buffer();
            editor.buffers.push(scratch_buf);
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
                self.editor.scroll_lines(window_id, delta);
            }
            Message::ScrollCols { delta, window_id } => {
                self.editor.scroll_cols(window_id, delta);
            }
            Message::MouseClick { x, y, window_id } => {
                self.editor.handle_click(window_id, x, y);
            }
            Message::ViewportResized {
                lines,
                cols,
                window_id,
            } => {
                self.editor.handle_resize(window_id, lines, cols);
            }
            Message::Lsp { server_id, message } => {
                self.editor.handle_lsp_message(server_id, &message);
            }
        }
        Task::none()
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
