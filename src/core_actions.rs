use crate::{
    action::{EditorAction, Motion, Range},
    buffer::Buffer,
    registers::Registers,
    vim::mode::VimMode,
    window::Window,
};

/// Used by both Editor and InfoPanel.
pub fn resolve_motion(
    buffer: &Buffer,
    cursor: usize,
    visible_lines: usize,
    motion: &Motion,
    count: usize,
) -> usize {
    match motion {
        Motion::Left => {
            let mut c = cursor;
            for _ in 0..count {
                c = buffer.move_left(c);
            }
            c
        }
        Motion::Right => {
            let mut c = cursor;
            for _ in 0..count {
                c = buffer.move_right(c);
            }
            c
        }
        Motion::Up => {
            let mut c = cursor;
            for _ in 0..count {
                c = buffer.move_up(c);
            }
            c
        }
        Motion::Down => {
            let mut c = cursor;
            for _ in 0..count {
                c = buffer.move_down(c);
            }
            c
        }
        Motion::WordForward => {
            let mut c = cursor;
            for _ in 0..count {
                c = buffer.move_word_forward(c);
            }
            c
        }
        Motion::WordBackward => {
            let mut c = cursor;
            for _ in 0..count {
                c = buffer.move_word_backward(c);
            }
            c
        }
        Motion::WordEnd => {
            let mut c = cursor;
            for _ in 0..count {
                c = buffer.move_word_end(c);
            }
            c
        }
        Motion::LineStart => buffer.move_to_line_start(cursor),
        Motion::LineEnd => buffer.move_to_line_end(cursor),
        Motion::FirstNonWhitespace => buffer.move_to_first_non_whitespace(cursor),
        Motion::FileStart => buffer.move_to_start(),
        Motion::FileEnd => buffer.move_to_end(),
        Motion::FindChar {
            ch,
            forward,
            stop_before,
        } => {
            let mut c = cursor;
            for _ in 0..count {
                c = buffer.find_char_on_line(c, *ch, *forward, *stop_before);
            }
            c
        }
        Motion::HalfPageDown => {
            let half = (visible_lines / 2).max(1) * count;
            let mut c = cursor;
            for _ in 0..half {
                c = buffer.move_down(c);
            }
            c
        }
        Motion::HalfPageUp => {
            let half = (visible_lines / 2).max(1) * count;
            let mut c = cursor;
            for _ in 0..half {
                c = buffer.move_up(c);
            }
            c
        }
    }
}

/// Execute the "portable" subset of editor actions that only need (win, buf, registers).
/// Returns true if the action was handled, false if the caller needs to handle it.
pub fn execute_core(
    win: &mut Window,
    buf: &mut Buffer,
    registers: &mut Registers,
    status_message: &mut String,
    action: &EditorAction,
) -> bool {
    if buf.is_read_only() && action.is_mutation() {
        *status_message = String::from("Cannot edit read-only buffer");
        return true;
    }
    match action {
        EditorAction::MoveCursor { motion, count } => {
            win.cursor = resolve_motion(buf, win.cursor, win.visible_lines, motion, *count);
        }
        EditorAction::SetCursor(pos) => {
            win.cursor = buf.clamp_cursor(*pos);
        }
        EditorAction::InsertChar(ch) => {
            win.cursor = buf.insert_char(win.cursor, *ch);
        }
        EditorAction::InsertNewline => {
            let cursor = win.cursor;
            let line_idx = buf.char_to_line(cursor);
            let line_start = buf.line_to_char(line_idx);
            let indent_width = buf.indent_width() as usize;
            let one_indent = if buf.use_tabs() {
                "\t".to_string()
            } else {
                " ".repeat(indent_width)
            };

            let leading_ws: String = buf
                .rope()
                .line(line_idx)
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();

            let line_to_cursor: String = buf.rope().slice(line_start..cursor).chars().collect();
            let last_significant = line_to_cursor.trim_end().chars().last();

            let new_indent = match last_significant {
                Some('{') | Some('(') | Some('[') => {
                    format!("{}{}", leading_ws, one_indent)
                }
                _ => leading_ws,
            };

            win.cursor = buf.insert_str(cursor, &format!("\n{}", new_indent));
        }
        EditorAction::InsertTab => {
            let tab_str = if buf.use_tabs() {
                "\t".repeat(buf.indent_width() as usize)
            } else {
                " ".repeat(buf.indent_width() as usize)
            };
            win.cursor = buf.insert_str(win.cursor, &tab_str);
        }
        EditorAction::DeleteTillEndOfLine => {
            win.cursor = buf.delete_till_eol(win.cursor);
        }
        EditorAction::DeleteCharForward { count } => {
            for _ in 0..*count {
                win.cursor = buf.delete_char_forward(win.cursor);
            }
        }
        EditorAction::DeleteCharBackward => {
            win.cursor = buf.delete_char_backward(win.cursor);
        }
        EditorAction::DeleteLine { count } => {
            for _ in 0..*count {
                win.cursor = buf.delete_line(win.cursor);
            }
        }
        EditorAction::DeleteRange(Range { start, end }) => {
            let (new_cursor, deleted) = buf.delete_range(win.cursor, *start, *end);
            win.cursor = new_cursor;
            registers.unnamed = deleted;
        }
        EditorAction::ChangeRange(Range { start, end }) => {
            buf.start_edit_group(win.cursor);
            let (new_cursor, deleted) = buf.delete_range(win.cursor, *start, *end);
            win.cursor = new_cursor;
            registers.unnamed = deleted;
        }
        EditorAction::YankRange(Range { start, end }) => {
            registers.unnamed = buf.yank_range(*start, *end);
        }
        EditorAction::ReplaceChar(ch) => {
            win.cursor = buf.replace_char(win.cursor, *ch);
        }
        EditorAction::Paste { before } => {
            let text = registers.unnamed.clone();
            win.cursor = if *before {
                buf.paste_before(win.cursor, &text)
            } else {
                buf.paste_after(win.cursor, &text)
            };
        }
        EditorAction::Undo => {
            if let Some(new_cursor) = buf.undo() {
                win.cursor = new_cursor;
            }
        }
        EditorAction::Redo => {
            if let Some(new_cursor) = buf.redo() {
                win.cursor = new_cursor;
            }
        }
        EditorAction::StartEditGroup => {
            buf.start_edit_group(win.cursor);
        }
        EditorAction::FinishEditGroup => {
            buf.finish_edit_group();
        }
        EditorAction::SetSelection(sel) => {
            win.selection = *sel;
        }
        EditorAction::UpdateVisualSelection { anchor, mode } => {
            let cursor = win.cursor;
            let selection = if *mode == VimMode::VisualLine {
                let anchor_line = buf.char_to_line(*anchor);
                let cursor_line = buf.char_to_line(cursor);
                let (start_line, end_line) = if anchor_line <= cursor_line {
                    (anchor_line, cursor_line)
                } else {
                    (cursor_line, anchor_line)
                };
                let start = buf.line_to_char(start_line);
                let end = if end_line + 1 < buf.total_lines() {
                    buf.line_to_char(end_line + 1)
                } else {
                    buf.len_chars()
                };
                (start, end)
            } else if *anchor <= cursor {
                (*anchor, cursor + 1)
            } else {
                (cursor, *anchor + 1)
            };
            win.selection = Some(selection);
        }
        EditorAction::SystemCopy => {
            let text = if let Some((start, end)) = win.selection {
                buf.yank_range(start, end)
            } else {
                let cursor = win.cursor;
                let line = buf.char_to_line(cursor);
                let line_start = buf.line_to_char(line);
                let line_end = if line + 1 < buf.total_lines() {
                    buf.line_to_char(line + 1)
                } else {
                    buf.len_chars()
                };
                buf.yank_range(line_start, line_end)
            };
            registers.unnamed = text.clone();
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(text);
            }
        }
        EditorAction::SystemCut => {
            let text = if let Some((start, end)) = win.selection {
                let yanked = buf.yank_range(start, end);
                let (new_cursor, _) = buf.delete_range(win.cursor, start, end);
                win.cursor = new_cursor;
                win.selection = None;
                yanked
            } else {
                let cursor = win.cursor;
                let line = buf.char_to_line(cursor);
                let line_start = buf.line_to_char(line);
                let line_end = if line + 1 < buf.total_lines() {
                    buf.line_to_char(line + 1)
                } else {
                    buf.len_chars()
                };
                let yanked = buf.yank_range(line_start, line_end);
                let (new_cursor, _) = buf.delete_range(cursor, line_start, line_end);
                win.cursor = new_cursor;
                yanked
            };
            registers.unnamed = text.clone();
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(text);
            }
        }
        EditorAction::SystemPaste => {
            let text = if let Ok(mut clipboard) = arboard::Clipboard::new() {
                clipboard.get_text().unwrap_or_default()
            } else {
                return true;
            };
            if !text.is_empty() {
                win.cursor = buf.insert_str(win.cursor, &text);
            }
        }
        EditorAction::SetStatusMessage(msg) => {
            *status_message = msg.clone();
        }
        EditorAction::LspApplyCompletion {
            delete_backward,
            insert_text,
        } => {
            for _ in 0..*delete_backward {
                win.cursor = buf.delete_char_backward(win.cursor);
            }
            win.cursor = buf.insert_str(win.cursor, insert_text);
        }
        _ => return false,
    }
    true
}

/// Navigate search matches forward. Used by both Editor and InfoPanel.
pub fn search_next_in(win: &mut Window, pattern: &str, count: usize) {
    if pattern.is_empty() || win.search_matches.is_empty() {
        return;
    }
    let mut cursor = win.cursor;
    for _ in 0..count {
        let next = win
            .search_matches
            .iter()
            .find(|&&m| m > cursor)
            .or(win.search_matches.first());
        if let Some(&pos) = next {
            cursor = pos;
        }
    }
    win.cursor = cursor;
}

/// Navigate search matches backward. Used by both Editor and InfoPanel.
pub fn search_prev_in(win: &mut Window, pattern: &str, count: usize) {
    if pattern.is_empty() || win.search_matches.is_empty() {
        return;
    }
    let mut cursor = win.cursor;
    for _ in 0..count {
        let prev = win
            .search_matches
            .iter()
            .rev()
            .find(|&&m| m < cursor)
            .or(win.search_matches.last());
        if let Some(&pos) = prev {
            cursor = pos;
        }
    }
    win.cursor = cursor;
}
