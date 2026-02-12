use std::path::PathBuf;

use crate::layout::{LayoutNode, SplitDirection};
use crate::viewport::Viewport;
use crate::window::Window;

pub struct Workspace {
    pub windows: Vec<Window>,
    pub active_window: usize,
    pub layout: LayoutNode,
    pub cwd: PathBuf,
}

impl Workspace {
    pub fn new(buffer_id: usize, cwd: PathBuf) -> Self {
        Self {
            windows: vec![Window::new(buffer_id)],
            active_window: 0,
            layout: LayoutNode::single(0),
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

    pub fn split(&mut self, direction: SplitDirection) {
        let current_buf_id = self.windows[self.active_window].buffer_id;
        let new_window = Window::new(current_buf_id);
        let new_id = self.windows.len();
        self.windows.push(new_window);
        self.layout.split_leaf(self.active_window, new_id, direction);
        self.active_window = new_id;
    }

    pub fn close_window(&mut self) -> bool {
        if self.layout.leaf_count() <= 1 {
            return false;
        }
        let removed = self.active_window;
        self.layout.remove_leaf(removed);
        self.windows.remove(removed);
        self.layout.fix_ids_after_remove(removed);
        if self.active_window >= self.windows.len() || self.active_window == removed {
            self.active_window = self.layout.first_leaf();
        }
        true
    }

    pub fn focus_direction(&mut self, direction: SplitDirection, forward: bool) {
        if let Some(target) = self.layout.neighbor(self.active_window, direction, forward) {
            self.active_window = target;
        }
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
