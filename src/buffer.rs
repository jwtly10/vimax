use std::path::Path;

use ropey::Rope;

pub struct Buffer {
    rope: Rope,
    read_only: bool,
    cursor: usize,
    name: String,
    file_path: Option<String>,
    modified: bool,
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

    pub fn insert_char(&mut self, ch: char) {
        self.modified = true;
        self.rope.insert_char(self.cursor, ch);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        self.modified = true;
        self.rope.insert(self.cursor, s);
        self.cursor += s.chars().count();
    }

    pub fn delete_char_backward(&mut self) {
        if self.cursor > 0 {
            self.modified = true;
            self.cursor -= 1;
            self.rope.remove(self.cursor..self.cursor + 1);
        }
    }

    pub fn delete_char_forward(&mut self) {
        if self.cursor < self.rope.len_chars() {
            self.modified = true;
            self.rope.remove(self.cursor..self.cursor + 1);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.rope.len_chars() {
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
        } else {
            anyhow::bail!("Cannot save buffer without a file path");
        }

        Ok(())
    }
}
