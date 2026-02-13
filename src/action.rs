use iced::Task;

use crate::app::Message;
use crate::vim::mode::VimMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    LineStart,
    LineEnd,
    FirstNonWhitespace,
    FileStart,
    FileEnd,
    FindChar {
        ch: char,
        forward: bool,
        stop_before: bool,
    },
    HalfPageDown,
    HalfPageUp,
}

#[derive(Debug, Clone)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub enum EditorAction {
    MoveCursor { motion: Motion, count: usize },
    SetCursor(usize),
    InsertChar(char),
    InsertNewline,
    InsertTab,
    DeleteTillEndOfLine,
    DeleteCharForward { count: usize },
    DeleteCharBackward,
    DeleteLine { count: usize },
    DeleteRange(Range),
    ChangeRange(Range),
    YankRange(Range),
    ReplaceChar(char),
    Paste { before: bool },
    Undo,
    Redo,
    StartEditGroup,
    FinishEditGroup,
    SetSearchPattern(String),
    SearchNext { count: usize },
    SearchPrev { count: usize },
    ClearSearch,
    Save,
    Quit { force: bool },
    WriteQuit,
    SetMode(String),
    SetStatusMessage(String),
    SetSelection(Option<(usize, usize)>),
    UpdateVisualSelection { anchor: usize, mode: VimMode },
    OpenFile(std::path::PathBuf),
    NextBuffer,
    PrevBuffer,
    CloseBuffer,
    VSplit,
    HSplit,
    CloseWindow,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    SystemCopy,
    SystemCut,
    SystemPaste,
    ForceQuitApp,
}

pub enum EditorEffect {
    None,
    Task(Task<Message>),
}
