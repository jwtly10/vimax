use crate::action::{BufferQuery, Motion};
use crate::buffer::Buffer;
use crate::viewport::Viewport;

pub struct Window {
    pub buffer_id: usize,
    pub cursor: usize,
    pub viewport: Viewport,
    pub selection: Option<(usize, usize)>,
    pub search_matches: Vec<usize>,
    pub search_version: u64,
    pub search_cached_pattern: String,
}

impl Window {
    pub fn new(buffer_id: usize) -> Self {
        Self {
            buffer_id,
            cursor: 0,
            viewport: Viewport::new(),
            selection: None,
            search_matches: Vec::new(),
            search_version: u64::MAX,
            search_cached_pattern: String::new(),
        }
    }

    pub fn cursor_position(&self, buffer: &Buffer) -> (usize, usize) {
        buffer.cursor_position(self.cursor)
    }

    pub fn ensure_cursor_visible(&mut self, buffer: &Buffer) {
        let (cursor_line, cursor_col) = self.cursor_position(buffer);
        let total = buffer.total_lines();
        self.viewport
            .ensure_cursor_visible(cursor_line, cursor_col, total);
    }

    pub fn update_search_cache(&mut self, buffer: &Buffer, pattern: &str) {
        let version = buffer.version();
        if pattern == self.search_cached_pattern && version == self.search_version {
            return;
        }
        self.search_matches = buffer.find_all(pattern);
        self.search_cached_pattern = pattern.to_string();
        self.search_version = version;
    }
}

pub struct WindowView<'a> {
    pub buffer: &'a Buffer,
    pub cursor: usize,
}

impl BufferQuery for WindowView<'_> {
    fn cursor(&self) -> usize {
        self.cursor
    }

    fn cursor_position(&self) -> (usize, usize) {
        self.buffer.cursor_position(self.cursor)
    }

    fn len_chars(&self) -> usize {
        self.buffer.len_chars()
    }

    fn total_lines(&self) -> usize {
        self.buffer.total_lines()
    }

    fn line_to_char(&self, line: usize) -> usize {
        self.buffer.line_to_char(line)
    }

    fn char_to_line(&self, pos: usize) -> usize {
        self.buffer.char_to_line(pos)
    }

    fn text_object_inner_word(&self) -> (usize, usize) {
        self.buffer.text_object_inner_word(self.cursor)
    }

    fn text_object_a_word(&self) -> (usize, usize) {
        self.buffer.text_object_a_word(self.cursor)
    }

    fn text_object_delimited(&self, open: char, close: char, include: bool) -> (usize, usize) {
        self.buffer
            .text_object_delimited(self.cursor, open, close, include)
    }

    fn text_object_quoted(&self, quote: char, include: bool) -> (usize, usize) {
        self.buffer.text_object_quoted(self.cursor, quote, include)
    }

    fn cursor_after_motion(&self, motion: &Motion, count: usize) -> usize {
        self.buffer.cursor_after_motion(self.cursor, motion, count)
    }
}
