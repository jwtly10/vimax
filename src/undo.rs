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

    pub fn start_group(&mut self, cursor_before: usize) {
        self.pending = Some(EditGroup {
            edits: Vec::new(),
            cursor_before,
        });
    }

    pub fn finish_group(&mut self) {
        if let Some(group) = self.pending.take()
            && !group.edits.is_empty()
        {
            self.undo.push(group);
            self.redo.clear();
        }
    }

    pub fn push_edit(&mut self, edit: EditKind, cursor_before: usize) {
        self.undo.push(EditGroup {
            edits: vec![edit],
            cursor_before,
        });
        self.redo.clear();
    }

    pub fn push_edits(&mut self, edits: Vec<EditKind>, cursor_before: usize) {
        self.undo.push(EditGroup {
            edits,
            cursor_before,
        });
        self.redo.clear();
    }

    /// Coalesces consecutive char inserts at adjacent positions into one edit.
    pub fn record_insert(&mut self, pos: usize, ch: char) {
        if let Some(group) = &mut self.pending {
            if let Some(EditKind::Insert { pos: last_pos, text }) = group.edits.last_mut()
                && *last_pos + text.len() == pos
            {
                text.push(ch);
                return;
            }
            group.edits.push(EditKind::Insert {
                pos,
                text: ch.to_string(),
            });
        }
    }

    pub fn record_insert_str(&mut self, pos: usize, s: &str) {
        if let Some(group) = &mut self.pending {
            group.edits.push(EditKind::Insert {
                pos,
                text: s.to_string(),
            });
        }
    }

    pub fn record_delete(&mut self, pos: usize, text: &str) {
        if let Some(group) = &mut self.pending {
            group.edits.push(EditKind::Delete {
                pos,
                text: text.to_string(),
            });
        }
    }
}
