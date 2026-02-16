use crate::layout::SplitDirection;
use crate::text_grid;
use crate::action::EditorEffect;

use super::Editor;

const SCROLL_SPEED: f32 = 0.5;

impl Editor {
    // --- Window management actions ---

    pub(crate) fn execute_vsplit(&mut self) {
        self.workspace_mut().split(SplitDirection::Vertical);
    }

    pub(crate) fn execute_hsplit(&mut self) {
        self.workspace_mut().split(SplitDirection::Horizontal);
    }

    pub(crate) fn execute_close_window(&mut self) -> EditorEffect {
        let ws = &mut self.workspaces[self.active_workspace];
        if ws.layout.leaf_count() > 1 {
            ws.close_window();
        } else if self.has_unsaved_changes() {
            self.status_message = String::from(
                "Unsaved changes! Use :q! to force quit, or :wq to save and close",
            );
        } else {
            return EditorEffect::Task(iced::exit());
        }
        EditorEffect::None
    }

    pub(crate) fn execute_focus_left(&mut self) {
        self.workspace_mut()
            .focus_direction(SplitDirection::Vertical, false);
    }

    pub(crate) fn execute_focus_right(&mut self) {
        self.workspace_mut()
            .focus_direction(SplitDirection::Vertical, true);
    }

    pub(crate) fn execute_focus_up(&mut self) {
        self.workspace_mut()
            .focus_direction(SplitDirection::Horizontal, false);
    }

    pub(crate) fn execute_focus_down(&mut self) {
        self.workspace_mut()
            .focus_direction(SplitDirection::Horizontal, true);
    }

    // --- Viewport operations (pushed down from app.rs) ---

    pub fn scroll_lines(&mut self, window_id: usize, delta: f32) {
        let ws = self.workspace_mut();
        if window_id >= ws.windows.len() {
            return;
        }
        let buf_id = ws.windows[window_id].buffer_id;
        let old_scroll = ws.windows[window_id].scroll_y;
        let cursor = ws.windows[window_id].cursor;
        let total = self.buffers[buf_id].total_lines();
        let (cur_line, cur_col) = self.buffers[buf_id].cursor_position(cursor);

        let ws = self.workspace_mut();
        ws.windows[window_id].scroll_lines(delta, SCROLL_SPEED, total);
        let new_scroll = ws.windows[window_id].scroll_y;
        let scroll_delta = new_scroll as isize - old_scroll as isize;
        if scroll_delta != 0 {
            let new_line = (cur_line as isize + scroll_delta).max(0) as usize;
            let new_cursor =
                self.buffers[buf_id].cursor_from_position(new_line, cur_col);
            self.workspace_mut().windows[window_id].cursor = new_cursor;
        }
    }

    pub fn scroll_cols(&mut self, window_id: usize, delta: f32) {
        let ws = self.workspace_mut();
        if window_id >= ws.windows.len() {
            return;
        }
        let buf_id = ws.windows[window_id].buffer_id;
        let max_len = self.buffers[buf_id].max_line_len();
        self.workspace_mut().windows[window_id].scroll_cols(
            delta,
            SCROLL_SPEED,
            max_len,
        );
    }

    pub fn handle_click(&mut self, window_id: usize, x: f32, y: f32) {
        let ws = self.workspace_mut();
        ws.active_window = window_id;
        if window_id >= ws.windows.len() {
            return;
        }
        let win = &ws.windows[window_id];
        let line = win.scroll_y + (y / text_grid::LINE_HEIGHT) as usize;
        let col = win.scroll_x
            + ((x - text_grid::GUTTER_WIDTH - 8.0).max(0.0) / text_grid::CHAR_WIDTH)
                as usize;
        let buf_id = win.buffer_id;
        let new_cursor = self.buffers[buf_id].cursor_from_position(line, col);
        self.workspace_mut().windows[window_id].cursor = new_cursor;
        self.ensure_cursor_visible();
    }

    pub fn handle_resize(&mut self, window_id: usize, lines: usize, cols: usize) {
        let mut resized = false;
        {
            let ws = self.workspace_mut();
            if window_id < ws.windows.len() {
                let win = &mut ws.windows[window_id];
                if win.visible_lines != lines || win.visible_cols != cols {
                    win.visible_lines = lines;
                    win.visible_cols = cols;
                    resized = true;
                }
            }
        }
        if resized {
            self.ensure_cursor_visible();
        }
    }
}
