use crate::action::Motion;

use super::Editor;

impl Editor {
    pub(crate) fn move_cursor(&mut self, motion: &Motion, count: usize) {
        let ws = &self.workspaces[self.active_workspace];
        let win = ws.window();
        let buf_id = win.buffer_id;
        let cursor = win.cursor;
        let buffer = &self.buffers[buf_id];

        let new_cursor = match motion {
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
                let half = (win.visible_lines / 2).max(1) * count;
                let mut c = cursor;
                for _ in 0..half {
                    c = buffer.move_down(c);
                }
                c
            }
            Motion::HalfPageUp => {
                let half = (win.visible_lines / 2).max(1) * count;
                let mut c = cursor;
                for _ in 0..half {
                    c = buffer.move_up(c);
                }
                c
            }
        };

        self.workspace_mut().window_mut().cursor = new_cursor;
    }

    pub(crate) fn execute_set_cursor(&mut self, pos: usize) {
        self.window_mut().cursor = self.buffer().clamp_cursor(pos);
    }

    pub(crate) fn execute_search_next(&mut self, count: usize) {
        self.push_jump();
        self.search_next(count);
    }

    pub(crate) fn execute_search_prev(&mut self, count: usize) {
        self.push_jump();
        self.search_prev(count);
    }

    pub(crate) fn execute_clear_search(&mut self) {
        self.search_pattern.clear();
        let win = self.window_mut();
        win.search_matches.clear();
        win.search_cached_pattern.clear();
        self.status_message.clear();
    }

    fn search_next(&mut self, count: usize) {
        if self.search_pattern.is_empty() {
            self.status_message = String::from("No search pattern");
            return;
        }
        self.update_search_cache();
        let win = self.workspace().window();
        if win.search_matches.is_empty() {
            self.status_message = format!("/{} [0/0]", self.search_pattern);
            return;
        }
        let cursor_before = win.cursor;
        let mut cursor = win.cursor;
        let matches = &win.search_matches;
        for _ in 0..count {
            let next = matches.iter().find(|&&m| m > cursor).or(matches.first());
            if let Some(&pos) = next {
                cursor = pos;
            }
        }
        self.workspace_mut().window_mut().cursor = cursor;

        let matches = &self.workspace().window().search_matches;
        let total = matches.len();
        let current = matches
            .iter()
            .position(|&m| m == cursor)
            .map(|i| i + 1)
            .unwrap_or(0);
        let wrapped = if cursor <= cursor_before && count > 0 {
            " [wrapped]"
        } else {
            ""
        };
        self.status_message = format!(
            "/{} [{}/{}]{}",
            self.search_pattern, current, total, wrapped
        );
    }

    fn search_prev(&mut self, count: usize) {
        if self.search_pattern.is_empty() {
            self.status_message = String::from("No search pattern");
            return;
        }
        self.update_search_cache();
        let win = self.workspace().window();
        if win.search_matches.is_empty() {
            self.status_message = format!("?{} [0/0]", self.search_pattern);
            return;
        }
        let cursor_before = win.cursor;
        let mut cursor = win.cursor;
        let matches = &win.search_matches;
        for _ in 0..count {
            let prev = matches
                .iter()
                .rev()
                .find(|&&m| m < cursor)
                .or(matches.last());
            if let Some(&pos) = prev {
                cursor = pos;
            }
        }
        self.workspace_mut().window_mut().cursor = cursor;

        let matches = &self.workspace().window().search_matches;
        let total = matches.len();
        let current = matches
            .iter()
            .position(|&m| m == cursor)
            .map(|i| i + 1)
            .unwrap_or(0);
        let wrapped = if cursor >= cursor_before && count > 0 {
            " [wrapped]"
        } else {
            ""
        };
        self.status_message = format!(
            "?{} [{}/{}]{}",
            self.search_pattern, current, total, wrapped
        );
    }
}
