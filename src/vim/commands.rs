use crate::action::{EditorAction, Motion};
use crate::buffer::Buffer;

use super::input::InputState;

pub fn resolve(
    id: &str,
    count: usize,
    input: &mut InputState,
    buffer: &Buffer,
    cursor: usize,
) -> Vec<EditorAction> {
    match id {
        "cursor.move_left" => vec![EditorAction::MoveCursor {
            motion: Motion::Left,
            count,
        }],
        "cursor.move_right" => vec![EditorAction::MoveCursor {
            motion: Motion::Right,
            count,
        }],
        "cursor.move_up" => vec![EditorAction::MoveCursor {
            motion: Motion::Up,
            count,
        }],
        "cursor.move_down" => vec![EditorAction::MoveCursor {
            motion: Motion::Down,
            count,
        }],
        "cursor.move_word_forward" => vec![EditorAction::MoveCursor {
            motion: Motion::WordForward,
            count,
        }],
        "cursor.move_word_backward" => vec![EditorAction::MoveCursor {
            motion: Motion::WordBackward,
            count,
        }],
        "cursor.move_word_end" => vec![EditorAction::MoveCursor {
            motion: Motion::WordEnd,
            count,
        }],
        "cursor.move_line_start" => vec![EditorAction::MoveCursor {
            motion: Motion::LineStart,
            count: 1,
        }],
        "cursor.move_line_end" => vec![EditorAction::MoveCursor {
            motion: Motion::LineEnd,
            count: 1,
        }],
        "cursor.move_first_non_whitespace" => vec![EditorAction::MoveCursor {
            motion: Motion::FirstNonWhitespace,
            count: 1,
        }],
        "cursor.move_to_start" => vec![EditorAction::MoveCursor {
            motion: Motion::FileStart,
            count: 1,
        }],
        "cursor.move_to_end" => vec![EditorAction::MoveCursor {
            motion: Motion::FileEnd,
            count: 1,
        }],

        "scroll.half_down" => vec![EditorAction::MoveCursor {
            motion: Motion::HalfPageDown,
            count,
        }],
        "scroll.half_up" => vec![EditorAction::MoveCursor {
            motion: Motion::HalfPageUp,
            count,
        }],

        "edit.delete_till_eol" => vec![EditorAction::DeleteTillEndOfLine],
        "edit.delete_char_forward" => vec![EditorAction::DeleteCharForward { count }],
        "edit.delete_line" => vec![EditorAction::DeleteLine { count }],
        "edit.undo" => vec![EditorAction::Undo],
        "edit.redo" => vec![EditorAction::Redo],
        "edit.paste_after" => vec![EditorAction::Paste { before: false }],
        "edit.paste_before" => vec![EditorAction::Paste { before: true }],
        "edit.replace_char" => {
            input.pending_replace = true;
            vec![]
        }

        "insert.backspace" => vec![EditorAction::DeleteCharBackward],
        "insert.delete" => vec![EditorAction::DeleteCharForward { count: 1 }],
        "insert.newline" => vec![EditorAction::InsertNewline],
        "insert.tab" => vec![EditorAction::InsertTab],

        "vim.enter_insert" => input.enter_insert(),
        "vim.enter_insert_after" => input.enter_insert_after(),
        "vim.enter_insert_line_end" => input.enter_insert_line_end(),
        "vim.enter_insert_line_start" => input.enter_insert_line_start(),
        "vim.open_below" => input.open_below(),
        "vim.open_above" => input.open_above(),
        "vim.exit_insert" => input.exit_insert(),
        "vim.enter_command" => {
            input.enter_command();
            vec![]
        }
        "vim.enter_visual" => {
            input.enter_visual(cursor);
            vec![]
        }
        "vim.enter_visual_line" => {
            input.enter_visual_line(cursor);
            vec![]
        }
        "vim.exit_visual" => input.exit_visual(),
        "vim.clear_search" => vec![EditorAction::ClearSearch],
        "vim.enter_search" => {
            input.enter_search();
            vec![]
        }

        "lsp.goto_definition" => vec![EditorAction::LspGotoDefinition],

        "picker.buffers" => input.open_buffer_picker(),
        "picker.project_files" => input.open_project_files_picker(false),
        "picker.project_files_show_ignored" => input.open_project_files_picker(true),

        "search.next" => vec![EditorAction::SearchNext { count }],
        "search.prev" => vec![EditorAction::SearchPrev { count }],

        "op.delete" | "op.change" | "op.yank" => vec![],

        "visual.delete" => input.visual_delete(buffer, cursor),
        "visual.yank" => input.visual_yank(buffer, cursor),
        "visual.change" => input.visual_change(buffer, cursor),

        "motion.find_char_forward" => {
            input.pending_find_char = Some((true, false));
            vec![]
        }
        "motion.find_char_backward" => {
            input.pending_find_char = Some((false, false));
            vec![]
        }
        "motion.find_char_forward_before" => {
            input.pending_find_char = Some((true, true));
            vec![]
        }
        "motion.find_char_backward_before" => {
            input.pending_find_char = Some((false, true));
            vec![]
        }
        "motion.repeat_find_char" => {
            if let Some((ch, forward, stop_before)) = input.last_find_char {
                vec![EditorAction::MoveCursor {
                    motion: Motion::FindChar {
                        ch,
                        forward,
                        stop_before,
                    },
                    count,
                }]
            } else {
                vec![]
            }
        }
        "motion.repeat_find_char_reverse" => {
            if let Some((ch, forward, stop_before)) = input.last_find_char {
                vec![EditorAction::MoveCursor {
                    motion: Motion::FindChar {
                        ch,
                        forward: !forward,
                        stop_before,
                    },
                    count,
                }]
            } else {
                vec![]
            }
        }

        "buffer.save" => vec![EditorAction::Save],
        "buffer.quit" => vec![EditorAction::Quit { force: false }],
        "buffer.force_quit" => vec![EditorAction::Quit { force: true }],
        "buffer.write_quit" => vec![EditorAction::WriteQuit],

        "window.vsplit" => vec![EditorAction::VSplit],
        "window.hsplit" => vec![EditorAction::HSplit],
        "window.close" => vec![EditorAction::CloseWindow],
        "window.focus_left" => vec![EditorAction::FocusLeft],
        "window.focus_right" => vec![EditorAction::FocusRight],
        "window.focus_up" => vec![EditorAction::FocusUp],
        "window.focus_down" => vec![EditorAction::FocusDown],

        "system.copy" => vec![EditorAction::SystemCopy],
        "system.cut" => vec![EditorAction::SystemCut],
        "system.paste" => vec![EditorAction::SystemPaste],

        _ => vec![],
    }
}
