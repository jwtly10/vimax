use iced::keyboard;
use tracing::{debug, info};

use crate::action::{EditorAction, Motion, Range};
use crate::buffer::Buffer;

use super::commands;
use super::keymap::{Command, KeyId, KeyPress, Keymap, KeymapLookup};
use super::mode::VimMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
}

#[derive(Debug, Clone, Copy)]
pub enum TextObject {
    InnerWord,
    AWord,
    InnerParen,
    AParen,
    InnerSingleQuote,
    ASingleQuote,
    InnerAngle,
    AAngle,
    InnerDoubleQuote,
    ADoubleQuote,
    InnerCurly,
    ACurly,
    InnerBracket,
    ABracket,
}

fn is_bare_modifier(key: &keyboard::Key) -> bool {
    matches!(
        key,
        keyboard::Key::Named(
            keyboard::key::Named::Shift
                | keyboard::key::Named::Control
                | keyboard::key::Named::Alt
                | keyboard::key::Named::Super
                | keyboard::key::Named::Meta
        )
    )
}

fn parse_text_object(modifier: char, obj: char) -> Option<TextObject> {
    match (modifier, obj) {
        ('i', 'w') => Some(TextObject::InnerWord),
        ('a', 'w') => Some(TextObject::AWord),
        ('i', '(') | ('i', ')') | ('i', 'b') => Some(TextObject::InnerParen),
        ('a', '(') | ('a', ')') | ('a', 'b') => Some(TextObject::AParen),
        ('i', '\'') => Some(TextObject::InnerSingleQuote),
        ('a', '\'') => Some(TextObject::ASingleQuote),
        ('i', '"') => Some(TextObject::InnerDoubleQuote),
        ('a', '"') => Some(TextObject::ADoubleQuote),
        ('i', '<') | ('i', '>') => Some(TextObject::InnerAngle),
        ('a', '<') | ('a', '>') => Some(TextObject::AAngle),
        ('i', '{') | ('i', '}') | ('i', 'B') => Some(TextObject::InnerCurly),
        ('a', '{') | ('a', '}') | ('a', 'B') => Some(TextObject::ACurly),
        ('i', '[') | ('i', ']') => Some(TextObject::InnerBracket),
        ('a', '[') | ('a', ']') => Some(TextObject::ABracket),
        _ => None,
    }
}

fn resolve_text_object(buffer: &Buffer, cursor: usize, obj: TextObject) -> (usize, usize) {
    match obj {
        TextObject::InnerWord => buffer.text_object_inner_word(cursor),
        TextObject::AWord => buffer.text_object_a_word(cursor),
        TextObject::InnerParen => buffer.text_object_delimited(cursor, '(', ')', false),
        TextObject::AParen => buffer.text_object_delimited(cursor, '(', ')', true),
        TextObject::InnerCurly => buffer.text_object_delimited(cursor, '{', '}', false),
        TextObject::ACurly => buffer.text_object_delimited(cursor, '{', '}', true),
        TextObject::InnerBracket => buffer.text_object_delimited(cursor, '[', ']', false),
        TextObject::ABracket => buffer.text_object_delimited(cursor, '[', ']', true),
        TextObject::InnerAngle => buffer.text_object_delimited(cursor, '<', '>', false),
        TextObject::AAngle => buffer.text_object_delimited(cursor, '<', '>', true),
        TextObject::InnerSingleQuote => buffer.text_object_quoted(cursor, '\'', false),
        TextObject::ASingleQuote => buffer.text_object_quoted(cursor, '\'', true),
        TextObject::InnerDoubleQuote => buffer.text_object_quoted(cursor, '"', false),
        TextObject::ADoubleQuote => buffer.text_object_quoted(cursor, '"', true),
    }
}

pub struct InputState {
    pub mode: VimMode,
    pub selection_anchor: Option<usize>,
    pub pending_keys: Vec<KeyPress>,
    pub pending_global_keys: Vec<KeyPress>,
    pub count_accum: Option<usize>,
    pub command_line: String,
    pub pending_operator: Option<Operator>,
    pub search_query: String,
    pub pending_replace: bool,
    pub pending_find_char: Option<(bool, bool)>,
    pub last_find_char: Option<(char, bool, bool)>,
    pub search_history: History,
    pub command_history: History,
}

pub struct History {
    entries: Vec<String>,
    index: Option<usize>,
    scratch: Option<String>, // latest command 'cache'
}

impl History {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: None,
            scratch: None,
        }
    }
    pub fn push(&mut self, entry: String) {
        self.entries.push(entry);
        self.index = None;
        self.scratch = None;
    }
    pub fn up(&mut self, current: &mut String) {
        if let Some(idx) = self.index {
            if idx > 0 {
                self.index = Some(idx - 1);
                *current = self.entries[idx - 1].clone();
            }
        } else if !self.entries.is_empty() {
            self.index = Some(self.entries.len() - 1);
            // Store the actual cmd so that we can restore it
            self.scratch = Some(current.clone());
            *current = self.entries.last().unwrap().clone();
        }
    }
    pub fn down(&mut self, current: &mut String) {
        if let Some(idx) = self.index {
            if idx + 1 < self.entries.len() {
                self.index = Some(idx + 1);
                *current = self.entries[idx + 1].clone();
            } else {
                self.index = None;
                if let Some(scratch) = self.scratch.take() {
                    *current = scratch;
                }
            }
        }
    }
    pub fn reset(&mut self) {
        self.index = None;
        self.scratch = None;
    }
}

impl InputState {
    pub fn new() -> Self {
        Self {
            mode: VimMode::Normal,
            selection_anchor: None,
            pending_keys: Vec::new(),
            pending_global_keys: Vec::new(),
            count_accum: None,
            command_line: String::new(),
            pending_operator: None,
            search_query: String::new(),
            pending_replace: false,
            pending_find_char: None,
            last_find_char: None,
            search_history: History::new(),
            command_history: History::new(),
        }
    }

    pub fn enter_insert(&mut self) -> Vec<EditorAction> {
        info!("entering insert mode");
        self.mode = VimMode::Insert;
        vec![
            EditorAction::StartEditGroup,
            EditorAction::ClearSearch,
            EditorAction::SetMode("INSERT".to_string()),
        ]
    }

    pub fn enter_insert_after(&mut self) -> Vec<EditorAction> {
        self.mode = VimMode::Insert;
        vec![
            EditorAction::StartEditGroup,
            EditorAction::MoveCursor {
                motion: Motion::Right,
                count: 1,
            },
            EditorAction::ClearSearch,
            EditorAction::SetMode("INSERT".to_string()),
        ]
    }

    pub fn enter_insert_line_end(&mut self) -> Vec<EditorAction> {
        self.mode = VimMode::Insert;
        vec![
            EditorAction::StartEditGroup,
            EditorAction::MoveCursor {
                motion: Motion::LineEnd,
                count: 1,
            },
            EditorAction::ClearSearch,
            EditorAction::SetMode("INSERT".to_string()),
        ]
    }

    pub fn enter_insert_line_start(&mut self) -> Vec<EditorAction> {
        self.mode = VimMode::Insert;
        vec![
            EditorAction::StartEditGroup,
            EditorAction::MoveCursor {
                motion: Motion::LineStart,
                count: 1,
            },
            EditorAction::ClearSearch,
            EditorAction::SetMode("INSERT".to_string()),
        ]
    }

    pub fn open_below(&mut self) -> Vec<EditorAction> {
        self.mode = VimMode::Insert;
        vec![
            EditorAction::StartEditGroup,
            EditorAction::MoveCursor {
                motion: Motion::LineEnd,
                count: 1,
            },
            EditorAction::InsertNewline,
            EditorAction::ClearSearch,
            EditorAction::SetMode("INSERT".to_string()),
        ]
    }

    pub fn open_above(&mut self) -> Vec<EditorAction> {
        self.mode = VimMode::Insert;
        vec![
            EditorAction::StartEditGroup,
            EditorAction::MoveCursor {
                motion: Motion::LineStart,
                count: 1,
            },
            EditorAction::InsertNewline,
            EditorAction::MoveCursor {
                motion: Motion::Up,
                count: 1,
            },
            EditorAction::ClearSearch,
            EditorAction::SetMode("INSERT".to_string()),
        ]
    }

    pub fn exit_insert(&mut self) -> Vec<EditorAction> {
        info!("exiting insert mode");
        self.mode = VimMode::Normal;
        vec![
            EditorAction::FinishEditGroup,
            EditorAction::SetMode("NORMAL".to_string()),
        ]
    }

    pub fn enter_command(&mut self) {
        self.command_line.clear();
        self.mode = VimMode::Command;
    }

    pub fn enter_search(&mut self) {
        self.search_query.clear();
        self.mode = VimMode::Search;
    }

    pub fn open_buffer_picker(&mut self) -> Vec<EditorAction> {
        self.mode = VimMode::Normal;
        vec![EditorAction::OpenBufferPicker]
    }

    pub fn open_project_files_picker(&mut self, show_ignored: bool) -> Vec<EditorAction> {
        self.mode = VimMode::Normal;
        vec![EditorAction::OpenFilePicker {
            show_ignored,
            max_results: 1000,
        }]
    }

    pub fn enter_visual(&mut self, cursor: usize) {
        info!("entering visual mode");
        self.selection_anchor = Some(cursor);
        self.mode = VimMode::Visual;
    }

    pub fn enter_visual_line(&mut self, cursor: usize) {
        info!("entering visual line mode");
        if self.mode == VimMode::Visual {
            self.mode = VimMode::VisualLine;
        } else if self.mode == VimMode::VisualLine {
            self.selection_anchor = None;
            self.mode = VimMode::Normal;
        } else {
            self.selection_anchor = Some(cursor);
            self.mode = VimMode::VisualLine;
        }
    }

    pub fn exit_visual(&mut self) -> Vec<EditorAction> {
        info!("exiting visual mode");
        self.selection_anchor = None;
        self.mode = VimMode::Normal;
        vec![
            EditorAction::SetSelection(None),
            EditorAction::SetMode("NORMAL".to_string()),
        ]
    }

    pub fn visual_delete(&mut self, buffer: &Buffer, cursor: usize) -> Vec<EditorAction> {
        let mut actions = Vec::new();
        if let Some((start, end)) = self.compute_selection(buffer, cursor) {
            actions.push(EditorAction::DeleteRange(Range { start, end }));
        }
        self.selection_anchor = None;
        self.mode = VimMode::Normal;
        actions.push(EditorAction::SetSelection(None));
        actions.push(EditorAction::SetMode("NORMAL".to_string()));
        actions
    }

    pub fn visual_yank(&mut self, buffer: &Buffer, cursor: usize) -> Vec<EditorAction> {
        let mut actions = Vec::new();
        if let Some((start, end)) = self.compute_selection(buffer, cursor) {
            actions.push(EditorAction::YankRange(Range { start, end }));
        }
        self.selection_anchor = None;
        self.mode = VimMode::Normal;
        actions.push(EditorAction::SetSelection(None));
        actions.push(EditorAction::SetMode("NORMAL".to_string()));
        actions
    }

    pub fn visual_change(&mut self, buffer: &Buffer, cursor: usize) -> Vec<EditorAction> {
        let mut actions = Vec::new();
        if let Some((start, end)) = self.compute_selection(buffer, cursor) {
            actions.push(EditorAction::ChangeRange(Range { start, end }));
        }
        self.selection_anchor = None;
        self.mode = VimMode::Insert;
        actions.push(EditorAction::SetSelection(None));
        actions.push(EditorAction::SetMode("INSERT".to_string()));
        actions
    }

    pub fn compute_selection(&self, buffer: &Buffer, cursor: usize) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;

        if self.mode == VimMode::VisualLine {
            let anchor_line = buffer.char_to_line(anchor);
            let cursor_line = buffer.char_to_line(cursor);
            let (start_line, end_line) = if anchor_line <= cursor_line {
                (anchor_line, cursor_line)
            } else {
                (cursor_line, anchor_line)
            };
            let start = buffer.line_to_char(start_line);
            let end = if end_line + 1 < buffer.total_lines() {
                buffer.line_to_char(end_line + 1)
            } else {
                buffer.len_chars()
            };
            Some((start, end))
        } else if anchor <= cursor {
            Some((anchor, cursor + 1))
        } else {
            Some((cursor, anchor + 1))
        }
    }

    pub fn mode_color(&self) -> (f32, f32, f32) {
        match self.mode {
            VimMode::Normal => (0.6, 0.8, 1.0),
            VimMode::Insert => (0.6, 1.0, 0.6),
            VimMode::Command | VimMode::Search => (1.0, 0.8, 0.5),
            VimMode::Visual | VimMode::VisualLine => (0.9, 0.6, 1.0),
        }
    }

    pub fn status_line_override(&self) -> Option<String> {
        match self.mode {
            VimMode::Command => Some(format!(":{}", self.command_line)),
            VimMode::Search => Some(format!("/{}", self.search_query)),
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_key(
        &mut self,
        key: &keyboard::Key,
        modified_key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        text: Option<&str>,
        global_keymap: &Keymap,
        mode_keymap: &Keymap,
        buffer: &Buffer,
        cursor: usize,
    ) -> Vec<EditorAction> {
        if is_bare_modifier(key) {
            return vec![];
        }

        if !self.pending_global_keys.is_empty() {
            if let Some(kp) = KeyPress::from_iced(modified_key, modifiers)
                .or_else(|| KeyPress::from_iced(key, modifiers))
            {
                self.pending_global_keys.push(kp);
                match global_keymap.lookup(&self.pending_global_keys) {
                    KeymapLookup::Match(cmd) => {
                        self.pending_global_keys.clear();
                        let count = self.count_accum.unwrap_or(1);
                        self.count_accum = None;
                        return commands::resolve(cmd, count, self, buffer, cursor);
                    }
                    KeymapLookup::Pending => return vec![],
                    KeymapLookup::NoMatch => {
                        self.pending_global_keys.clear();
                    }
                }
            } else {
                self.pending_global_keys.clear();
            }
        } else if (modifiers.control() || modifiers.command() || modifiers.alt())
            && let Some(kp) = KeyPress::from_iced(modified_key, modifiers)
        {
            match global_keymap.lookup(&[kp]) {
                KeymapLookup::Match(cmd) => {
                    let count = self.count_accum.unwrap_or(1);
                    self.count_accum = None;
                    return commands::resolve(cmd, count, self, buffer, cursor);
                }
                KeymapLookup::Pending => {
                    self.pending_global_keys.push(kp);
                    return vec![];
                }
                KeymapLookup::NoMatch => {}
            }
        }

        match self.mode {
            VimMode::Normal => self.handle_normal(modified_key, mode_keymap, buffer, cursor),
            VimMode::Visual | VimMode::VisualLine => {
                self.handle_visual(modified_key, mode_keymap, buffer, cursor)
            }
            VimMode::Insert => {
                self.handle_insert(key, modifiers, mode_keymap, buffer, cursor, text)
            }
            VimMode::Command => self.handle_command(key, text),
            VimMode::Search => self.handle_search(key, text),
        }
    }

    fn handle_normal(
        &mut self,
        key: &keyboard::Key,
        keymap: &Keymap,
        buffer: &Buffer,
        cursor: usize,
    ) -> Vec<EditorAction> {
        if is_bare_modifier(key) {
            return vec![];
        }

        if self.pending_replace {
            self.pending_replace = false;
            if let keyboard::Key::Character(c) = key
                && let Some(ch) = c.as_str().chars().next()
            {
                return vec![EditorAction::ReplaceChar(ch)];
            }
            return vec![];
        }

        if let Some((forward, stop_before)) = self.pending_find_char.take() {
            if let keyboard::Key::Character(c) = key
                && let Some(ch) = c.as_str().chars().next()
            {
                self.last_find_char = Some((ch, forward, stop_before));
                return vec![EditorAction::MoveCursor {
                    motion: Motion::FindChar {
                        ch,
                        forward,
                        stop_before,
                    },
                    count: self.count_accum.unwrap_or(1),
                }];
            }
            return vec![];
        }

        if self.pending_operator.is_some() {
            return self.handle_operator_pending(key, keymap, buffer, cursor);
        }

        if let keyboard::Key::Character(c) = key {
            let s = c.as_str();
            if s.len() == 1 {
                let ch = s.chars().next().unwrap();
                if ch.is_ascii_digit() && (ch != '0' || self.count_accum.is_some()) {
                    let current = self.count_accum.unwrap_or(0);
                    self.count_accum = Some(current * 10 + (ch as usize - '0' as usize));
                    return vec![];
                }
            }
        }

        if let Some(kp) = KeyPress::from_iced(key, &keyboard::Modifiers::default()) {
            self.pending_keys.push(kp);
            match keymap.lookup(&self.pending_keys) {
                KeymapLookup::Match(cmd) => {
                    let count = self.count_accum.unwrap_or(1);
                    self.pending_keys.clear();
                    self.count_accum = None;

                    if cmd.is_operator() {
                        self.pending_operator = Some(match cmd {
                            Command::OpDelete => Operator::Delete,
                            Command::OpChange => Operator::Change,
                            _ => Operator::Yank,
                        });
                        self.count_accum = Some(count);
                        return vec![];
                    }

                    commands::resolve(cmd, count, self, buffer, cursor)
                }
                KeymapLookup::Pending => vec![],
                KeymapLookup::NoMatch => {
                    debug!(keys = ?self.pending_keys, "unhandled normal mode key sequence");
                    self.pending_keys.clear();
                    self.count_accum = None;
                    vec![]
                }
            }
        } else {
            self.pending_keys.clear();
            self.count_accum = None;
            vec![]
        }
    }

    fn handle_operator_pending(
        &mut self,
        key: &keyboard::Key,
        keymap: &Keymap,
        buffer: &Buffer,
        cursor: usize,
    ) -> Vec<EditorAction> {
        let operator = self.pending_operator.unwrap();

        if let Some((forward, stop_before)) = self.pending_find_char.take() {
            if let keyboard::Key::Character(c) = key
                && let Some(ch) = c.as_str().chars().next()
            {
                self.last_find_char = Some((ch, forward, stop_before));
                let count = self.count_accum.unwrap_or(1);
                self.pending_operator = None;
                self.pending_keys.clear();
                self.count_accum = None;

                return self.resolve_operator_with_find_char(
                    operator,
                    ch,
                    forward,
                    stop_before,
                    count,
                    buffer,
                    cursor,
                );
            }
            self.pending_operator = None;
            self.pending_keys.clear();
            self.count_accum = None;
            return vec![];
        }

        if let keyboard::Key::Character(c) = key {
            let ch = c.as_str();
            let is_doubled = match operator {
                Operator::Delete => ch == "d",
                Operator::Change => ch == "c",
                Operator::Yank => ch == "y",
            };
            if is_doubled {
                let count = self.count_accum.unwrap_or(1);
                self.pending_operator = None;
                self.pending_keys.clear();
                self.count_accum = None;
                return self.resolve_operator_whole_line(operator, count, buffer, cursor);
            }
        }

        if let Some(kp) = KeyPress::from_iced(key, &keyboard::Modifiers::default()) {
            self.pending_keys.push(kp);

            if self.pending_keys.len() == 1 && matches!(kp.key, KeyId::Char('i') | KeyId::Char('a'))
            {
                return vec![];
            }

            if self.pending_keys.len() == 2 {
                let first = self.pending_keys[0];
                let second = self.pending_keys[1];
                if let (KeyId::Char(modifier), KeyId::Char(obj)) = (first.key, second.key) {
                    let text_obj = parse_text_object(modifier, obj);
                    if let Some(obj) = text_obj {
                        let count = self.count_accum.unwrap_or(1);
                        self.pending_operator = None;
                        self.pending_keys.clear();
                        self.count_accum = None;
                        return self
                            .resolve_operator_text_object(operator, obj, count, buffer, cursor);
                    }
                    self.pending_keys.clear();
                    self.pending_keys.push(second);
                }
            }

            match keymap.lookup(&self.pending_keys) {
                KeymapLookup::Match(cmd) => {
                    let count = self.count_accum.unwrap_or(1);
                    self.pending_keys.clear();
                    self.count_accum = None;

                    if cmd.is_operator() {
                        self.pending_operator = None;
                        return vec![];
                    }

                    if let Some((forward, stop_before)) = cmd.find_char_params() {
                        self.pending_find_char = Some((forward, stop_before));
                        return vec![];
                    }

                    let op = self.pending_operator.take().unwrap();
                    self.resolve_operator_motion(op, cmd, count, buffer, cursor)
                }
                KeymapLookup::Pending => vec![],
                KeymapLookup::NoMatch => {
                    debug!(
                        keys = ?self.pending_keys,
                        "unhandled operator-pending key sequence"
                    );
                    self.pending_keys.clear();
                    self.count_accum = None;
                    self.pending_operator = None;
                    vec![]
                }
            }
        } else {
            self.pending_keys.clear();
            self.count_accum = None;
            self.pending_operator = None;
            vec![]
        }
    }

    fn resolve_operator_motion(
        &mut self,
        operator: Operator,
        motion_cmd: Command,
        count: usize,
        buffer: &Buffer,
        cursor: usize,
    ) -> Vec<EditorAction> {
        let (motion, inclusive) = if let Some(pair) = motion_cmd.as_motion() {
            pair
        } else {
            match motion_cmd {
                Command::MotionRepeatFindChar => {
                    if let Some((ch, forward, stop_before)) = self.last_find_char {
                        (
                            Motion::FindChar {
                                ch,
                                forward,
                                stop_before,
                            },
                            true,
                        )
                    } else {
                        return vec![];
                    }
                }
                Command::MotionRepeatFindCharReverse => {
                    if let Some((ch, forward, stop_before)) = self.last_find_char {
                        (
                            Motion::FindChar {
                                ch,
                                forward: !forward,
                                stop_before,
                            },
                            true,
                        )
                    } else {
                        return vec![];
                    }
                }
                _ => return vec![],
            }
        };

        let after = buffer.cursor_after_motion(cursor, &motion, count);
        let (start, end) = if cursor <= after {
            (cursor, if inclusive { after + 1 } else { after })
        } else {
            (after, cursor + 1)
        };

        if start >= end {
            return vec![];
        }

        self.emit_operator(operator, start, end)
    }

    fn resolve_operator_whole_line(
        &mut self,
        operator: Operator,
        count: usize,
        buffer: &Buffer,
        cursor: usize,
    ) -> Vec<EditorAction> {
        let (line, _) = buffer.cursor_position(cursor);
        let line_start = buffer.line_to_char(line);
        let target_line = (line + count).min(buffer.total_lines());
        let line_end = if target_line < buffer.total_lines() {
            buffer.line_to_char(target_line)
        } else {
            buffer.len_chars()
        };

        if line_start == line_end {
            return vec![];
        }

        self.emit_operator(operator, line_start, line_end)
    }

    fn resolve_operator_text_object(
        &mut self,
        operator: Operator,
        obj: TextObject,
        _count: usize,
        buffer: &Buffer,
        cursor: usize,
    ) -> Vec<EditorAction> {
        let (start, end) = resolve_text_object(buffer, cursor, obj);
        if start == end {
            return vec![];
        }
        self.emit_operator(operator, start, end)
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_operator_with_find_char(
        &mut self,
        operator: Operator,
        ch: char,
        forward: bool,
        stop_before: bool,
        count: usize,
        buffer: &Buffer,
        cursor: usize,
    ) -> Vec<EditorAction> {
        let motion = Motion::FindChar {
            ch,
            forward,
            stop_before,
        };
        let after = buffer.cursor_after_motion(cursor, &motion, count);
        let (start, end) = if cursor <= after {
            (cursor, after + 1)
        } else {
            (after, cursor)
        };

        if start == end {
            return vec![];
        }

        self.emit_operator(operator, start, end)
    }

    fn emit_operator(&mut self, operator: Operator, start: usize, end: usize) -> Vec<EditorAction> {
        let range = Range { start, end };
        match operator {
            Operator::Delete => vec![EditorAction::DeleteRange(range)],
            Operator::Change => {
                self.mode = VimMode::Insert;
                vec![
                    EditorAction::ChangeRange(range),
                    EditorAction::SetMode("INSERT".to_string()),
                ]
            }
            Operator::Yank => vec![EditorAction::YankRange(range)],
        }
    }

    fn handle_insert(
        &mut self,
        key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        keymap: &Keymap,
        buffer: &Buffer,
        cursor: usize,
        text: Option<&str>,
    ) -> Vec<EditorAction> {
        if let Some(kp) = KeyPress::from_iced(key, modifiers)
            && matches!(kp.key, KeyId::Named(_))
        {
            match keymap.lookup(&[kp]) {
                KeymapLookup::Match(cmd) => {
                    return commands::resolve(cmd, 1, self, buffer, cursor);
                }
                _ => {
                    if !matches!(key, keyboard::Key::Named(keyboard::key::Named::Space)) {
                        return vec![];
                    }
                }
            }
        }

        if let Some(t) = text {
            let mut actions = Vec::new();
            for ch in t.chars() {
                if !ch.is_control() {
                    actions.push(EditorAction::InsertChar(ch));
                }
            }
            if !actions.is_empty() {
                return actions;
            }
        }

        vec![]
    }

    fn handle_visual(
        &mut self,
        key: &keyboard::Key,
        keymap: &Keymap,
        buffer: &Buffer,
        cursor: usize,
    ) -> Vec<EditorAction> {
        if is_bare_modifier(key) {
            return vec![];
        }

        if let Some(kp) = KeyPress::from_iced(key, &keyboard::Modifiers::default()) {
            if self.pending_keys.len() == 1 {
                let first = self.pending_keys[0];
                if let (KeyId::Char(modifier), KeyId::Char(obj)) = (first.key, kp.key) {
                    if let Some(text_obj) = parse_text_object(modifier, obj) {
                        self.pending_keys.clear();
                        self.count_accum = None;
                        let (start, end) = resolve_text_object(buffer, cursor, text_obj);
                        if start != end {
                            self.selection_anchor = Some(start);
                            return vec![
                                EditorAction::SetCursor(end.saturating_sub(1)),
                                EditorAction::SetSelection(Some((start, end))),
                            ];
                        }
                        return vec![];
                    }
                    self.pending_keys.clear();
                    self.pending_keys.push(kp);
                } else {
                    self.pending_keys.clear();
                    self.pending_keys.push(kp);
                }
            } else {
                if matches!(kp.key, KeyId::Char('i') | KeyId::Char('a')) {
                    self.pending_keys.push(kp);
                    return vec![];
                }
                if let KeyId::Char(ch) = kp.key
                    && ch.is_ascii_digit()
                    && (ch != '0' || self.count_accum.is_some())
                {
                    let current = self.count_accum.unwrap_or(0);
                    self.count_accum = Some(current * 10 + (ch as usize - '0' as usize));
                    return vec![];
                }
                self.pending_keys.push(kp);
            }

            match keymap.lookup(&self.pending_keys) {
                KeymapLookup::Match(cmd) => {
                    let count = self.count_accum.unwrap_or(1);
                    self.pending_keys.clear();
                    self.count_accum = None;
                    commands::resolve(cmd, count, self, buffer, cursor)
                }
                KeymapLookup::Pending => vec![],
                KeymapLookup::NoMatch => {
                    debug!(
                        keys = ?self.pending_keys,
                        "unhandled visual mode key sequence"
                    );
                    self.pending_keys.clear();
                    self.count_accum = None;
                    vec![]
                }
            }
        } else {
            self.pending_keys.clear();
            self.count_accum = None;
            vec![]
        }
    }

    fn handle_search(&mut self, key: &keyboard::Key, text: Option<&str>) -> Vec<EditorAction> {
        if let keyboard::Key::Named(named) = key {
            match named {
                keyboard::key::Named::Escape => {
                    self.search_query.clear();
                    self.mode = VimMode::Normal;
                    self.search_history.reset();
                    return vec![EditorAction::SetMode("NORMAL".to_string())];
                }
                keyboard::key::Named::Enter => {
                    let query = self.search_query.clone();
                    self.search_query.clear();
                    self.mode = VimMode::Normal;
                    if query.is_empty() {
                        return vec![EditorAction::SetMode("NORMAL".to_string())];
                    }
                    debug!(?query, "init vim search");
                    self.search_history.push(query.clone());
                    return vec![
                        EditorAction::SetSearchPattern(query),
                        EditorAction::SearchNext { count: 1 },
                        EditorAction::SetMode("NORMAL".to_string()),
                    ];
                }
                keyboard::key::Named::ArrowUp => {
                    self.search_history.up(&mut self.search_query);
                    return vec![];
                }
                keyboard::key::Named::ArrowDown => {
                    self.search_history.down(&mut self.search_query);
                    return vec![];
                }
                keyboard::key::Named::Backspace => {
                    if self.search_query.is_empty() {
                        self.mode = VimMode::Normal;
                        return vec![EditorAction::SetMode("NORMAL".to_string())];
                    } else {
                        self.search_query.pop();
                    }
                    return vec![];
                }
                keyboard::key::Named::Space => {}
                _ => return vec![],
            }
        }

        if let Some(t) = text {
            for ch in t.chars() {
                if !ch.is_control() {
                    self.search_query.push(ch);
                }
            }
        }
        vec![]
    }

    fn handle_command(&mut self, key: &keyboard::Key, text: Option<&str>) -> Vec<EditorAction> {
        if let keyboard::Key::Named(named) = key {
            match named {
                keyboard::key::Named::Escape => {
                    self.command_line.clear();
                    self.mode = VimMode::Normal;
                    self.command_history.reset();
                    return vec![EditorAction::SetMode("NORMAL".to_string())];
                }
                keyboard::key::Named::Enter => {
                    let cmd = self.command_line.clone();
                    self.command_line.clear();
                    self.mode = VimMode::Normal;
                    let mut actions = vec![EditorAction::SetMode("NORMAL".to_string())];
                    actions.extend(Self::resolve_ex_command(&cmd));
                    self.command_history.push(cmd);
                    return actions;
                }
                keyboard::key::Named::ArrowUp => {
                    self.command_history.up(&mut self.command_line);
                    return vec![];
                }
                keyboard::key::Named::ArrowDown => {
                    self.command_history.down(&mut self.command_line);
                    return vec![];
                }
                keyboard::key::Named::Backspace => {
                    if self.command_line.is_empty() {
                        self.mode = VimMode::Normal;
                        return vec![EditorAction::SetMode("NORMAL".to_string())];
                    } else {
                        self.command_line.pop();
                    }
                    return vec![];
                }
                keyboard::key::Named::Space => {}
                _ => return vec![],
            }
        }

        if let Some(t) = text {
            for ch in t.chars() {
                if !ch.is_control() {
                    self.command_line.push(ch);
                }
            }
        }
        vec![]
    }

    fn resolve_ex_command(cmd: &str) -> Vec<EditorAction> {
        let trimmed = cmd.trim();
        match trimmed {
            "w" => vec![EditorAction::Save],
            "q" => vec![EditorAction::CloseWindow],
            "q!" => vec![EditorAction::Quit { force: true }],
            "qa!" => vec![EditorAction::ForceQuitApp],
            "wq" => vec![EditorAction::WriteQuit],
            "bn" | "bnext" => vec![EditorAction::NextBuffer],
            "bp" | "bprev" | "bprevious" => vec![EditorAction::PrevBuffer],
            "bd" | "bdelete" => vec![EditorAction::CloseBuffer],
            "buffers" | "ls" => vec![EditorAction::OpenBufferPicker],
            "diagnostics" => vec![EditorAction::OpenDiagnosticsPicker],
            "vs" | "vsplit" => vec![EditorAction::VSplit],
            "sp" | "split" => vec![EditorAction::HSplit],
            "close" => vec![EditorAction::CloseWindow],
            _ if trimmed.starts_with("e ") || trimmed.starts_with("edit ") => {
                let path = trimmed
                    .strip_prefix("e ")
                    .or_else(|| trimmed.strip_prefix("edit "))
                    .unwrap()
                    .trim();
                if path.is_empty() {
                    vec![EditorAction::SetStatusMessage(
                        "Usage: :e <path>".to_string(),
                    )]
                } else {
                    vec![EditorAction::OpenFile(std::path::PathBuf::from(path))]
                }
            }
            other => vec![EditorAction::SetStatusMessage(format!(
                "Unknown command: {}",
                other
            ))],
        }
    }
}
