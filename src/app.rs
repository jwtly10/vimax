use crate::buffer::Buffer;
use crate::command::{self, ActionRegistry, CommandCtx, CommandEffect, CommandId};
use crate::input::{InputState, OperatorRange, TextObject, VimAction, VimMode};
use crate::keymap::Keymaps;
use crate::text_grid;

use iced::keyboard;
use iced::widget::Space;
use iced::widget::{column, container, row, text};
use iced::{Element, Length, Subscription, Task, Theme, event, window};
use tracing::{debug, error, info};

const SCROLL_MARGIN: usize = 5;
const SCROLL_SPEED: f32 = 0.3;

pub struct Remax {
    pub buffer: Buffer,
    pub input: InputState,
    actions: ActionRegistry,
    keymaps: Keymaps,
    scroll_y: usize,
    scroll_x: usize,
    pub visible_lines: usize,
    visible_cols: usize,
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

        let mut commands = ActionRegistry::new();
        command::register_all(&mut commands);

        info!("editor booted");
        (
            Self {
                buffer,
                input: InputState::new(),
                actions: commands,
                keymaps: Keymaps::new(),
                scroll_y: 0,
                scroll_x: 0,
                visible_lines: 40,
                visible_cols: 80,
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
                if self.buffer.is_modified() {
                    self.input.command_display =
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
                    mode = %self.input.mode,
                    ?key,
                    ?modified_key,
                    ?modifiers,
                    ?text,
                    "key event"
                );

                let mode_keymap = self.keymaps.for_mode(self.input.mode);

                let action = self.input.handle_key_event(
                    &key,
                    &modified_key,
                    &modifiers,
                    text.as_deref(),
                    self.keymaps.global(),
                    mode_keymap,
                );

                match action {
                    VimAction::Dispatch { command, ctx } => {
                        let task = self.execute_command(command, ctx);
                        self.ensure_cursor_visible();
                        return task;
                    }
                    VimAction::OperatorMotion {
                        operator,
                        range,
                        count,
                    } => {
                        self.execute_operator(operator, range, count);
                    }
                    VimAction::VisualTextObject(obj) => {
                        let (start, end) = self.resolve_text_object(obj);
                        if start != end {
                            self.input.selection_anchor = Some(start);
                            self.buffer.set_cursor(end.saturating_sub(1));
                        }
                    }
                    VimAction::InsertText(t) => {
                        if !self.input.search_pattern.is_empty() {
                            self.input.search_pattern.clear();
                        }
                        for ch in t.chars() {
                            self.buffer.insert_char(ch);
                        }
                    }
                    VimAction::ExecuteCommandLine(cmd) => {
                        let task = self.execute_ex_command(&cmd);
                        self.ensure_cursor_visible();
                        return task;
                    }
                    VimAction::ExecuteSearch(query) => {
                        if query.is_empty() {
                            return Task::none();
                        }
                        let from = self.buffer.cursor() + 1;
                        if let Some(pos) = self.buffer.find_next(&query, from) {
                            self.buffer.set_cursor(pos);
                        }
                        let matches = self.buffer.find_all(&query);
                        let total = matches.len();
                        let current = matches
                            .iter()
                            .position(|&m| m == self.buffer.cursor())
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        self.input.command_display =
                            format!("/{} [{}/{}]", query, current, total);
                        self.ensure_cursor_visible();
                    }
                    VimAction::SearchUpdated => {}
                    VimAction::ReplaceChar(ch) => {
                        self.buffer.replace_char(ch);
                    }
                    VimAction::FindChar {
                        ch,
                        forward,
                        stop_before,
                    } => {
                        self.buffer.find_char_on_line(ch, forward, stop_before);
                    }
                    VimAction::CommandLineUpdated | VimAction::Pending | VimAction::Unhandled => {}
                }
                self.ensure_cursor_visible();
            }
            Message::ScrollLines(delta) => {
                let total = self.buffer.total_lines();
                let new_y = (self.scroll_y as f32 - delta * SCROLL_SPEED).round();
                let max_scroll = total.saturating_sub(1);
                self.scroll_y = (new_y.max(0.0) as usize).min(max_scroll);
            }
            Message::ScrollCols(delta) => {
                let total = self.buffer.max_line_len();
                let new_x = (self.scroll_x as f32 - delta * SCROLL_SPEED).round();
                let max_scroll = total.saturating_sub(1);
                self.scroll_x = (new_x.max(0.0) as usize).min(max_scroll);
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

    /// Looks up a command by ID, copy the fn pointer out, then call it.
    fn execute_command(&mut self, id: CommandId, ctx: CommandCtx) -> Task<Message> {
        if let Some(func) = self.actions.get(id) {
            match func(self, ctx) {
                CommandEffect::None => Task::none(),
                CommandEffect::Task(task) => task,
                CommandEffect::DisplayMessage(msg) => {
                    self.input.command_display = msg;
                    Task::none()
                }
                CommandEffect::DisplayError(msg) => {
                    self.input.command_display = msg;
                    Task::none()
                }
            }
        } else {
            error!(command = id, "unknown command");
            Task::none()
        }
    }

    /// Map ex commands (:w, :q, etc.) to registry command IDs.
    fn execute_ex_command(&mut self, cmd: &str) -> Task<Message> {
        let id = match cmd.trim() {
            "w" => "buffer.save",
            "q" => "buffer.quit",
            "q!" => "buffer.force_quit",
            "wq" => "buffer.write_quit",
            other => {
                self.input.command_display = format!("Unknown command: {}", other);
                return Task::none();
            }
        };
        self.execute_command(id, CommandCtx { count: 1 })
    }

    fn resolve_text_object(&self, obj: TextObject) -> (usize, usize) {
        match obj {
            TextObject::InnerWord => self.buffer.text_object_inner_word(),
            TextObject::AWord => self.buffer.text_object_a_word(),
            TextObject::InnerParen => self.buffer.text_object_delimited('(', ')', false),
            TextObject::AParen => self.buffer.text_object_delimited('(', ')', true),
            TextObject::InnerCurly => self.buffer.text_object_delimited('{', '}', false),
            TextObject::ACurly => self.buffer.text_object_delimited('{', '}', true),
            TextObject::InnerBracket => self.buffer.text_object_delimited('[', ']', false),
            TextObject::ABracket => self.buffer.text_object_delimited('[', ']', true),
            TextObject::InnerAngle => self.buffer.text_object_delimited('<', '>', false),
            TextObject::AAngle => self.buffer.text_object_delimited('<', '>', true),
            TextObject::InnerSingleQuote => self.buffer.text_object_quoted('\'', false),
            TextObject::ASingleQuote => self.buffer.text_object_quoted('\'', true),
            TextObject::InnerDoubleQuote => self.buffer.text_object_quoted('"', false),
            TextObject::ADoubleQuote => self.buffer.text_object_quoted('"', true),
        }
    }

    fn execute_operator(&mut self, operator: &str, range: OperatorRange, count: usize) {
        let (start, end) = match range {
            OperatorRange::Motion { command } => {
                // Save cursor, execute motion, read new cursor → that's the range
                let before = self.buffer.cursor();
                let ctx = CommandCtx { count };
                if let Some(func) = self.actions.get(command) {
                    func(self, ctx);
                }
                let after = self.buffer.cursor();
                if before <= after {
                    (before, after)
                } else {
                    (after, before)
                }
            }
            OperatorRange::TextObject(obj) => self.resolve_text_object(obj),
            OperatorRange::WholeLine => {
                // Operate on count lines
                let (line, _) = self.buffer.cursor_position();
                let line_start = self.buffer.rope().line_to_char(line);
                let target_line = (line + count).min(self.buffer.total_lines());
                let line_end = if target_line < self.buffer.total_lines() {
                    self.buffer.rope().line_to_char(target_line)
                } else {
                    self.buffer.rope().len_chars()
                };
                (line_start, line_end)
            }
            OperatorRange::FindChar {
                ch,
                forward,
                stop_before,
            } => {
                let before = self.buffer.cursor();
                for _ in 0..count {
                    self.buffer.find_char_on_line(ch, forward, stop_before);
                }
                let after = self.buffer.cursor();
                if before <= after {
                    // For forward find, include the target char in the range
                    (before, after + 1)
                } else {
                    (after, before)
                }
            }
        };

        if start == end {
            return;
        }

        match operator {
            "op.delete" => {
                self.buffer.delete_range(start, end);
            }
            "op.change" => {
                self.buffer.delete_range(start, end);
                self.buffer.start_edit_group();
                self.input.mode = VimMode::Insert;
            }
            "op.yank" => {
                self.buffer.yank_range(start, end);
            }
            _ => {}
        }
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
        } else if cursor_line < self.scroll_y {
            self.scroll_y = cursor_line;
        } else if cursor_line >= self.scroll_y + self.visible_lines {
            self.scroll_y = cursor_line + 1 - self.visible_lines;
        }

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

    pub fn view(&self) -> Element<'_, Message> {
        let (cursor_line, cursor_col) = self.buffer.cursor_position();

        let selection = command::visual_range(self);

        let search_matches = self.buffer.find_all(&self.input.search_pattern);
        let search_len = self.input.search_pattern.len();
        let grid = text_grid::text_grid(
            &self.buffer,
            self.scroll_y,
            self.scroll_x,
            selection,
            search_matches,
            search_len,
        );
        let mode_label =
            text(format!(" {} ", self.input.mode))
                .size(14)
                .color(match self.input.mode {
                    VimMode::Normal => iced::Color::from_rgb(0.6, 0.8, 1.0),
                    VimMode::Insert => iced::Color::from_rgb(0.6, 1.0, 0.6),
                    VimMode::Command | VimMode::Search => {
                        iced::Color::from_rgb(1.0, 0.8, 0.5)
                    }
                    VimMode::Visual | VimMode::VisualLine => {
                        iced::Color::from_rgb(0.9, 0.6, 1.0)
                    }
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

        let cmdline_text = match self.input.mode {
            VimMode::Command => format!(":{}", self.input.command_line),
            VimMode::Search => format!("/{}", self.input.search_query),
            _ => self.input.command_display.clone(),
        };

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
        "Welcome to remax.\n\nPress 'i' to enter insert mode.\nPress 'Esc' to return to normal mode.\nUse h/j/k/l to navigate.",
    );
    buffer.move_to_start();
    buffer
}
