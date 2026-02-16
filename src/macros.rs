/// Borrow the active window and its buffer simultaneously (immutable).
#[macro_export]
macro_rules! current_ref {
    ($editor:expr) => {{
        let ws = &$editor.workspaces[$editor.active_workspace];
        let win = &ws.windows[ws.active_window];
        let buf = &$editor.buffers[win.buffer_id];
        (win, buf)
    }};
}

/// Borrow the active window (mutable) and its buffer (mutable) simultaneously.
#[macro_export]
macro_rules! current_mut {
    ($editor:expr) => {{
        let ws = &mut $editor.workspaces[$editor.active_workspace];
        let win = &mut ws.windows[ws.active_window];
        let buf_id = win.buffer_id;
        let buf = &mut $editor.buffers[buf_id];
        (win, buf, buf_id)
    }};
}

/// Borrow the active window (mutable) and its buffer (immutable) simultaneously.
#[macro_export]
macro_rules! current_win_mut {
    ($editor:expr) => {{
        let ws = &mut $editor.workspaces[$editor.active_workspace];
        let win = &mut ws.windows[ws.active_window];
        let buf_id = win.buffer_id;
        let buf = &$editor.buffers[buf_id];
        (win, buf)
    }};
}
