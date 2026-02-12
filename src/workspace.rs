use std::path::PathBuf;

use crate::viewport::Viewport;
use crate::window::Window;

pub struct Workspace {
    pub windows: Vec<Window>,
    pub active_window: usize,
    pub cwd: PathBuf,
}

impl Workspace {
    pub fn new(buffer_id: usize, cwd: PathBuf) -> Self {
        Self {
            windows: vec![Window::new(buffer_id)],
            active_window: 0,
            cwd,
        }
    }

    pub fn window(&self) -> &Window {
        &self.windows[self.active_window]
    }

    pub fn window_mut(&mut self) -> &mut Window {
        &mut self.windows[self.active_window]
    }

    pub fn cursor(&self) -> usize {
        self.windows[self.active_window].cursor
    }

    pub fn switch_buffer(&mut self, buffer_id: usize, buf_len_chars: usize) {
        let win = &mut self.windows[self.active_window];
        win.buffer_id = buffer_id;
        win.cursor = win.cursor.min(buf_len_chars.saturating_sub(1));
        win.selection = None;
    }

    pub fn reset_window_for_buffer(&mut self, buffer_id: usize) {
        let win = &mut self.windows[self.active_window];
        win.buffer_id = buffer_id;
        win.cursor = 0;
        win.selection = None;
        win.viewport = Viewport::new();
    }

    pub fn fix_buffer_ids_after_remove(&mut self, removed: usize, buf_count: usize) {
        for win in &mut self.windows {
            if win.buffer_id == removed {
                win.buffer_id = removed.min(buf_count - 1);
                win.cursor = 0;
                win.selection = None;
            } else if win.buffer_id > removed {
                win.buffer_id -= 1;
            }
        }
    }
}
