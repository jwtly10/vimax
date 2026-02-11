use std::collections::HashMap;

use iced::keyboard;
use tracing::debug;

use crate::command::CommandId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyId {
    Char(char),
    Named(keyboard::key::Named),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub key: KeyId,
    pub ctrl: bool,
    pub alt: bool,
    pub cmd: bool,
}

impl KeyPress {
    pub fn char(c: char) -> Self {
        Self {
            key: KeyId::Char(c),
            ctrl: false,
            alt: false,
            cmd: false,
        }
    }

    pub fn named(n: keyboard::key::Named) -> Self {
        Self {
            key: KeyId::Named(n),
            ctrl: false,
            alt: false,
            cmd: false,
        }
    }

    // TODO: not sure yet how these play nice with linux
    pub fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn cmd(mut self) -> Self {
        self.cmd = true;
        self
    }

    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// Convert an iced key event into a KeyPress for keymap lookup.
    /// `use_modified` controls whether we use modified_key (normal mode)
    /// or key (insert mode for named keys).
    pub fn from_iced(key: &keyboard::Key, modifiers: &keyboard::Modifiers) -> Option<Self> {
        let key_id = match key {
            keyboard::Key::Character(c) => {
                let s = c.as_str();
                let mut chars = s.chars();
                let ch = chars.next()?;
                if chars.next().is_some() {
                    return None; // multi-char sequences not supported as single key
                }
                KeyId::Char(ch)
            }
            keyboard::Key::Named(n) => KeyId::Named(*n),
            _ => return None,
        };

        Some(Self {
            key: key_id,
            ctrl: modifiers.control(),
            alt: modifiers.alt(),
            cmd: modifiers.command(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapLookup {
    Match(CommandId),
    Pending,
    NoMatch,
}

pub struct Keymap {
    bindings: HashMap<Vec<KeyPress>, CommandId>,
    prefixes: HashMap<Vec<KeyPress>, ()>,
}

impl Keymap {
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            prefixes: HashMap::new(),
        }
    }

    pub fn bind(&mut self, keys: Vec<KeyPress>, command: CommandId) {
        // Register all proper prefixes
        for len in 1..keys.len() {
            self.prefixes.insert(keys[..len].to_vec(), ());
        }
        self.bindings.insert(keys, command);
    }

    pub fn lookup(&self, keys: &[KeyPress]) -> KeymapLookup {
        debug!(?keys, "key command lookup");
        if let Some(&cmd) = self.bindings.get(keys) {
            debug!(?cmd, "key command found");
            KeymapLookup::Match(cmd)
        } else if self.prefixes.contains_key(keys) {
            debug!("starting key sequence, waiting for more input");
            KeymapLookup::Pending
        } else {
            debug!("no matching key sequence");
            KeymapLookup::NoMatch
        }
    }

    /// Copy all bindings from another keymap into this one.
    pub fn extend_from(&mut self, other: &Keymap) {
        for (keys, &cmd) in &other.bindings {
            self.bind(keys.clone(), cmd);
        }
    }

    /// Remove a binding by its key sequence.
    pub fn remove(&mut self, keys: &[KeyPress]) {
        self.bindings.remove(keys);
        // Note: we don't clean up prefixes since they might be shared
        // with other bindings. Stale prefixes just cause a Pending that
        // resolves to NoMatch on the next key, which is harmless.
    }
}

pub struct Keymaps {
    global: Keymap,
    normal: Keymap,
    insert: Keymap,
    visual: Keymap,
}

impl Keymaps {
    pub fn new() -> Self {
        let normal = build_normal_keymap();
        let visual = build_visual_keymap(&normal);
        Self {
            global: build_global_keymap(),
            normal,
            insert: build_insert_keymap(),
            visual,
        }
    }

    pub fn global(&self) -> &Keymap {
        &self.global
    }

    pub fn for_mode(&self, mode: crate::input::VimMode) -> &Keymap {
        use crate::input::VimMode;
        match mode {
            VimMode::Normal => &self.normal,
            VimMode::Insert => &self.insert,
            VimMode::Visual | VimMode::VisualLine => &self.visual,
            VimMode::Command | VimMode::Search => &self.insert, // command/search mode doesn't use keymap
        }
    }
}

// -- Keymap builders --

pub fn build_global_keymap() -> Keymap {
    let mut km = Keymap::new();
    km.bind(vec![KeyPress::char('r').ctrl()], "edit.redo");
    km.bind(vec![KeyPress::char('d').ctrl()], "scroll.half_down");
    km.bind(vec![KeyPress::char('u').ctrl()], "scroll.half_up");
    km.bind(vec![KeyPress::char('s').cmd()], "buffer.save");
    km
}

pub fn build_normal_keymap() -> Keymap {
    let mut km = Keymap::new();

    // Cursor movement
    km.bind(vec![KeyPress::char('h')], "cursor.move_left");
    km.bind(vec![KeyPress::char('j')], "cursor.move_down");
    km.bind(vec![KeyPress::char('k')], "cursor.move_up");
    km.bind(vec![KeyPress::char('l')], "cursor.move_right");
    km.bind(vec![KeyPress::char('w')], "cursor.move_word_forward");
    km.bind(vec![KeyPress::char('b')], "cursor.move_word_backward");
    km.bind(vec![KeyPress::char('0')], "cursor.move_line_start");
    km.bind(vec![KeyPress::char('$')], "cursor.move_line_end");
    km.bind(vec![KeyPress::char('e')], "cursor.move_line_end");
    km.bind(
        vec![KeyPress::char('^')],
        "cursor.move_first_non_whitespace",
    );
    km.bind(vec![KeyPress::char('G')], "cursor.move_to_end");

    // Arrow keys
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowLeft)],
        "cursor.move_left",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowRight)],
        "cursor.move_right",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowUp)],
        "cursor.move_up",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowDown)],
        "cursor.move_down",
    );

    // Editing
    km.bind(vec![KeyPress::char('x')], "edit.delete_char_forward");
    km.bind(vec![KeyPress::char('u')], "edit.undo");

    // Operators
    km.bind(vec![KeyPress::char('d')], "op.delete");
    km.bind(vec![KeyPress::char('c')], "op.change");
    km.bind(vec![KeyPress::char('y')], "op.yank");

    // Chords
    km.bind(
        vec![KeyPress::char('g'), KeyPress::char('g')],
        "cursor.move_to_start",
    );

    // Paste
    km.bind(vec![KeyPress::char('p')], "edit.paste_after");
    km.bind(vec![KeyPress::char('P')], "edit.paste_before");

    // Vim mode transitions
    km.bind(vec![KeyPress::char('i')], "vim.enter_insert");
    km.bind(vec![KeyPress::char('a')], "vim.enter_insert_after");
    km.bind(vec![KeyPress::char('A')], "vim.enter_insert_line_end");
    km.bind(vec![KeyPress::char('I')], "vim.enter_insert_line_start");
    km.bind(vec![KeyPress::char('o')], "vim.open_below");
    km.bind(vec![KeyPress::char('O')], "vim.open_above");
    km.bind(vec![KeyPress::char('v')], "vim.enter_visual");
    km.bind(vec![KeyPress::char('V')], "vim.enter_visual_line");
    km.bind(vec![KeyPress::char(':')], "vim.enter_command");

    km.bind(
        vec![KeyPress::named(keyboard::key::Named::Escape)],
        "vim.clear_search",
    );
    km.bind(vec![KeyPress::char('/')], "vim.enter_search");
    km.bind(vec![KeyPress::char('n')], "search.next");
    km.bind(vec![KeyPress::char('N')], "search.prev");
    km.bind(vec![KeyPress::char('r')], "edit.replace_char");
    km.bind(vec![KeyPress::char('f')], "motion.find_char_forward");
    km.bind(vec![KeyPress::char('F')], "motion.find_char_backward");
    km.bind(vec![KeyPress::char('t')], "motion.find_char_forward_before");
    km.bind(vec![KeyPress::char('T')], "motion.find_char_backward_before");
    km.bind(vec![KeyPress::char(';')], "motion.repeat_find_char");
    km.bind(vec![KeyPress::char(',')], "motion.repeat_find_char_reverse");

    km
}

fn build_visual_keymap(normal: &Keymap) -> Keymap {
    let mut km = Keymap::new();

    km.extend_from(normal);

    // Remove non supported keys
    km.remove(&[KeyPress::char('i')]);
    km.remove(&[KeyPress::char('a')]);
    km.remove(&[KeyPress::char('o')]);
    km.remove(&[KeyPress::char('O')]);
    km.remove(&[KeyPress::char('A')]);
    km.remove(&[KeyPress::char('I')]);
    km.remove(&[KeyPress::char(':')]);
    km.remove(&[KeyPress::char('u')]);
    km.remove(&[KeyPress::char('r')]);
    km.remove(&[KeyPress::char('/')]);
    km.remove(&[KeyPress::char('n')]);
    km.remove(&[KeyPress::char('N')]);

    km.bind(vec![KeyPress::char('d')], "visual.delete");
    km.bind(vec![KeyPress::char('x')], "visual.delete");
    km.bind(vec![KeyPress::char('y')], "visual.yank");
    km.bind(vec![KeyPress::char('c')], "visual.change");

    km.bind(
        vec![KeyPress::named(keyboard::key::Named::Escape)],
        "vim.exit_visual",
    );
    km.bind(vec![KeyPress::char('v')], "vim.exit_visual");

    // V toggles to linewise visual
    km.bind(vec![KeyPress::char('V')], "vim.enter_visual_line");

    km
}

pub fn build_insert_keymap() -> Keymap {
    let mut km = Keymap::new();

    km.bind(
        vec![KeyPress::named(keyboard::key::Named::Escape)],
        "vim.exit_insert",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::Backspace)],
        "insert.backspace",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::Delete)],
        "insert.delete",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::Enter)],
        "insert.newline",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::Tab)],
        "insert.tab",
    );

    // Arrow keys in insert mode
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowLeft)],
        "cursor.move_left",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowRight)],
        "cursor.move_right",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowUp)],
        "cursor.move_up",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowDown)],
        "cursor.move_down",
    );

    // Option arrow keys with modifiers for word/line movement in insert mode
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowLeft).alt()],
        "cursor.move_word_backward",
    );
    km.bind(
        vec![KeyPress::named(keyboard::key::Named::ArrowRight).alt()],
        "cursor.move_word_forward",
    );

    km
}
