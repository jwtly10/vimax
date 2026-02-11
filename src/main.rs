mod buffer;
mod text_grid;
mod undo;

use buffer::Buffer;
use iced::keyboard;
use iced::widget::Space;
use iced::widget::{column, container, row, text};
use iced::{Element, Length, Subscription, Task, Theme, event};
use tracing::{debug, error, info};

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("remax=debug")),
        )
        .with_writer(std::io::stderr)
        .init();

    info!("remax starting");

    iced::application(Remax::boot, Remax::update, Remax::view)
        .subscription(Remax::subscription)
        .theme(Remax::theme)
        .run()
}

const SCROLL_MARGIN: usize = 5;
const SCROLL_SPEED: f32 = 3.0;

struct Remax {
    buffer: Buffer,
    vim_mode: VimMode,
    scroll_y: usize,
    scroll_x: usize,
    visible_lines: usize,
    visible_cols: usize,
    pending_normal_key: Option<char>, // Quick multi-key commands
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VimMode {
    Normal,
    Insert,
}

impl std::fmt::Display for VimMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VimMode::Normal => write!(f, "NORMAL"),
            VimMode::Insert => write!(f, "INSERT"),
        }
    }
}

/// We carry the full KeyPressed payload so insert mode can use `text`
/// and normal mode can use `modified_key`
#[derive(Debug, Clone)]
enum Message {
    KeyEvent {
        key: keyboard::Key,
        modified_key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        text: Option<smol_str::SmolStr>,
    },
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
    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn boot() -> (Self, Task<Message>) {
        let args = parse_args();
        let file_path = args.get(0); // TODO: Assuming the first is always file path

        let buffer: Buffer;

        if file_path.is_some() {
            let path = std::path::Path::new(file_path.unwrap());
            let buf_name = path.file_name().unwrap_or_default().to_string_lossy();
            debug!(?path, "attempting to read file into buffer");
            match std::fs::read_to_string(path) {
                Ok(content) => buffer = Buffer::from_str(&content, &buf_name, &path, false),
                Err(e) => {
                    debug!(?e, "failed to read file, starting with empty buffer");
                    buffer = create_scratch_buffer();
                }
            }
        } else {
            debug!("no file path provided, starting with empty buffer");
            buffer = create_scratch_buffer();
        }

        info!("editor booted");
        (
            Self {
                buffer,
                vim_mode: VimMode::Normal,
                scroll_y: 0,
                scroll_x: 0,
                visible_lines: 40,
                visible_cols: 80,
                pending_normal_key: None,
            },
            Task::none(),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
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
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::KeyEvent {
                key,
                modified_key,
                modifiers,
                text,
            } => {
                debug!(
                    mode = %self.vim_mode,
                    ?key,
                    ?modified_key,
                    ?modifiers,
                    ?text,
                    "key event"
                );

                // Global shorts overwrite anything handled by the vim layer
                if self.handle_global_key(&modified_key, &modifiers) {
                    self.ensure_cursor_visible();
                    return Task::none();
                }

                match self.vim_mode {
                    VimMode::Normal => self.handle_normal_key(&modified_key, &modifiers),
                    VimMode::Insert => self.handle_insert_key(&key, &modifiers, text.as_deref()),
                }
                self.ensure_cursor_visible();
            }
            Message::ScrollLines(delta) => {
                let total = self.buffer.total_lines();
                let new_y = (self.scroll_y as f32 - delta * SCROLL_SPEED).round();
                let max_scroll = total.saturating_sub(1);
                self.scroll_y = (new_y.max(0.0) as usize).min(max_scroll);
                debug!(delta, scroll_y = self.scroll_y, "scrolled lines");
            }
            Message::ScrollCols(delta) => {
                let total = self.buffer.max_line_len();
                let new_x = (self.scroll_x as f32 - delta * SCROLL_SPEED).round();
                let max_scroll = total.saturating_sub(1);
                self.scroll_x = (new_x.max(0.0) as usize).min(max_scroll);
                debug!(delta, scroll_x = self.scroll_x, "scrolled cols");
            }
            Message::MouseClick { x, y } => {
                let line = self.scroll_y + (y / text_grid::LINE_HEIGHT) as usize;
                let col = self.scroll_x
                    + ((x - text_grid::GUTTER_WIDTH - 8.0).max(0.0) / text_grid::CHAR_WIDTH)
                        as usize;
                self.buffer.set_cursor_position(line, col);
                self.ensure_cursor_visible();
            }
            Message::ViewportResized { lines, cols } => {
                if self.visible_lines != lines || self.visible_cols != cols {
                    self.visible_lines = lines;
                    self.visible_cols = cols;
                    self.ensure_cursor_visible();
                }
            }
        }
        Task::none()
    }

    fn ensure_cursor_visible(&mut self) {
        let (cursor_line, cursor_col) = self.buffer.cursor_position();

        // Vertical scrolling with margin
        if self.visible_lines > SCROLL_MARGIN * 2 {
            if cursor_line < self.scroll_y + SCROLL_MARGIN {
                self.scroll_y = cursor_line.saturating_sub(SCROLL_MARGIN);
            } else if cursor_line + SCROLL_MARGIN >= self.scroll_y + self.visible_lines {
                self.scroll_y =
                    (cursor_line + SCROLL_MARGIN + 1).saturating_sub(self.visible_lines);
            }
        } else {
            // Window too small for margin, just keep cursor in view
            if cursor_line < self.scroll_y {
                self.scroll_y = cursor_line;
            } else if cursor_line >= self.scroll_y + self.visible_lines {
                self.scroll_y = cursor_line + 1 - self.visible_lines;
            }
        }

        // Clamp scroll_y to valid range
        let total = self.buffer.total_lines();
        let max_scroll = total.saturating_sub(1);
        self.scroll_y = self.scroll_y.min(max_scroll);

        // Horizontal scrolling
        let h_margin = 10_usize;
        if cursor_col < self.scroll_x {
            self.scroll_x = cursor_col.saturating_sub(h_margin);
        } else if cursor_col >= self.scroll_x + self.visible_cols {
            self.scroll_x = cursor_col + 1 + h_margin - self.visible_cols;
        }
    }

    /// Handles global shortcuts that work in any mode
    fn handle_global_key(&mut self, key: &keyboard::Key, modifiers: &keyboard::Modifiers) -> bool {
        if modifiers.control() {
            if let keyboard::Key::Character(c) = key {
                if c.as_str() == "r" {
                    self.buffer.redo();
                    return true;
                }
            }
        }
        if modifiers.command() {
            match key {
                keyboard::Key::Character(c) => match c.as_str() {
                    "s" => {
                        debug!("save shortcut triggered");
                        // TODO: Need some error propogation
                        let res = self.buffer.save();
                        if let Err(e) = res {
                            error!(?e, "failed to save file");
                        } else {
                            info!("file saved successfully");
                        }
                        return true;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        false
    }

    /// Normal mode: we match on `modified_key` which already has shift applied.
    fn handle_normal_key(&mut self, key: &keyboard::Key, _modifiers: &keyboard::Modifiers) {
        if let Some(pending) = self.pending_normal_key.take() {
            if let keyboard::Key::Character(c) = key {
                match (pending, c.as_str()) {
                    ('g', "g") => self.buffer.move_to_start(),
                    ('d', "d") => self.buffer.delete_line(),
                    _ => {}
                }
            }
            return;
        }
        match key {
            keyboard::Key::Character(c) => match c.as_str() {
                "g" | "d" => {
                    self.pending_normal_key = Some(c.as_str().chars().next().unwrap());
                }
                "u" => self.buffer.undo(),
                "i" => {
                    info!("entering insert mode");
                    self.buffer.start_edit_group();
                    self.vim_mode = VimMode::Insert;
                }
                "a" => {
                    self.buffer.start_edit_group();
                    self.buffer.move_right();
                    self.vim_mode = VimMode::Insert;
                }
                "e" => {
                    self.buffer.move_to_line_end();
                }
                "o" => {
                    self.buffer.start_edit_group();
                    self.buffer.move_to_line_end();
                    self.buffer.insert_char('\n');
                    self.vim_mode = VimMode::Insert;
                }
                "O" => {
                    self.buffer.start_edit_group();
                    self.buffer.move_to_line_start();
                    self.buffer.insert_char('\n');
                    self.buffer.move_up();
                    self.vim_mode = VimMode::Insert;
                }
                "A" => {
                    self.buffer.start_edit_group();
                    self.buffer.move_to_line_end();
                    self.vim_mode = VimMode::Insert;
                }
                "I" => {
                    self.buffer.start_edit_group();
                    self.buffer.move_to_line_start();
                    self.vim_mode = VimMode::Insert;
                }
                "h" => self.buffer.move_left(),
                "j" => self.buffer.move_down(),
                "k" => self.buffer.move_up(),
                "l" => self.buffer.move_right(),
                "w" => self.buffer.move_word_forward(),
                "b" => self.buffer.move_word_backward(),
                "0" => self.buffer.move_to_line_start(),
                "$" => self.buffer.move_to_line_end(),
                "^" => self.buffer.move_to_first_non_whitespace(),
                "G" => self.buffer.move_to_end(),
                "x" => self.buffer.delete_char_forward(),
                other => {
                    debug!(key = other, "unhandled normal mode key");
                }
            },
            keyboard::Key::Named(named) => match named {
                keyboard::key::Named::ArrowLeft => self.buffer.move_left(),
                keyboard::key::Named::ArrowRight => self.buffer.move_right(),
                keyboard::key::Named::ArrowUp => self.buffer.move_up(),
                keyboard::key::Named::ArrowDown => self.buffer.move_down(),
                keyboard::key::Named::Escape => {
                    debug!("escape in normal mode (no-op)");
                }
                _ => {
                    debug!(?named, "unhandled normal mode named key");
                }
            },
            _ => {}
        }
    }

    /// Insert mode: we use the `text` field from the OS for character input.
    /// and only handle special keys if needed
    fn handle_insert_key(
        &mut self,
        key: &keyboard::Key,
        _modifiers: &keyboard::Modifiers,
        text: Option<&str>,
    ) {
        if let keyboard::Key::Named(named) = key {
            match named {
                keyboard::key::Named::Escape => {
                    info!("entering normal mode");
                    self.buffer.finish_edit_group();
                    self.vim_mode = VimMode::Normal;
                    return;
                }
                keyboard::key::Named::Backspace => {
                    self.buffer.delete_char_backward();
                    return;
                }
                keyboard::key::Named::Delete => {
                    self.buffer.delete_char_forward();
                    return;
                }
                keyboard::key::Named::Enter => {
                    self.buffer.insert_char('\n');
                    return;
                }
                keyboard::key::Named::Tab => {
                    self.buffer.insert_str("    ");
                    return;
                }
                keyboard::key::Named::ArrowLeft => {
                    self.buffer.move_left();
                    return;
                }
                keyboard::key::Named::ArrowRight => {
                    self.buffer.move_right();
                    return;
                }
                keyboard::key::Named::ArrowUp => {
                    self.buffer.move_up();
                    return;
                }
                keyboard::key::Named::ArrowDown => {
                    self.buffer.move_down();
                    return;
                }
                keyboard::key::Named::Space => {
                    // fall through - let OS handle space char
                }
                _ => return,
            }
        }

        // Let OS handle
        if let Some(t) = text {
            for ch in t.chars() {
                if !ch.is_control() {
                    self.buffer.insert_char(ch);
                }
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let (cursor_line, cursor_col) = self.buffer.cursor_position();

        let grid = text_grid::text_grid(&self.buffer, self.scroll_y, self.scroll_x);
        let mode_label = text(format!(" {} ", self.vim_mode))
            .size(14)
            .color(match self.vim_mode {
                VimMode::Normal => iced::Color::from_rgb(0.6, 0.8, 1.0),
                VimMode::Insert => iced::Color::from_rgb(0.6, 1.0, 0.6),
            });

        let modified_indicator = if self.buffer.is_modified() { "[+]" } else { "" };

        let buffer_name = text(format!(" {} {}", self.buffer.name(), modified_indicator))
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

        let content = column![grid, modeline];

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

/// Parses args that were passed in when launching the app
fn parse_args() -> Vec<String> {
    let args = std::env::args();
    if args.len() > 1 {
        info!(?args, "launch args");
        args.skip(1).collect()
    } else {
        Vec::new()
    }
}

/// Creates a new scratch buffer with welcome text.
fn create_scratch_buffer() -> Buffer {
    let mut buffer = Buffer::new();
    buffer.set_name("*scratch*");
    buffer.insert_str("Welcome to remax.\n\nPress 'i' to enter insert mode.\nPress 'Esc' to return to normal mode.\nUse h/j/k/l to navigate.");
    buffer.move_to_start();
    buffer
}
