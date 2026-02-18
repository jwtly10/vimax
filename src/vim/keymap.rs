use std::collections::HashMap;

use iced::keyboard;
use tracing::trace;

use crate::action::Motion;

use super::mode::VimMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum Command {
    // Cursor / motions
    CursorMoveLeft,
    CursorMoveRight,
    CursorMoveUp,
    CursorMoveDown,
    CursorMoveWordForward,
    CursorMoveWordBackward,
    CursorMoveWordEnd,
    CursorMoveLineStart,
    CursorMoveLineEnd,
    CursorMoveFirstNonWhitespace,
    CursorMoveToStart,
    CursorMoveToEnd,

    // Scroll
    ScrollHalfDown,
    ScrollHalfUp,

    // Edit
    EditDeleteTillEol,
    EditDeleteCharForward,
    EditDeleteLine,
    EditUndo,
    EditRedo,
    EditPasteAfter,
    EditPasteBefore,
    EditReplaceChar,

    // Insert-mode editing
    InsertBackspace,
    InsertDelete,
    InsertNewline,
    InsertTab,

    // Operators
    OpDelete,
    OpChange,
    OpYank,

    // Vim mode transitions
    VimEnterInsertMode,
    VimEnterInsertAfter,
    VimEnterInsertLineEnd,
    VimEnterInsertLineStart,
    VimOpenBelow,
    VimOpenAbove,
    VimExitInsertMode,
    VimEnterCommandMode,
    VimEnterVisualMode,
    VimEnterVisualLine,
    VimExitVisualMode,
    VimClearSearch,
    VimEnterSearchMode,

    // Find-char motions
    MotionFindCharForward,
    MotionFindCharBackward,
    MotionFindCharForwardBefore,
    MotionFindCharBackwardBefore,
    MotionRepeatFindChar,
    MotionRepeatFindCharReverse,

    // Search
    SearchNext,
    SearchPrev,

    // Buffer
    BufferSave,
    BufferQuit,
    BufferForceQuit,
    BufferWriteQuit,

    // Window
    WindowVsplit,
    WindowHsplit,
    WindowClose,
    WindowFocusLeft,
    WindowFocusRight,
    WindowFocusUp,
    WindowFocusDown,

    // System clipboard
    SystemCopy,
    SystemCut,
    SystemPaste,

    // Visual-mode operations
    VisualDelete,
    VisualYank,
    VisualChange,

    // Picker
    PickerBuffers,
    PickerProjectFiles,
    PickerProjectFilesShowIgnored,

    // LSP
    LspGotoDefinition,
    LspGotoReferences,
    LspGotoImplementation,
    LspGotoDeclaration,

    // Jump list
    JumpBackward,
    JumpForward,

    // Diagnostics
    PickerDiagnostics,

    // Hover / Info panel
    LspHover,
    DiagnosticHover,
}

impl Command {
    pub fn is_operator(self) -> bool {
        matches!(
            self,
            Command::OpDelete | Command::OpChange | Command::OpYank
        )
    }

    /// If this command represents a cursor motion, return the corresponding
    /// `Motion` value and whether the motion is inclusive
    pub fn as_motion(self) -> Option<(Motion, bool)> {
        match self {
            Command::CursorMoveLeft => Some((Motion::Left, false)),
            Command::CursorMoveRight => Some((Motion::Right, false)),
            Command::CursorMoveUp => Some((Motion::Up, false)),
            Command::CursorMoveDown => Some((Motion::Down, false)),
            Command::CursorMoveWordForward => Some((Motion::WordForward, false)),
            Command::CursorMoveWordBackward => Some((Motion::WordBackward, false)),
            Command::CursorMoveWordEnd => Some((Motion::WordEnd, true)),
            Command::CursorMoveLineStart => Some((Motion::LineStart, false)),
            Command::CursorMoveLineEnd => Some((Motion::LineEnd, true)),
            Command::CursorMoveFirstNonWhitespace => Some((Motion::FirstNonWhitespace, false)),
            Command::CursorMoveToStart => Some((Motion::FileStart, false)),
            Command::CursorMoveToEnd => Some((Motion::FileEnd, false)),
            _ => None,
        }
    }

    /// Returns the (forward, stop_before) pair for find-char motions.
    pub fn find_char_params(self) -> Option<(bool, bool)> {
        match self {
            Command::MotionFindCharForward => Some((true, false)),
            Command::MotionFindCharBackward => Some((false, false)),
            Command::MotionFindCharForwardBefore => Some((true, true)),
            Command::MotionFindCharBackwardBefore => Some((false, true)),
            _ => None,
        }
    }
}

const LEADER_KEY: KeyPress = KeyPress {
    key: KeyId::Named(keyboard::key::Named::Space),
    ctrl: false,
    alt: false,
    cmd: false,
};

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

    pub fn from_iced(key: &keyboard::Key, modifiers: &keyboard::Modifiers) -> Option<Self> {
        let key_id = match key {
            keyboard::Key::Character(c) => {
                let s = c.as_str();
                let mut chars = s.chars();
                let ch = chars.next()?;
                if chars.next().is_some() {
                    return None;
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
    Match(Command),
    Pending,
    NoMatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: HashMap<Vec<KeyPress>, Command>,
    prefixes: HashMap<Vec<KeyPress>, ()>,
}

impl Keymap {
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            prefixes: HashMap::new(),
        }
    }

    pub fn bind(&mut self, keys: Vec<KeyPress>, command: Command) {
        for len in 1..keys.len() {
            self.prefixes.insert(keys[..len].to_vec(), ());
        }
        self.bindings.insert(keys, command);
    }

    pub fn lookup(&self, keys: &[KeyPress]) -> KeymapLookup {
        trace!(?keys, "key command lookup");
        if let Some(&cmd) = self.bindings.get(keys) {
            trace!(?cmd, "key command found");
            KeymapLookup::Match(cmd)
        } else if self.prefixes.contains_key(keys) {
            trace!("starting key sequence, waiting for more input");
            KeymapLookup::Pending
        } else {
            trace!("no matching key sequence");
            KeymapLookup::NoMatch
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingScope {
    /// Available in normal, visual, and insert (arrow keys, etc.)
    Motion,
    /// Normal + visual (hjkl motions, operators, find-char, …)
    NormalAndVisual,
    /// Normal mode only
    NormalOnly,
    /// Visual mode only
    VisualOnly,
    /// Insert mode only
    InsertOnly,
    /// Global keymap (ctrl/cmd chords available everywhere)
    Global,
}

struct AnnotatedBinding {
    keys: Vec<KeyPress>,
    command: Command,
    scope: BindingScope,
}

/// Declarative macro that produces a `Vec<AnnotatedBinding>`.
///
/// Usage:
/// ```ignore
/// keybindings! {
///     // scope => [key sequence] => Command,
///     Motion  => [KeyPress::char('h')] => Command::CursorMoveLeft,
///     Global  => [KeyPress::char('r').ctrl()] => Command::EditRedo,
/// }
/// ```
macro_rules! keybindings {
    ( $( $scope:ident => [ $($key:expr),+ $(,)? ] => $cmd:expr ),+ $(,)? ) => {
        vec![
            $(
                AnnotatedBinding {
                    keys: vec![ $($key),+ ],
                    command: $cmd,
                    scope: BindingScope::$scope,
                }
            ),+
        ]
    };
}

pub struct Keymaps {
    global: Keymap,
    normal: Keymap,
    insert: Keymap,
    visual: Keymap,
}

impl Keymaps {
    pub fn new() -> Self {
        let bindings = all_bindings();

        let mut global = Keymap::new();
        let mut normal = Keymap::new();
        let mut insert = Keymap::new();
        let mut visual = Keymap::new();

        for b in bindings {
            match b.scope {
                BindingScope::Global => {
                    global.bind(b.keys, b.command);
                }
                BindingScope::Motion => {
                    normal.bind(b.keys.clone(), b.command);
                    visual.bind(b.keys.clone(), b.command);
                    insert.bind(b.keys, b.command);
                }
                BindingScope::NormalAndVisual => {
                    normal.bind(b.keys.clone(), b.command);
                    visual.bind(b.keys, b.command);
                }
                BindingScope::NormalOnly => {
                    normal.bind(b.keys, b.command);
                }
                BindingScope::VisualOnly => {
                    visual.bind(b.keys, b.command);
                }
                BindingScope::InsertOnly => {
                    insert.bind(b.keys, b.command);
                }
            }
        }

        Self {
            global,
            normal,
            insert,
            visual,
        }
    }

    pub fn global(&self) -> &Keymap {
        &self.global
    }

    pub fn for_mode(&self, mode: VimMode) -> &Keymap {
        match mode {
            VimMode::Normal => &self.normal,
            VimMode::Insert => &self.insert,
            VimMode::Visual | VimMode::VisualLine => &self.visual,
            VimMode::Command | VimMode::Search => &self.insert,
        }
    }
}

fn all_bindings() -> Vec<AnnotatedBinding> {
    use keyboard::key::Named;

    keybindings! {
        // ── Global (ctrl / cmd chords) ──────────────────────────────────
        Global => [KeyPress::char('r').ctrl()]              => Command::EditRedo,
        Global => [KeyPress::char('d').ctrl()]              => Command::ScrollHalfDown,
        Global => [KeyPress::char('u').ctrl()]              => Command::ScrollHalfUp,
        Global => [KeyPress::char('s').cmd()]               => Command::BufferSave,
        Global => [KeyPress::char('w').ctrl(), KeyPress::char('v')]  => Command::WindowVsplit,
        Global => [KeyPress::char('w').ctrl(), KeyPress::char('s')]  => Command::WindowHsplit,
        Global => [KeyPress::char('w').ctrl(), KeyPress::char('c')]  => Command::WindowClose,
        Global => [KeyPress::char('w').ctrl(), KeyPress::char('h')]  => Command::WindowFocusLeft,
        Global => [KeyPress::char('w').ctrl(), KeyPress::char('j')]  => Command::WindowFocusDown,
        Global => [KeyPress::char('w').ctrl(), KeyPress::char('k')]  => Command::WindowFocusUp,
        Global => [KeyPress::char('w').ctrl(), KeyPress::char('l')]  => Command::WindowFocusRight,
        Global => [KeyPress::char('o').ctrl()]              => Command::JumpBackward,
        Global => [KeyPress::char('i').ctrl()]              => Command::JumpForward,
        Global => [KeyPress::char('c').cmd()]               => Command::SystemCopy,
        Global => [KeyPress::char('x').cmd()]               => Command::SystemCut,
        Global => [KeyPress::char('v').cmd()]               => Command::SystemPaste,
        Global => [KeyPress::char('w').cmd()]               => Command::WindowClose,

        // ── Motion (normal + visual + insert arrow keys) ────────────────
        Motion => [KeyPress::named(Named::ArrowLeft)]       => Command::CursorMoveLeft,
        Motion => [KeyPress::named(Named::ArrowRight)]      => Command::CursorMoveRight,
        Motion => [KeyPress::named(Named::ArrowUp)]         => Command::CursorMoveUp,
        Motion => [KeyPress::named(Named::ArrowDown)]       => Command::CursorMoveDown,

        // ── Normal + Visual shared ──────────────────────────────────────
        NormalAndVisual => [KeyPress::char('h')]            => Command::CursorMoveLeft,
        NormalAndVisual => [KeyPress::char('j')]            => Command::CursorMoveDown,
        NormalAndVisual => [KeyPress::char('k')]            => Command::CursorMoveUp,
        NormalAndVisual => [KeyPress::char('l')]            => Command::CursorMoveRight,
        NormalAndVisual => [KeyPress::char('w')]            => Command::CursorMoveWordForward,
        NormalAndVisual => [KeyPress::char('b')]            => Command::CursorMoveWordBackward,
        NormalAndVisual => [KeyPress::char('0')]            => Command::CursorMoveLineStart,
        NormalAndVisual => [KeyPress::char('$')]            => Command::CursorMoveLineEnd,
        NormalAndVisual => [KeyPress::char('e')]            => Command::CursorMoveWordEnd,
        NormalAndVisual => [KeyPress::char('^')]            => Command::CursorMoveFirstNonWhitespace,
        NormalAndVisual => [KeyPress::char('G')]            => Command::CursorMoveToEnd,
        NormalAndVisual => [KeyPress::char('g'), KeyPress::char('g')] => Command::CursorMoveToStart,
        NormalAndVisual => [KeyPress::char('f')]            => Command::MotionFindCharForward,
        NormalAndVisual => [KeyPress::char('F')]            => Command::MotionFindCharBackward,
        NormalAndVisual => [KeyPress::char('t')]            => Command::MotionFindCharForwardBefore,
        NormalAndVisual => [KeyPress::char('T')]            => Command::MotionFindCharBackwardBefore,
        NormalAndVisual => [KeyPress::char(';')]            => Command::MotionRepeatFindChar,
        NormalAndVisual => [KeyPress::char(',')]            => Command::MotionRepeatFindCharReverse,

        // ── Normal only ─────────────────────────────────────────────────
        NormalOnly => [KeyPress::char('g'), KeyPress::char('d')] => Command::LspGotoDefinition,
        NormalOnly => [KeyPress::char('g'), KeyPress::char('r')] => Command::LspGotoReferences,
        NormalOnly => [KeyPress::char('g'), KeyPress::char('i')] => Command::LspGotoImplementation,
        NormalOnly => [KeyPress::char('g'), KeyPress::char('D')] => Command::LspGotoDeclaration,
        NormalOnly => [LEADER_KEY, KeyPress::char('b'), KeyPress::char('b')] => Command::PickerBuffers,
        NormalOnly => [LEADER_KEY, KeyPress::char('p'), KeyPress::char('f')] => Command::PickerProjectFiles,
        NormalOnly => [LEADER_KEY, KeyPress::char('p'), KeyPress::char('g')] => Command::PickerProjectFilesShowIgnored,
        NormalOnly => [LEADER_KEY, KeyPress::char('x'), KeyPress::char('x')] => Command::PickerDiagnostics,
        NormalOnly => [KeyPress::char('D')]                 => Command::EditDeleteTillEol,
        NormalOnly => [KeyPress::char('x')]                 => Command::EditDeleteCharForward,
        NormalOnly => [KeyPress::char('u')]                 => Command::EditUndo,
        NormalOnly => [KeyPress::char('d')]                 => Command::OpDelete,
        NormalOnly => [KeyPress::char('c')]                 => Command::OpChange,
        NormalOnly => [KeyPress::char('y')]                 => Command::OpYank,
        NormalOnly => [KeyPress::char('p')]                 => Command::EditPasteAfter,
        NormalOnly => [KeyPress::char('P')]                 => Command::EditPasteBefore,
        NormalOnly => [KeyPress::char('i')]                 => Command::VimEnterInsertMode,
        NormalOnly => [KeyPress::char('a')]                 => Command::VimEnterInsertAfter,
        NormalOnly => [KeyPress::char('A')]                 => Command::VimEnterInsertLineEnd,
        NormalOnly => [KeyPress::char('I')]                 => Command::VimEnterInsertLineStart,
        NormalOnly => [KeyPress::char('o')]                 => Command::VimOpenBelow,
        NormalOnly => [KeyPress::char('O')]                 => Command::VimOpenAbove,
        NormalOnly => [KeyPress::char('v')]                 => Command::VimEnterVisualMode,
        NormalOnly => [KeyPress::char('V')]                 => Command::VimEnterVisualLine,
        NormalOnly => [KeyPress::char(':')]                 => Command::VimEnterCommandMode,
        NormalOnly => [KeyPress::named(Named::Escape)]      => Command::VimClearSearch,
        NormalOnly => [KeyPress::char('/')]                 => Command::VimEnterSearchMode,
        NormalOnly => [KeyPress::char('n')]                 => Command::SearchNext,
        NormalOnly => [KeyPress::char('N')]                 => Command::SearchPrev,
        NormalOnly => [KeyPress::char('r')]                 => Command::EditReplaceChar,
        NormalOnly => [KeyPress::char('K')]                 => Command::LspHover,
        NormalOnly => [KeyPress::char('g'), KeyPress::char('t')] => Command::DiagnosticHover,

        // ── Visual only ─────────────────────────────────────────────────
        VisualOnly => [KeyPress::char('d')]                 => Command::VisualDelete,
        VisualOnly => [KeyPress::char('x')]                 => Command::VisualDelete,
        VisualOnly => [KeyPress::char('y')]                 => Command::VisualYank,
        VisualOnly => [KeyPress::char('c')]                 => Command::VisualChange,
        VisualOnly => [KeyPress::named(Named::Escape)]      => Command::VimExitVisualMode,
        VisualOnly => [KeyPress::char('v')]                 => Command::VimExitVisualMode,
        VisualOnly => [KeyPress::char('V')]                 => Command::VimEnterVisualLine,

        // ── Insert only ─────────────────────────────────────────────────
        InsertOnly => [KeyPress::named(Named::Escape)]      => Command::VimExitInsertMode,
        InsertOnly => [KeyPress::named(Named::Backspace)]   => Command::InsertBackspace,
        InsertOnly => [KeyPress::named(Named::Delete)]      => Command::InsertDelete,
        InsertOnly => [KeyPress::named(Named::Enter)]       => Command::InsertNewline,
        InsertOnly => [KeyPress::named(Named::Tab)]         => Command::InsertTab,
        InsertOnly => [KeyPress::named(Named::ArrowLeft).alt()]  => Command::CursorMoveWordBackward,
        InsertOnly => [KeyPress::named(Named::ArrowRight).alt()] => Command::CursorMoveWordForward,
    }
}
