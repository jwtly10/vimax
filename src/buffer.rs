use crate::undo::{EditKind, UndoStack};

use std::path::Path;

use ropey::Rope;
use tracing::debug;

pub struct Buffer {
    rope: Rope,
    read_only: bool,
    cursor: usize,
    name: String,
    file_path: Option<String>,
    modified: bool,
    undo_stack: UndoStack,
    pub yank_register: String,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            read_only: false,
            cursor: 0,
            name: String::from("untitled"),
            file_path: None,
            modified: false,
            undo_stack: UndoStack::new(),
            yank_register: String::new(),
        }
    }

    pub fn from_str(s: &str, buf_name: &str, file_path: &Path, read_only: bool) -> Self {
        Self {
            rope: Rope::from_str(s),
            read_only,
            cursor: 0,
            name: String::from(buf_name),
            file_path: Some(file_path.to_string_lossy().to_string()),
            modified: false,
            undo_stack: UndoStack::new(),
            yank_register: String::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns 0-indexed (line, col)
    pub fn cursor_position(&self) -> (usize, usize) {
        let line = self.rope.char_to_line(self.cursor);
        let line_start = self.rope.line_to_char(line);
        let col = self.cursor - line_start;
        (line, col)
    }

    pub fn start_edit_group(&mut self) {
        self.undo_stack.start_group(self.cursor);
    }

    pub fn finish_edit_group(&mut self) {
        self.undo_stack.finish_group();
    }

    /// Undo the last edit group.
    pub fn undo(&mut self) {
        if let Some(group) = self.undo_stack.undo.pop() {
            for edit in group.edits.iter().rev() {
                match edit {
                    EditKind::Insert { pos, text } => {
                        self.rope.remove(*pos..*pos + text.len());
                    }
                    EditKind::Delete { pos, text } => {
                        self.rope.insert(*pos, text);
                    }
                }
            }
            self.cursor = group.cursor_before;
            self.modified = true;
            self.undo_stack.redo.push(group);
        }
    }

    /// Redo the last undone edit group.
    pub fn redo(&mut self) {
        if let Some(group) = self.undo_stack.redo.pop() {
            for edit in group.edits.iter() {
                match edit {
                    EditKind::Insert { pos, text } => {
                        self.rope.insert(*pos, text);
                    }
                    EditKind::Delete { pos, text } => {
                        self.rope.remove(*pos..*pos + text.len());
                    }
                }
            }
            // Place cursor at the end of the last edit
            if let Some(last) = group.edits.last() {
                match last {
                    EditKind::Insert { pos, text } => self.cursor = *pos + text.len(),
                    EditKind::Delete { pos, .. } => self.cursor = *pos,
                }
            }
            self.modified = true;
            self.undo_stack.undo.push(group);
        }
    }

    //  Mutations that record to undo stack

    pub fn insert_char(&mut self, ch: char) {
        self.modified = true;
        let pos = self.cursor;
        self.rope.insert_char(pos, ch);
        self.cursor += 1;
        self.undo_stack.record_insert(pos, ch);
    }

    pub fn insert_str(&mut self, s: &str) {
        self.modified = true;
        let pos = self.cursor;
        self.rope.insert(pos, s);
        self.cursor += s.chars().count();
        self.undo_stack.record_insert_str(pos, s);
    }

    pub fn delete_line(&mut self) {
        let cursor_before = self.cursor;
        let (line, _) = self.cursor_position();
        let line_start = self.rope.line_to_char(line);
        let line_end = if line + 1 < self.rope.len_lines() {
            self.rope.line_to_char(line + 1)
        } else {
            self.rope.len_chars()
        };
        if line_start == line_end {
            return;
        }
        let deleted: String = self.rope.slice(line_start..line_end).into();
        self.rope.remove(line_start..line_end);
        self.modified = true;
        self.cursor = line_start.min(self.rope.len_chars());
        self.undo_stack.push_edit(
            EditKind::Delete {
                pos: line_start,
                text: deleted,
            },
            cursor_before,
        );
    }

    pub fn delete_char_backward(&mut self) {
        if self.cursor > 0 {
            self.modified = true;
            self.cursor -= 1;
            let ch: String = self.rope.slice(self.cursor..self.cursor + 1).into();
            self.rope.remove(self.cursor..self.cursor + 1);
            self.undo_stack.record_delete(self.cursor, &ch);
        }
    }

    pub fn delete_char_forward(&mut self) {
        if self.cursor < self.rope.len_chars() {
            self.modified = true;
            let ch: String = self.rope.slice(self.cursor..self.cursor + 1).into();
            self.rope.remove(self.cursor..self.cursor + 1);
            // Normal mode `x` — push as immediate edit group
            if self.undo_stack.pending.is_none() {
                self.undo_stack.push_edit(
                    EditKind::Delete {
                        pos: self.cursor,
                        text: ch,
                    },
                    self.cursor,
                );
            } else {
                self.undo_stack.record_delete(self.cursor, &ch);
            }
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            let (_, col) = self.cursor_position();
            if col == 0 {
                return;
            }
            self.cursor -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.rope.len_chars() {
            let (line, col) = self.cursor_position();
            if col == self.line_len_no_newline(line) {
                return;
            }
            self.cursor += 1;
        }
    }

    pub fn move_up(&mut self) {
        let (line, col) = self.cursor_position();
        if line > 0 {
            let prev_line = line - 1;
            let prev_line_len = self.line_len_no_newline(prev_line);
            let new_col = col.min(prev_line_len);
            self.cursor = self.rope.line_to_char(prev_line) + new_col;
        }
    }

    pub fn move_down(&mut self) {
        let (line, col) = self.cursor_position();
        let total_lines = self.rope.len_lines();
        if line + 1 < total_lines {
            let next_line = line + 1;
            let next_line_len = self.line_len_no_newline(next_line);
            let new_col = col.min(next_line_len);
            self.cursor = self.rope.line_to_char(next_line) + new_col;
        }
    }

    pub fn move_to_line_start(&mut self) {
        let line = self.rope.char_to_line(self.cursor);
        self.cursor = self.rope.line_to_char(line);
    }

    pub fn move_to_line_end(&mut self) {
        let line = self.rope.char_to_line(self.cursor);
        let line_end = self.rope.line_to_char(line) + self.line_len_no_newline(line);
        self.cursor = line_end;
    }

    pub fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    pub fn move_to_end(&mut self) {
        self.cursor = self.rope.len_chars();
    }

    /// Vim `^` — move to first non-whitespace character on the line.
    pub fn move_to_first_non_whitespace(&mut self) {
        let line = self.rope.char_to_line(self.cursor);
        let line_start = self.rope.line_to_char(line);
        let line_slice = self.rope.line(line);
        let mut offset = 0;
        for ch in line_slice.chars() {
            if ch == ' ' || ch == '\t' {
                offset += 1;
            } else {
                break;
            }
        }
        self.cursor = line_start + offset;
    }

    /// Vim `w` — move to start of next word.
    pub fn move_word_forward(&mut self) {
        let len = self.rope.len_chars();
        if self.cursor >= len {
            return;
        }
        // Skip current word (non-whitespace)
        while self.cursor < len && !self.char_at(self.cursor).is_whitespace() {
            self.cursor += 1;
        }
        // Skip whitespace
        while self.cursor < len && self.char_at(self.cursor).is_whitespace() {
            self.cursor += 1;
        }
    }

    /// Vim `b` — move to start of previous word.
    pub fn move_word_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Skip whitespace before cursor
        while self.cursor > 0 && self.char_at(self.cursor - 1).is_whitespace() {
            self.cursor -= 1;
        }
        // Skip word (non-whitespace)
        while self.cursor > 0 && !self.char_at(self.cursor - 1).is_whitespace() {
            self.cursor -= 1;
        }
    }

    pub fn total_lines(&self) -> usize {
        self.rope.len_lines()
    }

    // TODO: Perf nightmare
    pub fn max_line_len(&self) -> usize {
        (0..self.rope.len_lines())
            .map(|line| self.line_len_no_newline(line))
            .max()
            .unwrap_or(0)
    }

    pub fn set_cursor_position(&mut self, line: usize, col: usize) {
        let total = self.rope.len_lines();
        let line = line.min(if total > 0 { total - 1 } else { 0 });
        let line_len = self.line_len_no_newline(line);
        let col = col.min(line_len);
        self.cursor = self.rope.line_to_char(line) + col;
    }

    fn char_at(&self, idx: usize) -> char {
        self.rope.char(idx)
    }

    /// Returns the length of the line without counting a trailing newline character, if present.
    fn line_len_no_newline(&self, line: usize) -> usize {
        let line_slice = self.rope.line(line);
        let len = line_slice.len_chars();
        if len > 0 {
            let last_char = line_slice.char(len - 1);
            if last_char == '\n' || last_char == '\r' {
                len - 1
            } else {
                len
            }
        } else {
            0
        }
    }

    /// Delete a range of characters [start, end), recording to undo stack.
    pub fn delete_range(&mut self, start: usize, end: usize) {
        if start >= end || start >= self.rope.len_chars() {
            return;
        }
        let end = end.min(self.rope.len_chars());
        let cursor_before = self.cursor;
        let deleted: String = self.rope.slice(start..end).into();
        self.yank_register = deleted.clone();
        self.rope.remove(start..end);
        self.modified = true;
        self.cursor = start.min(self.rope.len_chars().saturating_sub(1));
        self.undo_stack.push_edit(
            EditKind::Delete {
                pos: start,
                text: deleted,
            },
            cursor_before,
        );
    }

    /// Yank (copy) a range of characters [start, end) without modifying buffer.
    pub fn yank_range(&mut self, start: usize, end: usize) -> String {
        let end = end.min(self.rope.len_chars());
        if start >= end {
            return String::new();
        }
        let text: String = self.rope.slice(start..end).into();
        self.yank_register = text.clone();
        text
    }

    /// Paste yank register contents after cursor.
    pub fn paste_after(&mut self) {
        if self.yank_register.is_empty() {
            return;
        }
        let cursor_before = self.cursor;
        let text = self.yank_register.clone();
        let linewise = text.ends_with('\n');
        let pos = if linewise {
            // Paste on the line below
            let line = self.rope.char_to_line(self.cursor);
            if line + 1 < self.rope.len_lines() {
                self.rope.line_to_char(line + 1)
            } else {
                self.rope.len_chars()
            }
        } else {
            (self.cursor + 1).min(self.rope.len_chars())
        };
        self.rope.insert(pos, &text);
        if linewise {
            self.cursor = pos;
        } else {
            self.cursor = pos + text.chars().count() - 1;
        }
        self.modified = true;
        self.undo_stack
            .push_edit(EditKind::Insert { pos, text }, cursor_before);
    }

    /// Paste yank register contents before cursor.
    pub fn paste_before(&mut self) {
        if self.yank_register.is_empty() {
            return;
        }
        let cursor_before = self.cursor;
        let text = self.yank_register.clone();
        let linewise = text.ends_with('\n');
        let pos = if linewise {
            // Paste on the line above
            let line = self.rope.char_to_line(self.cursor);
            self.rope.line_to_char(line)
        } else {
            self.cursor
        };
        self.rope.insert(pos, &text);
        if linewise {
            self.cursor = pos;
        } else {
            self.cursor = pos + text.chars().count() - 1;
        }
        self.modified = true;
        self.undo_stack
            .push_edit(EditKind::Insert { pos, text }, cursor_before);
    }

    /// `iw` text object: inner word around cursor.
    /// Returns (start, end) char offsets.
    pub fn text_object_inner_word(&self) -> (usize, usize) {
        let len = self.rope.len_chars();
        if len == 0 {
            return (0, 0);
        }
        let pos = self.cursor.min(len.saturating_sub(1));
        let ch = self.rope.char(pos);
        let is_word_char = |c: char| !c.is_whitespace();
        let classifier: fn(char) -> bool = if is_word_char(ch) {
            is_word_char
        } else {
            |c: char| c.is_whitespace()
        };

        let mut start = pos;
        while start > 0 && classifier(self.rope.char(start - 1)) {
            start -= 1;
        }
        let mut end = pos;
        while end < len && classifier(self.rope.char(end)) {
            end += 1;
        }
        (start, end)
    }

    /// `aw` text object: a word around cursor (word + trailing whitespace).
    /// Returns (start, end) char offsets.
    pub fn text_object_a_word(&self) -> (usize, usize) {
        let (start, mut end) = self.text_object_inner_word();
        let len = self.rope.len_chars();
        // Include trailing whitespace
        while end < len && self.rope.char(end).is_whitespace() {
            end += 1;
        }
        // If no trailing whitespace was consumed, try leading whitespace
        if end == self.text_object_inner_word().1 {
            let mut new_start = start;
            while new_start > 0 && self.rope.char(new_start - 1).is_whitespace() {
                new_start -= 1;
            }
            return (new_start, end);
        }
        (start, end)
    }

    /// Set cursor to a raw char offset (clamped to valid range).
    pub fn set_cursor(&mut self, pos: usize) {
        self.cursor = pos.min(self.rope.len_chars().saturating_sub(1));
    }

    /// Delimited text object: find matching `open`/`close` pair around cursor.
    /// Returns (start, end) char offsets, or cursor pos if no match found.
    pub fn text_object_delimited(&self, open: char, close: char, include: bool) -> (usize, usize) {
        let len = self.rope.len_chars();
        if len == 0 {
            return (0, 0);
        }
        let pos = self.cursor.min(len.saturating_sub(1));

        // Search backward for unmatched open delimiter
        let mut depth = 0i32;
        let mut open_pos = None;
        let mut i = pos;
        loop {
            let ch = self.rope.char(i);
            if ch == close && i != pos {
                depth += 1;
            } else if ch == open {
                if depth == 0 {
                    open_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }

        let open_pos = match open_pos {
            Some(p) => p,
            None => return (pos, pos),
        };

        // Search forward for matching close delimiter
        depth = 0;
        let mut close_pos = None;
        for j in (open_pos + 1)..len {
            let ch = self.rope.char(j);
            if ch == open {
                depth += 1;
            } else if ch == close {
                if depth == 0 {
                    close_pos = Some(j);
                    break;
                }
                depth -= 1;
            }
        }

        let close_pos = match close_pos {
            Some(p) => p,
            None => return (pos, pos),
        };

        if include {
            (open_pos, close_pos + 1)
        } else {
            (open_pos + 1, close_pos)
        }
    }

    /// Quoted text object: find matching quote pair on current line containing cursor.
    /// Returns (start, end) char offsets, or cursor pos if no match found.
    pub fn text_object_quoted(&self, quote: char, include: bool) -> (usize, usize) {
        let len = self.rope.len_chars();
        if len == 0 {
            return (0, 0);
        }
        let pos = self.cursor.min(len.saturating_sub(1));
        let line = self.rope.char_to_line(pos);
        let line_start = self.rope.line_to_char(line);
        let line_slice = self.rope.line(line);
        let line_len = line_slice.len_chars();

        // Collect positions of all quote chars on this line
        let mut quotes = Vec::new();
        for i in 0..line_len {
            if line_slice.char(i) == quote {
                quotes.push(line_start + i);
            }
        }

        // Find the pair that contains the cursor
        let mut pair_idx = 0;
        while pair_idx + 1 < quotes.len() {
            let q_start = quotes[pair_idx];
            let q_end = quotes[pair_idx + 1];
            if pos >= q_start && pos <= q_end {
                return if include {
                    (q_start, q_end + 1)
                } else {
                    (q_start + 1, q_end)
                };
            }
            pair_idx += 2;
        }

        (pos, pos)
    }

    /// Saves a known buffer to its file path
    pub fn save(&mut self) -> anyhow::Result<()> {
        if self.read_only {
            anyhow::bail!("Cannot save read-only buffer");
        }

        if let Some(ref path) = self.file_path {
            let temp_path = format!("{}.tmp", path);
            let file = std::fs::File::create(&temp_path)?;
            let writer = std::io::BufWriter::new(file);
            self.rope.write_to(writer)?;
            std::fs::rename(temp_path, path)?;
            self.modified = false;
            debug!("Saved buffer '{}' to '{}'", self.name, path);
        } else {
            anyhow::bail!("Cannot save buffer without a file path");
        }

        Ok(())
    }
}
