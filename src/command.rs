use std::collections::HashMap;

use iced::Task;
use tracing::{error, info};

use crate::app::{Message, Remax};

pub type CommandId = &'static str;

pub struct CommandCtx {
    pub count: usize,
}

pub enum CommandEffect {
    None,
    Task(Task<Message>),
    DisplayMessage(String),
    DisplayError(String),
}

pub type CommandFn = fn(&mut Remax, CommandCtx) -> CommandEffect;

struct CommandEntry {
    func: CommandFn,
}

pub struct ActionRegistry {
    actions: HashMap<CommandId, CommandEntry>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            actions: HashMap::new(),
        }
    }

    pub fn register(&mut self, id: CommandId, func: CommandFn) {
        self.actions.insert(id, CommandEntry { func });
    }

    pub fn get(&self, id: CommandId) -> Option<CommandFn> {
        self.actions.get(id).map(|e| e.func)
    }
}

// -- Command functions --

// Cursor movement
pub fn cmd_move_left(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    for _ in 0..ctx.count {
        app.buffer.move_left();
    }
    CommandEffect::None
}

pub fn cmd_move_right(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    for _ in 0..ctx.count {
        app.buffer.move_right();
    }
    CommandEffect::None
}

pub fn cmd_move_up(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    for _ in 0..ctx.count {
        app.buffer.move_up();
    }
    CommandEffect::None
}

pub fn cmd_move_down(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    for _ in 0..ctx.count {
        app.buffer.move_down();
    }
    CommandEffect::None
}

pub fn cmd_move_word_forward(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    for _ in 0..ctx.count {
        app.buffer.move_word_forward();
    }
    CommandEffect::None
}

pub fn cmd_move_word_backward(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    for _ in 0..ctx.count {
        app.buffer.move_word_backward();
    }
    CommandEffect::None
}

pub fn cmd_move_line_start(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.move_to_line_start();
    CommandEffect::None
}

pub fn cmd_move_line_end(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.move_to_line_end();
    CommandEffect::None
}

pub fn cmd_move_first_non_whitespace(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.move_to_first_non_whitespace();
    CommandEffect::None
}

pub fn cmd_move_to_start(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.move_to_start();
    CommandEffect::None
}

pub fn cmd_move_to_end(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.move_to_end();
    CommandEffect::None
}

// Editing
pub fn cmd_delete_char_forward(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    for _ in 0..ctx.count {
        app.buffer.delete_char_forward();
    }
    CommandEffect::None
}

pub fn cmd_delete_line(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    for _ in 0..ctx.count {
        app.buffer.delete_line();
    }
    CommandEffect::None
}

pub fn cmd_undo(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.undo();
    CommandEffect::None
}

pub fn cmd_redo(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.redo();
    CommandEffect::None
}

// Insert mode entry commands
pub fn cmd_vim_enter_insert(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    info!("entering insert mode");
    app.buffer.start_edit_group();
    app.input.mode = crate::input::VimMode::Insert;
    CommandEffect::None
}

pub fn cmd_vim_enter_insert_after(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.start_edit_group();
    app.buffer.move_right();
    app.input.mode = crate::input::VimMode::Insert;
    CommandEffect::None
}

pub fn cmd_vim_enter_insert_line_end(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.start_edit_group();
    app.buffer.move_to_line_end();
    app.input.mode = crate::input::VimMode::Insert;
    CommandEffect::None
}

pub fn cmd_vim_enter_insert_line_start(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.start_edit_group();
    app.buffer.move_to_line_start();
    app.input.mode = crate::input::VimMode::Insert;
    CommandEffect::None
}

pub fn cmd_vim_open_below(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.start_edit_group();
    app.buffer.move_to_line_end();
    app.buffer.insert_char('\n');
    app.input.mode = crate::input::VimMode::Insert;
    CommandEffect::None
}

pub fn cmd_vim_open_above(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.start_edit_group();
    app.buffer.move_to_line_start();
    app.buffer.insert_char('\n');
    app.buffer.move_up();
    app.input.mode = crate::input::VimMode::Insert;
    CommandEffect::None
}

pub fn cmd_vim_exit_insert(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    info!("entering normal mode");
    app.buffer.finish_edit_group();
    app.input.mode = crate::input::VimMode::Normal;
    CommandEffect::None
}

pub fn cmd_vim_enter_command(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.input.command_line.clear();
    app.input.command_display.clear();
    app.input.mode = crate::input::VimMode::Command;
    CommandEffect::None
}

// Insert mode editing
pub fn cmd_insert_backspace(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.delete_char_backward();
    CommandEffect::None
}

pub fn cmd_insert_delete(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.delete_char_forward();
    CommandEffect::None
}

pub fn cmd_insert_newline(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.insert_char('\n');
    CommandEffect::None
}

pub fn cmd_insert_tab(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.insert_str("    ");
    CommandEffect::None
}

// Operator-pending mode
pub fn cmd_op_delete(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.input.pending_operator = Some("op.delete");
    CommandEffect::None
}

pub fn cmd_op_change(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.input.pending_operator = Some("op.change");
    CommandEffect::None
}

pub fn cmd_op_yank(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.input.pending_operator = Some("op.yank");
    CommandEffect::None
}

// Visual mode

/// Compute the visual selection range from anchor + cursor, accounting for mode.
pub fn visual_range(app: &Remax) -> Option<(usize, usize)> {
    let anchor = app.input.selection_anchor?;
    let cursor = app.buffer.cursor();

    if app.input.mode == crate::input::VimMode::VisualLine {
        // Expand to full line boundaries
        let anchor_line = app.buffer.rope().char_to_line(anchor);
        let cursor_line = app.buffer.rope().char_to_line(cursor);
        let (start_line, end_line) = if anchor_line <= cursor_line {
            (anchor_line, cursor_line)
        } else {
            (cursor_line, anchor_line)
        };
        let start = app.buffer.rope().line_to_char(start_line);
        let end = if end_line + 1 < app.buffer.total_lines() {
            app.buffer.rope().line_to_char(end_line + 1)
        } else {
            app.buffer.rope().len_chars()
        };
        Some((start, end))
    } else {
        // Charwise visual
        if anchor <= cursor {
            Some((anchor, cursor + 1))
        } else {
            Some((cursor, anchor + 1))
        }
    }
}

pub fn cmd_vim_enter_visual(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    info!("entering visual mode");
    app.input.selection_anchor = Some(app.buffer.cursor());
    app.input.mode = crate::input::VimMode::Visual;
    CommandEffect::None
}

pub fn cmd_vim_enter_visual_line(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    info!("entering visual line mode");
    if app.input.mode == crate::input::VimMode::Visual {
        // Switch from charwise to linewise, keep anchor
        app.input.mode = crate::input::VimMode::VisualLine;
    } else if app.input.mode == crate::input::VimMode::VisualLine {
        // Already in visual line, exit
        app.input.selection_anchor = None;
        app.input.mode = crate::input::VimMode::Normal;
    } else {
        // From normal mode
        app.input.selection_anchor = Some(app.buffer.cursor());
        app.input.mode = crate::input::VimMode::VisualLine;
    }
    CommandEffect::None
}

pub fn cmd_vim_exit_visual(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    info!("exiting visual mode");
    app.input.selection_anchor = None;
    app.input.mode = crate::input::VimMode::Normal;
    CommandEffect::None
}

pub fn cmd_visual_delete(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    if let Some((start, end)) = visual_range(app) {
        app.buffer.delete_range(start, end);
    }
    app.input.selection_anchor = None;
    app.input.mode = crate::input::VimMode::Normal;
    CommandEffect::None
}

pub fn cmd_visual_yank(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    if let Some((start, end)) = visual_range(app) {
        app.buffer.yank_range(start, end);
        app.buffer.set_cursor(start);
    }
    app.input.selection_anchor = None;
    app.input.mode = crate::input::VimMode::Normal;
    CommandEffect::None
}

pub fn cmd_visual_change(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    if let Some((start, end)) = visual_range(app) {
        app.buffer.delete_range(start, end);
    }
    app.input.selection_anchor = None;
    app.buffer.start_edit_group();
    app.input.mode = crate::input::VimMode::Insert;
    CommandEffect::None
}

// Paste
pub fn cmd_paste_after(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.paste_after();
    CommandEffect::None
}

pub fn cmd_paste_before(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    app.buffer.paste_before();
    CommandEffect::None
}

// Scrolling
pub fn cmd_scroll_half_down(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    let half = (app.visible_lines / 2).max(1) * ctx.count;
    for _ in 0..half {
        app.buffer.move_down();
    }
    CommandEffect::None
}

pub fn cmd_scroll_half_up(app: &mut Remax, ctx: CommandCtx) -> CommandEffect {
    let half = (app.visible_lines / 2).max(1) * ctx.count;
    for _ in 0..half {
        app.buffer.move_up();
    }
    CommandEffect::None
}

// File operations
pub fn cmd_save(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    match app.buffer.save() {
        Ok(()) => {
            info!("file saved");
            CommandEffect::DisplayMessage(String::from("Written"))
        }
        Err(e) => {
            error!(?e, "failed to save");
            CommandEffect::DisplayError(format!("Error: {}", e))
        }
    }
}

pub fn cmd_quit(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    if app.buffer.is_modified() {
        CommandEffect::DisplayError(String::from(
            "Unsaved changes! Use :q! to force quit, or :wq to save and quit",
        ))
    } else {
        CommandEffect::Task(iced::exit())
    }
}

pub fn cmd_force_quit(_app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    CommandEffect::Task(iced::exit())
}

pub fn cmd_write_quit(app: &mut Remax, _ctx: CommandCtx) -> CommandEffect {
    match app.buffer.save() {
        Ok(()) => CommandEffect::Task(iced::exit()),
        Err(e) => {
            error!(?e, "failed to save");
            CommandEffect::DisplayError(format!("Error: {}", e))
        }
    }
}

/// Register all built-in commands.
pub fn register_all(registry: &mut ActionRegistry) {
    // Cursor movement
    registry.register("cursor.move_left", cmd_move_left);
    registry.register("cursor.move_right", cmd_move_right);
    registry.register("cursor.move_up", cmd_move_up);
    registry.register("cursor.move_down", cmd_move_down);
    registry.register("cursor.move_word_forward", cmd_move_word_forward);
    registry.register("cursor.move_word_backward", cmd_move_word_backward);
    registry.register("cursor.move_line_start", cmd_move_line_start);
    registry.register("cursor.move_line_end", cmd_move_line_end);
    registry.register(
        "cursor.move_first_non_whitespace",
        cmd_move_first_non_whitespace,
    );
    registry.register("cursor.move_to_start", cmd_move_to_start);
    registry.register("cursor.move_to_end", cmd_move_to_end);

    // Editing
    registry.register("edit.delete_char_forward", cmd_delete_char_forward);
    registry.register("edit.delete_line", cmd_delete_line);
    registry.register("edit.undo", cmd_undo);
    registry.register("edit.redo", cmd_redo);

    // Vim mode transitions
    registry.register("vim.enter_insert", cmd_vim_enter_insert);
    registry.register("vim.enter_insert_after", cmd_vim_enter_insert_after);
    registry.register("vim.enter_insert_line_end", cmd_vim_enter_insert_line_end);
    registry.register(
        "vim.enter_insert_line_start",
        cmd_vim_enter_insert_line_start,
    );
    registry.register("vim.open_below", cmd_vim_open_below);
    registry.register("vim.open_above", cmd_vim_open_above);
    registry.register("vim.exit_insert", cmd_vim_exit_insert);
    registry.register("vim.enter_command", cmd_vim_enter_command);

    // Insert mode editing
    registry.register("insert.backspace", cmd_insert_backspace);
    registry.register("insert.delete", cmd_insert_delete);
    registry.register("insert.newline", cmd_insert_newline);
    registry.register("insert.tab", cmd_insert_tab);

    // Operator-pending mode
    registry.register("op.delete", cmd_op_delete);
    registry.register("op.change", cmd_op_change);
    registry.register("op.yank", cmd_op_yank);

    // Visual mode
    registry.register("vim.enter_visual", cmd_vim_enter_visual);
    registry.register("vim.enter_visual_line", cmd_vim_enter_visual_line);
    registry.register("vim.exit_visual", cmd_vim_exit_visual);
    registry.register("visual.delete", cmd_visual_delete);
    registry.register("visual.yank", cmd_visual_yank);
    registry.register("visual.change", cmd_visual_change);

    // Paste
    registry.register("edit.paste_after", cmd_paste_after);
    registry.register("edit.paste_before", cmd_paste_before);

    // Scrolling
    registry.register("scroll.half_down", cmd_scroll_half_down);
    registry.register("scroll.half_up", cmd_scroll_half_up);

    // File operations
    registry.register("buffer.save", cmd_save);
    registry.register("buffer.quit", cmd_quit);
    registry.register("buffer.force_quit", cmd_force_quit);
    registry.register("buffer.write_quit", cmd_write_quit);
}
