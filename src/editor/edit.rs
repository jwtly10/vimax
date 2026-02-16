use crate::action::Range;
use crate::vim::mode::VimMode;

use super::Editor;

impl Editor {
    pub(crate) fn execute_insert_char(&mut self, ch: char) {
        let cursor = self.cursor();
        let new_cursor = self.buffer_mut().insert_char(cursor, ch);
        self.window_mut().cursor = new_cursor;
    }

    pub(crate) fn execute_insert_newline(&mut self) {
        // Basic heuristic approach to detect 'default' indentation
        // TODO: Hopefully handled by LSP
        let cursor = self.cursor();
        let line_idx = self.buffer().char_to_line(cursor);
        let line_start = self.buffer().line_to_char(line_idx);
        let indent_width = self.buffer().indent_width() as usize;
        let use_tabs = self.buffer().use_tabs();
        let one_indent = if use_tabs {
            "\t".to_string()
        } else {
            " ".repeat(indent_width)
        };

        let leading_ws: String = self
            .buffer()
            .rope()
            .line(line_idx)
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();

        let line_to_cursor: String = self
            .buffer()
            .rope()
            .slice(line_start..cursor)
            .chars()
            .collect();
        let last_significant = line_to_cursor.trim_end().chars().last();

        let new_indent = match last_significant {
            Some('{') | Some('(') | Some('[') => {
                format!("{}{}", leading_ws, one_indent)
            }
            _ => leading_ws,
        };

        let new_cursor = self
            .buffer_mut()
            .insert_str(cursor, &format!("\n{}", new_indent));
        self.window_mut().cursor = new_cursor;
    }

    pub(crate) fn execute_insert_tab(&mut self) {
        let cursor = self.cursor();
        let tab_char = match self.buffer().use_tabs() {
            true => "\t",
            false => " ",
        };
        let tab_str = tab_char.repeat(self.buffer().indent_width() as usize);

        let new_cursor = self.buffer_mut().insert_str(cursor, tab_str.as_str());
        self.window_mut().cursor = new_cursor;
    }

    pub(crate) fn execute_delete_till_eol(&mut self) {
        let cursor = self.cursor();
        let new_cursor = self.buffer_mut().delete_till_eol(cursor);
        self.window_mut().cursor = new_cursor;
    }

    pub(crate) fn execute_delete_char_forward(&mut self, count: usize) {
        let mut cursor = self.cursor();
        for _ in 0..count {
            cursor = self.buffer_mut().delete_char_forward(cursor);
        }
        self.window_mut().cursor = cursor;
    }

    pub(crate) fn execute_delete_char_backward(&mut self) {
        let cursor = self.cursor();
        let new_cursor = self.buffer_mut().delete_char_backward(cursor);
        self.window_mut().cursor = new_cursor;
    }

    pub(crate) fn execute_delete_line(&mut self, count: usize) {
        let mut cursor = self.cursor();
        for _ in 0..count {
            cursor = self.buffer_mut().delete_line(cursor);
        }
        self.window_mut().cursor = cursor;
    }

    pub(crate) fn execute_delete_range(&mut self, range: Range) {
        let cursor = self.cursor();
        let (new_cursor, deleted) = self.buffer_mut().delete_range(cursor, range.start, range.end);
        self.window_mut().cursor = new_cursor;
        self.registers.unnamed = deleted;
    }

    pub(crate) fn execute_change_range(&mut self, range: Range) {
        let cursor = self.cursor();
        self.buffer_mut().start_edit_group(cursor);
        let (new_cursor, deleted) = self.buffer_mut().delete_range(cursor, range.start, range.end);
        self.window_mut().cursor = new_cursor;
        self.registers.unnamed = deleted;
    }

    pub(crate) fn execute_yank_range(&mut self, range: Range) {
        let text = self.buffer().yank_range(range.start, range.end);
        self.registers.unnamed = text;
    }

    pub(crate) fn execute_replace_char(&mut self, ch: char) {
        let cursor = self.cursor();
        let new_cursor = self.buffer_mut().replace_char(cursor, ch);
        self.window_mut().cursor = new_cursor;
    }

    pub(crate) fn execute_paste(&mut self, before: bool) {
        let text = self.registers.unnamed.clone();
        let cursor = self.cursor();
        let new_cursor = if before {
            self.buffer_mut().paste_before(cursor, &text)
        } else {
            self.buffer_mut().paste_after(cursor, &text)
        };
        self.window_mut().cursor = new_cursor;
    }

    pub(crate) fn execute_undo(&mut self) {
        if let Some(new_cursor) = self.buffer_mut().undo() {
            self.window_mut().cursor = new_cursor;
        }
    }

    pub(crate) fn execute_redo(&mut self) {
        if let Some(new_cursor) = self.buffer_mut().redo() {
            self.window_mut().cursor = new_cursor;
        }
    }

    pub(crate) fn execute_start_edit_group(&mut self) {
        let cursor = self.cursor();
        self.buffer_mut().start_edit_group(cursor);
    }

    pub(crate) fn execute_finish_edit_group(&mut self) {
        self.buffer_mut().finish_edit_group();
    }

    pub(crate) fn execute_update_visual_selection(&mut self, anchor: usize, mode: VimMode) {
        let cursor = self.cursor();
        let buf = self.buffer();
        let selection = if mode == VimMode::VisualLine {
            let anchor_line = buf.char_to_line(anchor);
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
        } else if anchor <= cursor {
            (anchor, cursor + 1)
        } else {
            (cursor, anchor + 1)
        };
        self.window_mut().selection = Some(selection);
    }

    pub(crate) fn system_copy(&mut self, cut: bool) {
        let win = self.workspace().window();
        let buf_id = win.buffer_id;
        let text = if let Some((start, end)) = win.selection {
            let yanked = self.buffers[buf_id].yank_range(start, end);
            if cut {
                let cursor = win.cursor;
                let (new_cursor, _) = self.buffers[buf_id].delete_range(cursor, start, end);
                self.workspace_mut().window_mut().cursor = new_cursor;
                self.workspace_mut().window_mut().selection = None;
            }
            yanked
        } else {
            let cursor = win.cursor;
            let line = self.buffers[buf_id].char_to_line(cursor);
            let line_start = self.buffers[buf_id].line_to_char(line);
            let line_end = if line + 1 < self.buffers[buf_id].total_lines() {
                self.buffers[buf_id].line_to_char(line + 1)
            } else {
                self.buffers[buf_id].len_chars()
            };
            let yanked = self.buffers[buf_id].yank_range(line_start, line_end);
            if cut {
                let (new_cursor, _) =
                    self.buffers[buf_id].delete_range(cursor, line_start, line_end);
                self.workspace_mut().window_mut().cursor = new_cursor;
            }
            yanked
        };
        self.registers.unnamed = text.clone();
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(text);
        }
    }

    pub(crate) fn system_paste(&mut self) {
        let text = if let Ok(mut clipboard) = arboard::Clipboard::new() {
            clipboard.get_text().unwrap_or_default()
        } else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let cursor = self.cursor();
        let new_cursor = self.buffer_mut().insert_str(cursor, &text);
        self.window_mut().cursor = new_cursor;
    }
}
