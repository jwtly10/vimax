pub enum EditKind {
    Insert { pos: usize, text: String },
    Delete { pos: usize, text: String },
}

pub struct EditGroup {
    pub edits: Vec<EditKind>,
    pub cursor_before: usize,
}

pub struct UndoStack {
    pub(crate) undo: Vec<EditGroup>,
    pub(crate) redo: Vec<EditGroup>,
    pub(crate) pending: Option<EditGroup>,
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
        }
    }

    /// Start collecting edits for an insert mode session.
    pub fn start_group(&mut self, cursor_before: usize) {
        self.pending = Some(EditGroup {
            edits: Vec::new(),
            cursor_before,
        });
    }

    /// Finish the current group and push it onto the undo stack.
    pub fn finish_group(&mut self) {
        if let Some(group) = self.pending.take() {
            if !group.edits.is_empty() {
                self.undo.push(group);
                self.redo.clear();
            }
        }
    }

    /// Push a single-edit group immediately (for normal mode commands like dd, x).
    pub fn push_edit(&mut self, edit: EditKind, cursor_before: usize) {
        self.undo.push(EditGroup {
            edits: vec![edit],
            cursor_before,
        });
        self.redo.clear();
    }

    /// Record an insert into the pending group, coalescing consecutive chars.
    pub fn record_insert(&mut self, pos: usize, ch: char) {
        if let Some(group) = &mut self.pending {
            if let Some(EditKind::Insert { pos: last_pos, text }) = group.edits.last_mut() {
                if *last_pos + text.len() == pos {
                    text.push(ch);
                    return;
                }
            }
            group.edits.push(EditKind::Insert {
                pos,
                text: ch.to_string(),
            });
        }
    }

    /// Record an insert of a string into the pending group.
    pub fn record_insert_str(&mut self, pos: usize, s: &str) {
        if let Some(group) = &mut self.pending {
            group.edits.push(EditKind::Insert {
                pos,
                text: s.to_string(),
            });
        }
    }

    /// Record a delete into the pending group (for backspace/delete in insert mode).
    pub fn record_delete(&mut self, pos: usize, text: &str) {
        if let Some(group) = &mut self.pending {
            group.edits.push(EditKind::Delete {
                pos,
                text: text.to_string(),
            });
        }
    }
}
