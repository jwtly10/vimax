use crate::action::Motion;
use crate::undo::{EditKind, UndoStack};

use std::path::Path;

use ropey::Rope;
use tracing::debug;

pub struct Buffer {
    rope: Rope,
    read_only: bool,
    name: String,
    file_path: Option<String>,
    modified: bool,
    undo_stack: UndoStack,
    version: u64,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            read_only: false,
            name: String::from("untitled"),
            file_path: None,
            modified: false,
            undo_stack: UndoStack::new(),
            version: 0,
        }
    }

    pub fn from_str(s: &str, buf_name: &str, file_path: &Path, read_only: bool) -> Self {
        Self {
            rope: Rope::from_str(s),
            read_only,
            name: String::from(buf_name),
            file_path: Some(file_path.to_string_lossy().to_string()),
            modified: false,
            undo_stack: UndoStack::new(),
            version: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    pub fn file_path(&self) -> Option<&str> {
        self.file_path.as_deref()
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn total_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line_to_char(&self, line: usize) -> usize {
        self.rope.line_to_char(line)
    }

    pub fn char_to_line(&self, pos: usize) -> usize {
        self.rope.char_to_line(pos)
    }

    pub fn cursor_position(&self, cursor: usize) -> (usize, usize) {
        let line = self.rope.char_to_line(cursor);
        let line_start = self.rope.line_to_char(line);
        let col = cursor - line_start;
        (line, col)
    }

    pub fn clamp_cursor(&self, cursor: usize) -> usize {
        cursor.min(self.rope.len_chars().saturating_sub(1))
    }

    pub fn cursor_from_position(&self, line: usize, col: usize) -> usize {
        let total = self.rope.len_lines();
        let line = line.min(if total > 0 { total - 1 } else { 0 });
        let line_len = self.line_len_no_newline(line);
        let col = col.min(line_len);
        self.rope.line_to_char(line) + col
    }

    pub fn start_edit_group(&mut self, cursor: usize) {
        self.undo_stack.start_group(cursor);
    }

    pub fn finish_edit_group(&mut self) {
        self.undo_stack.finish_group();
    }

    pub fn undo(&mut self) -> Option<usize> {
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
            let cursor = group.cursor_before;
            self.modified = true;
            self.version += 1;
            self.undo_stack.redo.push(group);
            Some(cursor)
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<usize> {
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
            let cursor = if let Some(last) = group.edits.last() {
                match last {
                    EditKind::Insert { pos, text } => *pos + text.len(),
                    EditKind::Delete { pos, .. } => *pos,
                }
            } else {
                0
            };
            self.modified = true;
            self.version += 1;
            self.undo_stack.undo.push(group);
            Some(cursor)
        } else {
            None
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn insert_char(&mut self, cursor: usize, ch: char) -> usize {
        self.modified = true;
        self.version += 1;
        self.rope.insert_char(cursor, ch);
        self.undo_stack.record_insert(cursor, ch);
        cursor + 1
    }

    pub fn insert_str(&mut self, cursor: usize, s: &str) -> usize {
        self.modified = true;
        self.version += 1;
        self.rope.insert(cursor, s);
        self.undo_stack.record_insert_str(cursor, s);
        cursor + s.chars().count()
    }

    pub fn delete_line(&mut self, cursor: usize) -> usize {
        let (line, _) = self.cursor_position(cursor);
        let line_start = self.rope.line_to_char(line);
        let line_end = if line + 1 < self.rope.len_lines() {
            self.rope.line_to_char(line + 1)
        } else {
            self.rope.len_chars()
        };
        if line_start == line_end {
            return cursor;
        }
        let deleted: String = self.rope.slice(line_start..line_end).into();
        self.rope.remove(line_start..line_end);
        self.modified = true;
        self.version += 1;
        let new_cursor = line_start.min(self.rope.len_chars());
        self.undo_stack.push_edit(
            EditKind::Delete {
                pos: line_start,
                text: deleted,
            },
            cursor,
        );
        new_cursor
    }

    pub fn delete_char_backward(&mut self, cursor: usize) -> usize {
        if cursor > 0 {
            self.modified = true;
            self.version += 1;
            let new_cursor = cursor - 1;
            let ch: String = self.rope.slice(new_cursor..new_cursor + 1).into();
            self.rope.remove(new_cursor..new_cursor + 1);
            self.undo_stack.record_delete(new_cursor, &ch);
            new_cursor
        } else {
            cursor
        }
    }

    pub fn delete_char_forward(&mut self, cursor: usize) -> usize {
        if cursor < self.rope.len_chars() {
            self.modified = true;
            self.version += 1;
            let ch: String = self.rope.slice(cursor..cursor + 1).into();
            self.rope.remove(cursor..cursor + 1);
            if self.undo_stack.pending.is_none() {
                self.undo_stack.push_edit(
                    EditKind::Delete {
                        pos: cursor,
                        text: ch,
                    },
                    cursor,
                );
            } else {
                self.undo_stack.record_delete(cursor, &ch);
            }
        }
        cursor
    }

    pub fn delete_range(&mut self, cursor: usize, start: usize, end: usize) -> (usize, String) {
        if start >= end || start >= self.rope.len_chars() {
            return (cursor, String::new());
        }
        let end = end.min(self.rope.len_chars());
        let deleted: String = self.rope.slice(start..end).into();
        self.rope.remove(start..end);
        self.modified = true;
        self.version += 1;
        let new_cursor = start.min(self.rope.len_chars().saturating_sub(1));

        if self.undo_stack.pending.is_some() {
            self.undo_stack.record_delete(start, &deleted);
        } else {
            self.undo_stack.push_edit(
                EditKind::Delete {
                    pos: start,
                    text: deleted.clone(),
                },
                cursor,
            );
        }
        (new_cursor, deleted)
    }

    pub fn yank_range(&self, start: usize, end: usize) -> String {
        let end = end.min(self.rope.len_chars());
        if start >= end {
            return String::new();
        }
        self.rope.slice(start..end).into()
    }

    pub fn paste_after(&mut self, cursor: usize, text: &str) -> usize {
        if text.is_empty() {
            return cursor;
        }
        let linewise = text.ends_with('\n');
        let pos = if linewise {
            let line = self.rope.char_to_line(cursor);
            if line + 1 < self.rope.len_lines() {
                self.rope.line_to_char(line + 1)
            } else {
                self.rope.len_chars()
            }
        } else {
            let len = self.rope.len_chars();
            if cursor < len && self.rope.char(cursor) == '\n' {
                cursor
            } else {
                (cursor + 1).min(len)
            }
        };
        self.rope.insert(pos, text);
        let new_cursor = if linewise {
            pos
        } else {
            pos + text.chars().count() - 1
        };
        self.modified = true;
        self.version += 1;
        self.undo_stack.push_edit(
            EditKind::Insert {
                pos,
                text: text.to_string(),
            },
            cursor,
        );
        new_cursor
    }

    pub fn paste_before(&mut self, cursor: usize, text: &str) -> usize {
        if text.is_empty() {
            return cursor;
        }
        let linewise = text.ends_with('\n');
        let pos = if linewise {
            let line = self.rope.char_to_line(cursor);
            self.rope.line_to_char(line)
        } else {
            cursor
        };
        self.rope.insert(pos, text);
        let new_cursor = if linewise {
            pos
        } else {
            pos + text.chars().count() - 1
        };
        self.modified = true;
        self.version += 1;
        self.undo_stack.push_edit(
            EditKind::Insert {
                pos,
                text: text.to_string(),
            },
            cursor,
        );
        new_cursor
    }

    pub fn replace_char(&mut self, cursor: usize, ch: char) -> usize {
        let len = self.rope.len_chars();
        if cursor >= len {
            return cursor;
        }
        let old: String = self.rope.slice(cursor..cursor + 1).into();
        self.rope.remove(cursor..cursor + 1);
        self.rope.insert_char(cursor, ch);
        self.modified = true;
        self.version += 1;
        self.undo_stack.push_edits(
            vec![
                EditKind::Delete {
                    pos: cursor,
                    text: old,
                },
                EditKind::Insert {
                    pos: cursor,
                    text: ch.to_string(),
                },
            ],
            cursor,
        );
        cursor
    }

    pub fn move_left(&self, cursor: usize) -> usize {
        if cursor > 0 {
            let (_, col) = self.cursor_position(cursor);
            if col == 0 {
                return cursor;
            }
            cursor - 1
        } else {
            cursor
        }
    }

    pub fn move_right(&self, cursor: usize) -> usize {
        if cursor < self.rope.len_chars() {
            let (line, col) = self.cursor_position(cursor);
            if col == self.line_len_no_newline(line) {
                return cursor;
            }
            cursor + 1
        } else {
            cursor
        }
    }

    pub fn move_up(&self, cursor: usize) -> usize {
        let (line, col) = self.cursor_position(cursor);
        if line > 0 {
            let prev_line = line - 1;
            let prev_line_len = self.line_len_no_newline(prev_line);
            let new_col = col.min(prev_line_len);
            self.rope.line_to_char(prev_line) + new_col
        } else {
            cursor
        }
    }

    pub fn move_down(&self, cursor: usize) -> usize {
        let (line, col) = self.cursor_position(cursor);
        let total_lines = self.rope.len_lines();
        if line + 1 < total_lines {
            let next_line = line + 1;
            let next_line_len = self.line_len_no_newline(next_line);
            let new_col = col.min(next_line_len);
            self.rope.line_to_char(next_line) + new_col
        } else {
            cursor
        }
    }

    pub fn move_to_line_start(&self, cursor: usize) -> usize {
        let line = self.rope.char_to_line(cursor);
        self.rope.line_to_char(line)
    }

    pub fn move_to_line_end(&self, cursor: usize) -> usize {
        let line = self.rope.char_to_line(cursor);
        self.rope.line_to_char(line) + self.line_len_no_newline(line)
    }

    pub fn move_to_start(&self) -> usize {
        0
    }

    pub fn move_to_end(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn move_to_first_non_whitespace(&self, cursor: usize) -> usize {
        let line = self.rope.char_to_line(cursor);
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
        line_start + offset
    }

    pub fn move_word_forward(&self, cursor: usize) -> usize {
        let len = self.rope.len_chars();
        if cursor >= len {
            return cursor;
        }

        let mut pos = cursor;
        let start_char = self.char_at(pos);

        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

        if is_word_char(start_char) {
            while pos < len && is_word_char(self.char_at(pos)) {
                pos += 1;
            }
        } else if start_char.is_whitespace() {
            while pos < len && self.char_at(pos).is_whitespace() {
                pos += 1;
            }
        } else {
            while pos < len {
                let c = self.char_at(pos);
                if is_word_char(c) || c.is_whitespace() {
                    break;
                }
                pos += 1;
            }
        }

        while pos < len && self.char_at(pos).is_whitespace() {
            pos += 1;
        }

        pos
    }

    pub fn move_word_backward(&self, cursor: usize) -> usize {
        if cursor == 0 {
            return cursor;
        }

        let mut pos = cursor;
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

        while pos > 0 && self.char_at(pos - 1).is_whitespace() {
            pos -= 1;
        }

        if pos == 0 {
            return pos;
        }

        let start_char = self.char_at(pos - 1);

        if is_word_char(start_char) {
            while pos > 0 && is_word_char(self.char_at(pos - 1)) {
                pos -= 1;
            }
        } else {
            while pos > 0 {
                let c = self.char_at(pos - 1);
                if is_word_char(c) || c.is_whitespace() {
                    break;
                }
                pos -= 1;
            }
        }

        pos
    }

    pub fn move_word_end(&self, cursor: usize) -> usize {
        let len = self.rope.len_chars();
        if cursor + 1 >= len {
            return cursor;
        }
        let mut pos = cursor + 1;
        while pos < len && self.char_at(pos).is_whitespace() {
            pos += 1;
        }
        while pos + 1 < len && !self.char_at(pos + 1).is_whitespace() {
            pos += 1;
        }
        pos
    }

    pub fn find_char_on_line(
        &self,
        cursor: usize,
        ch: char,
        forward: bool,
        stop_before: bool,
    ) -> usize {
        let (line, col) = self.cursor_position(cursor);
        let line_start = self.rope.line_to_char(line);
        let line_len = self.line_len_no_newline(line);

        if forward {
            for i in (col + 1)..line_len {
                if self.rope.char(line_start + i) == ch {
                    return line_start + if stop_before { i - 1 } else { i };
                }
            }
        } else if col > 0 {
            for i in (0..col).rev() {
                if self.rope.char(line_start + i) == ch {
                    return line_start + if stop_before { i + 1 } else { i };
                }
            }
        }
        cursor
    }

    pub fn text_object_inner_word(&self, cursor: usize) -> (usize, usize) {
        let len = self.rope.len_chars();
        if len == 0 {
            return (0, 0);
        }
        let pos = cursor.min(len.saturating_sub(1));
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

    pub fn text_object_a_word(&self, cursor: usize) -> (usize, usize) {
        let (start, mut end) = self.text_object_inner_word(cursor);
        let len = self.rope.len_chars();
        while end < len && self.rope.char(end).is_whitespace() {
            end += 1;
        }
        if end == self.text_object_inner_word(cursor).1 {
            let mut new_start = start;
            while new_start > 0 && self.rope.char(new_start - 1).is_whitespace() {
                new_start -= 1;
            }
            return (new_start, end);
        }
        (start, end)
    }

    pub fn text_object_delimited(
        &self,
        cursor: usize,
        open: char,
        close: char,
        include: bool,
    ) -> (usize, usize) {
        let len = self.rope.len_chars();
        if len == 0 {
            return (0, 0);
        }
        let pos = cursor.min(len.saturating_sub(1));

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

    pub fn text_object_quoted(&self, cursor: usize, quote: char, include: bool) -> (usize, usize) {
        let len = self.rope.len_chars();
        if len == 0 {
            return (0, 0);
        }
        let pos = cursor.min(len.saturating_sub(1));
        let line = self.rope.char_to_line(pos);
        let line_start = self.rope.line_to_char(line);
        let line_slice = self.rope.line(line);
        let line_len = line_slice.len_chars();

        let mut quotes = Vec::new();
        for i in 0..line_len {
            if line_slice.char(i) == quote {
                quotes.push(line_start + i);
            }
        }

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

    pub fn max_line_len(&self) -> usize {
        (0..self.rope.len_lines())
            .map(|line| self.line_len_no_newline(line))
            .max()
            .unwrap_or(0)
    }

    pub fn char_at(&self, idx: usize) -> char {
        self.rope.char(idx)
    }

    pub fn line_len_no_newline(&self, line: usize) -> usize {
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

    pub fn find_all(&self, query: &str) -> Vec<usize> {
        if query.is_empty() {
            return Vec::new();
        }
        let text: String = self.rope.chars().collect();
        let mut matches = Vec::new();
        let mut start = 0;
        while let Some(pos) = text[start..].find(query) {
            matches.push(start + pos);
            start += pos + 1;
        }
        matches
    }

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

    pub fn cursor_after_motion(&self, cursor: usize, motion: &Motion, count: usize) -> usize {
        let mut pos = cursor;
        let len = self.rope.len_chars();

        for _ in 0..count {
            match motion {
                Motion::Left => {
                    let line = self.rope.char_to_line(pos);
                    let line_start = self.rope.line_to_char(line);
                    if pos > line_start {
                        pos -= 1;
                    }
                }
                Motion::Right => {
                    if pos < len {
                        let line = self.rope.char_to_line(pos);
                        let line_len = self.line_len_no_newline(line);
                        let line_start = self.rope.line_to_char(line);
                        let col = pos - line_start;
                        if col < line_len {
                            pos += 1;
                        }
                    }
                }
                Motion::Up => {
                    let line = self.rope.char_to_line(pos);
                    let line_start = self.rope.line_to_char(line);
                    let col = pos - line_start;
                    if line > 0 {
                        let prev_line = line - 1;
                        let prev_len = self.line_len_no_newline(prev_line);
                        let new_col = col.min(prev_len);
                        pos = self.rope.line_to_char(prev_line) + new_col;
                    }
                }
                Motion::Down => {
                    let line = self.rope.char_to_line(pos);
                    let line_start = self.rope.line_to_char(line);
                    let col = pos - line_start;
                    if line + 1 < self.rope.len_lines() {
                        let next_line = line + 1;
                        let next_len = self.line_len_no_newline(next_line);
                        let new_col = col.min(next_len);
                        pos = self.rope.line_to_char(next_line) + new_col;
                    }
                }
                Motion::WordForward => {
                    if pos < len {
                        while pos < len && !self.rope.char(pos).is_whitespace() {
                            pos += 1;
                        }
                        while pos < len && self.rope.char(pos).is_whitespace() {
                            pos += 1;
                        }
                    }
                }
                Motion::WordBackward => {
                    if pos > 0 {
                        while pos > 0 && self.rope.char(pos - 1).is_whitespace() {
                            pos -= 1;
                        }
                        while pos > 0 && !self.rope.char(pos - 1).is_whitespace() {
                            pos -= 1;
                        }
                    }
                }
                Motion::WordEnd => {
                    if pos + 1 < len {
                        pos += 1;
                        while pos < len && self.rope.char(pos).is_whitespace() {
                            pos += 1;
                        }
                        while pos + 1 < len && !self.rope.char(pos + 1).is_whitespace() {
                            pos += 1;
                        }
                    }
                }
                Motion::LineStart => {
                    let line = self.rope.char_to_line(pos);
                    pos = self.rope.line_to_char(line);
                }
                Motion::LineEnd => {
                    let line = self.rope.char_to_line(pos);
                    pos = self.rope.line_to_char(line) + self.line_len_no_newline(line);
                }
                Motion::FirstNonWhitespace => {
                    let line = self.rope.char_to_line(pos);
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
                    pos = line_start + offset;
                }
                Motion::FileStart => {
                    pos = 0;
                }
                Motion::FileEnd => {
                    pos = len;
                }
                Motion::FindChar {
                    ch,
                    forward,
                    stop_before,
                } => {
                    let line = self.rope.char_to_line(pos);
                    let line_start = self.rope.line_to_char(line);
                    let col = pos - line_start;
                    let line_len = self.line_len_no_newline(line);

                    if *forward {
                        for i in (col + 1)..line_len {
                            if self.rope.char(line_start + i) == *ch {
                                pos = line_start + if *stop_before { i - 1 } else { i };
                                break;
                            }
                        }
                    } else if col > 0 {
                        for i in (0..col).rev() {
                            if self.rope.char(line_start + i) == *ch {
                                pos = line_start + if *stop_before { i + 1 } else { i };
                                break;
                            }
                        }
                    }
                }
                Motion::HalfPageDown | Motion::HalfPageUp => {}
            }
        }
        pos
    }
}
