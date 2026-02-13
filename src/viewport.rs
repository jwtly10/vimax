const SCROLL_MARGIN: usize = 5;

pub struct Viewport {
    pub scroll_y: usize,
    pub scroll_x: usize,
    pub visible_lines: usize,
    pub visible_cols: usize,
}

impl Viewport {
    pub fn new() -> Self {
        Self {
            scroll_y: 0,
            scroll_x: 0,
            visible_lines: 40,
            visible_cols: 80,
        }
    }

    pub fn ensure_cursor_visible(
        &mut self,
        cursor_line: usize,
        cursor_col: usize,
        total_lines: usize,
    ) {
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

        let max_scroll = total_lines.saturating_sub(1);
        self.scroll_y = self.scroll_y.min(max_scroll);

        let h_margin = 10_usize;
        if cursor_col < self.scroll_x {
            self.scroll_x = cursor_col.saturating_sub(h_margin);
        } else if cursor_col >= self.scroll_x + self.visible_cols {
            self.scroll_x = cursor_col + 1 + h_margin - self.visible_cols;
        }
    }

    pub fn scroll_lines(&mut self, delta: f32, speed: f32, total_lines: usize) {
        let new_y = self.scroll_y as f32 - delta * speed;
        let max_scroll = total_lines.saturating_sub(1);
        self.scroll_y = (new_y.max(0.0) as usize).min(max_scroll);
    }

    pub fn scroll_cols(&mut self, delta: f32, speed: f32, max_line_len: usize) {
        let new_x = (self.scroll_x as f32 - delta * speed).round();
        let max_scroll = max_line_len.saturating_sub(1);
        self.scroll_x = (new_x.max(0.0) as usize).min(max_scroll);
    }
}
