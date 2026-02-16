/// Borrow the active window and its buffer simultaneously (immutable).
///
/// This macro exists because `editor.buffer()` and `editor.workspace().window()`
/// both borrow `&self`, meaning you can't hold references to both the window and
/// its buffer at the same time through method calls. The macro expands inline so
/// Rust can see that `editor.workspaces` and `editor.buffers` are disjoint borrows.
///
/// Returns `(&Window, &Buffer)`.
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
///
/// This is the mutable version of `current_ref!`. It allows you to read from the
/// buffer and write to the window cursor in a single expression, which is impossible
/// through `&mut self` method calls.
///
/// Returns `(&mut Window, &mut Buffer, usize)` — the third element is the buffer id,
/// since you sometimes need it for syntax states or other per-buffer lookups.
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
///
/// Useful for operations that only read the buffer but need to update the window,
/// like cursor movement.
///
/// Returns `(&mut Window, &Buffer)`.
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
