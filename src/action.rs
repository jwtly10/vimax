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
}

pub enum EditorEffect {
    None,
    Task(Task<Message>),
}

pub trait BufferQuery {
    fn cursor(&self) -> usize;
    fn cursor_position(&self) -> (usize, usize);
    fn len_chars(&self) -> usize;
    fn total_lines(&self) -> usize;
    fn line_to_char(&self, line: usize) -> usize;
    fn char_to_line(&self, pos: usize) -> usize;
    fn text_object_inner_word(&self) -> (usize, usize);
    fn text_object_a_word(&self) -> (usize, usize);
    fn text_object_delimited(&self, open: char, close: char, include: bool) -> (usize, usize);
    fn text_object_quoted(&self, quote: char, include: bool) -> (usize, usize);
    fn cursor_after_motion(&self, motion: &Motion, count: usize) -> usize;
}
