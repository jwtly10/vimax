mod buffer;
mod text_grid;

use buffer::Buffer;
use iced::keyboard;
use iced::widget::Space;
use iced::widget::{column, container, row, text};
use iced::{Element, Length, Subscription, Task, Theme, event};
use tracing::{debug, info};

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

struct Remax {
    buffer: Buffer,
    vim_mode: VimMode,
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
}

impl Remax {
    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn boot() -> (Self, Task<Message>) {
        let mut buffer = Buffer::new();
        buffer.set_name("*scratch*");
        buffer.insert_str("Welcome to remax.\n\nPress 'i' to enter insert mode.\nPress 'Esc' to return to normal mode.\nUse h/j/k/l to navigate.\n");
        buffer.move_to_start();
        info!("editor booted");
        (
            Self {
                buffer,
                vim_mode: VimMode::Normal,
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
                match self.vim_mode {
                    VimMode::Normal => self.handle_normal_key(&modified_key, &modifiers),
                    VimMode::Insert => self.handle_insert_key(&key, &modifiers, text.as_deref()),
                }
            }
        }
        Task::none()
    }

    /// Normal mode: we match on `modified_key` which already has shift applied.
    fn handle_normal_key(&mut self, key: &keyboard::Key, _modifiers: &keyboard::Modifiers) {
        match key {
            keyboard::Key::Character(c) => match c.as_str() {
                "i" => {
                    info!("entering insert mode");
                    self.vim_mode = VimMode::Insert;
                }
                "a" => {
                    self.buffer.move_right();
                    self.vim_mode = VimMode::Insert;
                }
                "o" => {
                    self.buffer.move_to_line_end();
                    self.buffer.insert_char('\n');
                    self.vim_mode = VimMode::Insert;
                }
                "O" => {
                    self.buffer.move_to_line_start();
                    self.buffer.insert_char('\n');
                    self.buffer.move_up();
                    self.vim_mode = VimMode::Insert;
                }
                "A" => {
                    self.buffer.move_to_line_end();
                    self.vim_mode = VimMode::Insert;
                }
                "I" => {
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
                keyboard::key::Named::Space => self.buffer.insert_char(' '),
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

        let grid = text_grid::text_grid(&self.buffer);

        let mode_label = text(format!(" {} ", self.vim_mode))
            .size(14)
            .color(match self.vim_mode {
                VimMode::Normal => iced::Color::from_rgb(0.6, 0.8, 1.0),
                VimMode::Insert => iced::Color::from_rgb(0.6, 1.0, 0.6),
            });

        let buffer_name = text(format!(" {} ", self.buffer.name()))
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
