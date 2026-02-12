use crate::action::EditorEffect;
use crate::buffer::Buffer;
use crate::editor::Editor;
use crate::text_grid;
use crate::vim::VimLayer;
use crate::window::WindowView;

use iced::keyboard;
use iced::widget::Space;
use iced::widget::{column, container, row, text};
use iced::{Element, Length, Subscription, Task, Theme, event, window};
use tracing::{debug, info};

const SCROLL_SPEED: f32 = 0.3;

pub struct Remax {
    editor: Editor,
    vim: VimLayer,
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
    ScrollLines(f32),
    ScrollCols(f32),
    MouseClick {
        x: f32,
        y: f32,
    },
    ViewportResized {
        lines: usize,
        cols: usize,
    },
}

impl Remax {
    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    pub fn boot() -> (Self, Task<Message>) {
        let args = parse_args();
        let buffer = if let Some(file_path) = args.first() {
            let path = std::path::Path::new(file_path);
            let buf_name = path.file_name().unwrap_or_default().to_string_lossy();
            debug!(?path, "attempting to read file into buffer");
            match std::fs::read_to_string(path) {
                Ok(content) => Buffer::from_str(&content, &buf_name, path, false),
                Err(e) => {
                    debug!(?e, "failed to read file, starting with empty buffer");
                    create_scratch_buffer()
                }
            }
        } else {
            debug!("no file path provided, starting with empty buffer");
            create_scratch_buffer()
        };

        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        info!(?cwd, "editor booted");
        (
            Self {
                editor: Editor::new(buffer, cwd),
                vim: VimLayer::new(),
            },
            Task::none(),
        )
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
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
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowCloseRequested(id) => {
                if self.editor.buffer().is_modified() {
                    self.editor.status_message =
                        String::from("Unsaved changes! Use :q! to force quit");
                    return Task::none();
                }
                return window::close(id);
            }
            Message::KeyEvent {
                key,
                modified_key,
                modifiers,
                text,
            } => {
                debug!(
                    mode = %self.vim.mode(),
                    ?key,
                    ?modified_key,
                    ?modifiers,
                    ?text,
                    "key event"
                );

                let actions = {
                    let view = WindowView {
                        buffer: self.editor.buffer(),
                        cursor: self.editor.cursor(),
                    };
                    self.vim.handle_key(
                        &key,
                        &modified_key,
                        &modifiers,
                        text.as_deref(),
                        &view,
                    )
                };

                for action in actions {
                    match self.editor.execute(action) {
                        EditorEffect::Task(t) => {
                            self.editor.update_search_cache();
                            self.editor.ensure_cursor_visible();
                            return t;
                        }
                        EditorEffect::None => {}
                    }
                }
                self.editor.update_search_cache();
                self.editor.ensure_cursor_visible();
            }
            Message::ScrollLines(delta) => {
                let total = self.editor.buffer().total_lines();
                self.editor
                    .window_mut()
                    .viewport
                    .scroll_lines(delta, SCROLL_SPEED, total);
            }
            Message::ScrollCols(delta) => {
                let max_len = self.editor.buffer().max_line_len();
                self.editor
                    .window_mut()
                    .viewport
                    .scroll_cols(delta, SCROLL_SPEED, max_len);
            }
            Message::MouseClick { x, y } => {
                let win = self.editor.window();
                let line =
                    win.viewport.scroll_y + (y / text_grid::LINE_HEIGHT) as usize;
                let col = win.viewport.scroll_x
                    + ((x - text_grid::GUTTER_WIDTH - 8.0).max(0.0) / text_grid::CHAR_WIDTH)
                        as usize;
                let new_cursor = self.editor.buffer().cursor_from_position(line, col);
                self.editor.window_mut().cursor = new_cursor;
                self.editor.ensure_cursor_visible();
            }
            Message::ViewportResized { lines, cols } => {
                let win = self.editor.window_mut();
                if win.viewport.visible_lines != lines
                    || win.viewport.visible_cols != cols
                {
                    win.viewport.visible_lines = lines;
                    win.viewport.visible_cols = cols;
                }
                self.editor.ensure_cursor_visible();
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let win = self.editor.window();
        let buffer = self.editor.buffer();
        let (cursor_line, cursor_col) = buffer.cursor_position(win.cursor);

        let grid = text_grid::text_grid(
            buffer,
            win.cursor,
            win.viewport.scroll_y,
            win.viewport.scroll_x,
            win.selection,
            self.editor.search_matches(),
            self.editor.search_len(),
        );

        let (mr, mg, mb) = self.vim.mode_color();
        let mode_label = text(format!(" {} ", self.vim.mode()))
            .size(14)
            .color(iced::Color::from_rgb(mr, mg, mb));

        let modified_indicator = if buffer.is_modified() {
            "[+]"
        } else {
            ""
        };

        let buffer_name = text(format!(
            " {} {}",
            buffer.name(),
            modified_indicator
        ))
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

        let cmdline_text = self
            .vim
            .status_line_override()
            .unwrap_or_else(|| self.editor.status_message.clone());

        let cmdline = container(
            text(format!(" {}", cmdline_text))
                .size(14)
                .color(iced::Color::from_rgb(0.8, 0.8, 0.8)),
        )
        .width(Length::Fill)
        .padding([2, 0]);

        let content = column![grid, modeline, cmdline];

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
