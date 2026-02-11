use iced::keyboard;
use tracing::debug;

use crate::command::{CommandCtx, CommandId};
use crate::keymap::{KeyId, KeyPress, Keymap, KeymapLookup};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
    Command,
    Visual,
    VisualLine,
}

impl std::fmt::Display for VimMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VimMode::Normal => write!(f, "NORMAL"),
            VimMode::Insert => write!(f, "INSERT"),
            VimMode::Command => write!(f, "COMMAND"),
            VimMode::Visual => write!(f, "VISUAL"),
            VimMode::VisualLine => write!(f, "V-LINE"),
        }
    }
}

pub enum VimAction {
    Dispatch {
        command: CommandId,
        ctx: CommandCtx,
    },
    OperatorMotion {
        operator: &'static str,
        range: OperatorRange,
        count: usize,
    },
    VisualTextObject(TextObject),
    InsertText(String),
    ExecuteCommandLine(String),
    CommandLineUpdated,
    Pending,
    Unhandled,
}

pub enum OperatorRange {
    Motion { command: CommandId },
    TextObject(TextObject),
    WholeLine,
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

/// Checks if current key is just a modifier with no other inputs
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

/// Parse a text object from modifier ('i'/'a') and specifier char.
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

pub struct InputState {
    pub mode: VimMode,
    pending_keys: Vec<KeyPress>,
    count_accum: Option<usize>,
    pub command_line: String,
    pub command_display: String,
    pub selection_anchor: Option<usize>,
    pub pending_operator: Option<&'static str>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            mode: VimMode::Normal,
            pending_keys: Vec::new(),
            count_accum: None,
            command_line: String::new(),
            command_display: String::new(),
            selection_anchor: None,
            pending_operator: None,
        }
    }

    pub fn handle_key_event(
        &mut self,
        key: &keyboard::Key,
        modified_key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        text: Option<&str>,
        global_keymap: &Keymap,
        mode_keymap: &Keymap,
    ) -> VimAction {
        // Global keymaps take precidence
        if (modifiers.control() || modifiers.command() || modifiers.alt())
            && let Some(kp) = KeyPress::from_iced(modified_key, modifiers)
            && let KeymapLookup::Match(cmd) = global_keymap.lookup(&[kp])
        {
            return VimAction::Dispatch {
                command: cmd,
                ctx: CommandCtx { count: 1 },
            };
        }

        match self.mode {
            VimMode::Normal => self.handle_normal(modified_key, mode_keymap),
            VimMode::Visual | VimMode::VisualLine => self.handle_visual(modified_key, mode_keymap),
            VimMode::Insert => self.handle_insert(key, modifiers, mode_keymap, text),
            VimMode::Command => self.handle_command(key, text),
        }
    }

    fn handle_normal(&mut self, key: &keyboard::Key, keymap: &Keymap) -> VimAction {
        // Ignoring bare modifiers... since I never press both keys at the exact same time
        if is_bare_modifier(key) {
            return VimAction::Pending;
        }

        // If operator is pending, handle the motion/text-object/doubled
        if let Some(operator) = self.pending_operator {
            return self.handle_operator_pending(key, keymap, operator);
        }

        // 0 is not a valid motion by itself, but 10 is, so only handle 0 if its not the first count char
        if let keyboard::Key::Character(c) = key {
            let s = c.as_str();
            if s.len() == 1 {
                let ch = s.chars().next().unwrap();
                if ch.is_ascii_digit()
                    && (ch != '0' || self.count_accum.is_some())
                {
                    let current = self.count_accum.unwrap_or(0);
                    self.count_accum = Some(current * 10 + (ch as usize - '0' as usize));
                    return VimAction::Pending;
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

                    // If this command is an operator, set pending and wait for motion
                    if cmd == "op.delete" || cmd == "op.change" || cmd == "op.yank" {
                        self.pending_operator = Some(cmd);
                        return VimAction::Pending;
                    }

                    VimAction::Dispatch {
                        command: cmd,
                        ctx: CommandCtx { count },
                    }
                }
                KeymapLookup::Pending => VimAction::Pending,
                KeymapLookup::NoMatch => {
                    debug!(keys = ?self.pending_keys, "unhandled normal mode key sequence");
                    self.pending_keys.clear();
                    self.count_accum = None;
                    VimAction::Unhandled
                }
            }
        } else {
            self.pending_keys.clear();
            self.count_accum = None;
            VimAction::Unhandled
        }
    }

    fn handle_operator_pending(
        &mut self,
        key: &keyboard::Key,
        keymap: &Keymap,
        operator: &'static str,
    ) -> VimAction {
        // Check for operator-doubled (dd, cc, yy)
        if let keyboard::Key::Character(c) = key {
            let ch = c.as_str();
            let is_doubled = match operator {
                "op.delete" => ch == "d",
                "op.change" => ch == "c",
                "op.yank" => ch == "y",
                _ => false,
            };
            if is_doubled {
                let count = self.count_accum.unwrap_or(1);
                self.pending_operator = None;
                self.pending_keys.clear();
                self.count_accum = None;
                return VimAction::OperatorMotion {
                    operator,
                    range: OperatorRange::WholeLine,
                    count,
                };
            }
        }

        // Check for text objects: i + w/a + w
        if let Some(kp) = KeyPress::from_iced(key, &keyboard::Modifiers::default()) {
            self.pending_keys.push(kp);

            // Check for text object sequences: "iw", "aw"
            if self.pending_keys.len() == 1 && matches!(kp.key, KeyId::Char('i') | KeyId::Char('a'))
            {
                return VimAction::Pending;
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
                        return VimAction::OperatorMotion {
                            operator,
                            range: OperatorRange::TextObject(obj),
                            count,
                        };
                    }
                    // Not a valid text object — fall through to motion lookup below
                    // But first, retry without the 'i'/'a' prefix since it's not a text object
                    self.pending_keys.clear();
                    self.pending_keys.push(second);
                }
            }

            // Try to resolve as a motion via normal keymap
            match keymap.lookup(&self.pending_keys) {
                KeymapLookup::Match(cmd) => {
                    let count = self.count_accum.unwrap_or(1);
                    self.pending_keys.clear();
                    self.count_accum = None;

                    // If the resolved command is an operator itself, it's not valid as a motion
                    if cmd == "op.delete" || cmd == "op.change" || cmd == "op.yank" {
                        self.pending_operator = None;
                        return VimAction::Unhandled;
                    }

                    let op = self.pending_operator.take().unwrap();
                    VimAction::OperatorMotion {
                        operator: op,
                        range: OperatorRange::Motion { command: cmd },
                        count,
                    }
                }
                KeymapLookup::Pending => VimAction::Pending,
                KeymapLookup::NoMatch => {
                    debug!(keys = ?self.pending_keys, "unhandled operator-pending key sequence");
                    self.pending_keys.clear();
                    self.count_accum = None;
                    self.pending_operator = None;
                    VimAction::Unhandled
                }
            }
        } else {
            self.pending_keys.clear();
            self.count_accum = None;
            self.pending_operator = None;
            VimAction::Unhandled
        }
    }

    fn handle_insert(
        &mut self,
        key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        keymap: &Keymap,
        text: Option<&str>,
    ) -> VimAction {
        // Check named keys/modifier keys shortcuts first
        if let Some(kp) = KeyPress::from_iced(key, modifiers)
            && matches!(kp.key, crate::keymap::KeyId::Named(_))
        {
            match keymap.lookup(&[kp]) {
                KeymapLookup::Match(cmd) => {
                    return VimAction::Dispatch {
                        command: cmd,
                        ctx: CommandCtx { count: 1 },
                    };
                }
                _ => {
                    // Space falls through to OS text handling - technically a 'named' key
                    if !matches!(key, keyboard::Key::Named(keyboard::key::Named::Space)) {
                        return VimAction::Unhandled;
                    }
                }
            }
        }

        // Otherwise fall through to OS input handling
        if let Some(t) = text {
            let filtered: String = t.chars().filter(|ch| !ch.is_control()).collect();
            if !filtered.is_empty() {
                return VimAction::InsertText(filtered);
            }
        }

        VimAction::Unhandled
    }

    fn handle_visual(&mut self, key: &keyboard::Key, keymap: &Keymap) -> VimAction {
        // Ignore bare modifier keys (Shift, Ctrl, etc.) — they aren't real input
        if is_bare_modifier(key) {
            return VimAction::Pending;
        }

        // Check for text object sequences: i/a + specifier
        if let Some(kp) = KeyPress::from_iced(key, &keyboard::Modifiers::default()) {
            // If we have a pending 'i' or 'a' from a previous key, try to complete the text object
            if self.pending_keys.len() == 1 {
                let first = self.pending_keys[0];
                if let (KeyId::Char(modifier), KeyId::Char(obj)) = (first.key, kp.key) {
                    if let Some(text_obj) = parse_text_object(modifier, obj) {
                        self.pending_keys.clear();
                        self.count_accum = None;
                        return VimAction::VisualTextObject(text_obj);
                    }
                    // Not a valid text object — retry second key as a motion
                    self.pending_keys.clear();
                    self.pending_keys.push(kp);
                    // Fall through to keymap lookup
                } else {
                    // Second key is not a char — clear pending and try as motion
                    self.pending_keys.clear();
                    self.pending_keys.push(kp);
                }
            } else {
                // No pending text object prefix — check if this starts one
                if matches!(kp.key, KeyId::Char('i') | KeyId::Char('a')) {
                    self.pending_keys.push(kp);
                    return VimAction::Pending;
                }
                // Not a text object starter — try count accumulation then keymap
                if let KeyId::Char(ch) = kp.key
                    && ch.is_ascii_digit()
                    && (ch != '0' || self.count_accum.is_some())
                {
                    let current = self.count_accum.unwrap_or(0);
                    self.count_accum = Some(current * 10 + (ch as usize - '0' as usize));
                    return VimAction::Pending;
                }
                self.pending_keys.push(kp);
            }

            match keymap.lookup(&self.pending_keys) {
                KeymapLookup::Match(cmd) => {
                    let count = self.count_accum.unwrap_or(1);
                    self.pending_keys.clear();
                    self.count_accum = None;
                    VimAction::Dispatch {
                        command: cmd,
                        ctx: CommandCtx { count },
                    }
                }
                KeymapLookup::Pending => VimAction::Pending,
                KeymapLookup::NoMatch => {
                    debug!(keys = ?self.pending_keys, "unhandled visual mode key sequence");
                    self.pending_keys.clear();
                    self.count_accum = None;
                    VimAction::Unhandled
                }
            }
        } else {
            self.pending_keys.clear();
            self.count_accum = None;
            VimAction::Unhandled
        }
    }

    fn handle_command(&mut self, key: &keyboard::Key, text: Option<&str>) -> VimAction {
        if let keyboard::Key::Named(named) = key {
            match named {
                keyboard::key::Named::Escape => {
                    self.command_line.clear();
                    self.mode = VimMode::Normal;
                    return VimAction::CommandLineUpdated;
                }
                keyboard::key::Named::Enter => {
                    let cmd = self.command_line.clone();
                    self.command_line.clear();
                    self.mode = VimMode::Normal;
                    return VimAction::ExecuteCommandLine(cmd);
                }
                keyboard::key::Named::Backspace => {
                    if self.command_line.is_empty() {
                        self.mode = VimMode::Normal;
                    } else {
                        self.command_line.pop();
                    }
                    return VimAction::CommandLineUpdated;
                }
                keyboard::key::Named::Space => {
                    // fall through to OS text handling
                }
                _ => return VimAction::Unhandled,
            }
        }

        if let Some(t) = text {
            for ch in t.chars() {
                if !ch.is_control() {
                    self.command_line.push(ch);
                }
            }
        }
        VimAction::CommandLineUpdated
    }
}
