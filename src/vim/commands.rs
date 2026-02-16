use crate::action::{EditorAction, Motion};
use crate::buffer::Buffer;

use super::input::InputState;
use super::keymap::Command;

pub fn resolve(
    cmd: Command,
    count: usize,
    input: &mut InputState,
    buffer: &Buffer,
    cursor: usize,
) -> Vec<EditorAction> {
    match cmd {
        Command::CursorMoveLeft => vec![EditorAction::MoveCursor {
            motion: Motion::Left,
            count,
        }],
        Command::CursorMoveRight => vec![EditorAction::MoveCursor {
            motion: Motion::Right,
            count,
        }],
        Command::CursorMoveUp => vec![EditorAction::MoveCursor {
            motion: Motion::Up,
            count,
        }],
        Command::CursorMoveDown => vec![EditorAction::MoveCursor {
            motion: Motion::Down,
            count,
        }],
        Command::CursorMoveWordForward => vec![EditorAction::MoveCursor {
            motion: Motion::WordForward,
            count,
        }],
        Command::CursorMoveWordBackward => vec![EditorAction::MoveCursor {
            motion: Motion::WordBackward,
            count,
        }],
        Command::CursorMoveWordEnd => vec![EditorAction::MoveCursor {
            motion: Motion::WordEnd,
            count,
        }],
        Command::CursorMoveLineStart => vec![EditorAction::MoveCursor {
            motion: Motion::LineStart,
            count: 1,
        }],
        Command::CursorMoveLineEnd => vec![EditorAction::MoveCursor {
            motion: Motion::LineEnd,
            count: 1,
        }],
        Command::CursorMoveFirstNonWhitespace => vec![EditorAction::MoveCursor {
            motion: Motion::FirstNonWhitespace,
            count: 1,
        }],
        Command::CursorMoveToStart => vec![EditorAction::MoveCursor {
            motion: Motion::FileStart,
            count: 1,
        }],
        Command::CursorMoveToEnd => vec![EditorAction::MoveCursor {
            motion: Motion::FileEnd,
            count: 1,
        }],

        Command::ScrollHalfDown => vec![EditorAction::MoveCursor {
            motion: Motion::HalfPageDown,
            count,
        }],
        Command::ScrollHalfUp => vec![EditorAction::MoveCursor {
            motion: Motion::HalfPageUp,
            count,
        }],

        Command::EditDeleteTillEol => vec![EditorAction::DeleteTillEndOfLine],
        Command::EditDeleteCharForward => vec![EditorAction::DeleteCharForward { count }],
        Command::EditDeleteLine => vec![EditorAction::DeleteLine { count }],
        Command::EditUndo => vec![EditorAction::Undo],
        Command::EditRedo => vec![EditorAction::Redo],
        Command::EditPasteAfter => vec![EditorAction::Paste { before: false }],
        Command::EditPasteBefore => vec![EditorAction::Paste { before: true }],
        Command::EditReplaceChar => {
            input.pending_replace = true;
            vec![]
        }

        Command::InsertBackspace => vec![EditorAction::DeleteCharBackward],
        Command::InsertDelete => vec![EditorAction::DeleteCharForward { count: 1 }],
        Command::InsertNewline => vec![EditorAction::InsertNewline],
        Command::InsertTab => vec![EditorAction::InsertTab],

        Command::VimEnterInsertMode => input.enter_insert(),
        Command::VimEnterInsertAfter => input.enter_insert_after(),
        Command::VimEnterInsertLineEnd => input.enter_insert_line_end(),
        Command::VimEnterInsertLineStart => input.enter_insert_line_start(),
        Command::VimOpenBelow => input.open_below(),
        Command::VimOpenAbove => input.open_above(),
        Command::VimExitInsertMode => input.exit_insert(),
        Command::VimEnterCommandMode => {
            input.enter_command();
            vec![]
        }
        Command::VimEnterVisualMode => {
            input.enter_visual(cursor);
            vec![]
        }
        Command::VimEnterVisualLine => {
            input.enter_visual_line(cursor);
            vec![]
        }
        Command::VimExitVisualMode => input.exit_visual(),
        Command::VimClearSearch => vec![EditorAction::ClearSearch],
        Command::VimEnterSearchMode => {
            input.enter_search();
            vec![]
        }

        Command::LspGotoDefinition => vec![EditorAction::LspGotoDefinition],
        Command::LspGotoReferences => vec![EditorAction::LspReferences],
        Command::LspGotoImplementation => vec![EditorAction::LspImplementation],
        Command::LspGotoDeclaration => vec![EditorAction::LspDeclaration],

        Command::PickerBuffers => input.open_buffer_picker(),
        Command::PickerProjectFiles => input.open_project_files_picker(false),
        Command::PickerProjectFilesShowIgnored => input.open_project_files_picker(true),

        Command::SearchNext => vec![EditorAction::SearchNext { count }],
        Command::SearchPrev => vec![EditorAction::SearchPrev { count }],

        Command::JumpBackward => vec![EditorAction::JumpBackward],
        Command::JumpForward => vec![EditorAction::JumpForward],

        Command::OpDelete | Command::OpChange | Command::OpYank => vec![],

        Command::VisualDelete => input.visual_delete(buffer, cursor),
        Command::VisualYank => input.visual_yank(buffer, cursor),
        Command::VisualChange => input.visual_change(buffer, cursor),

        Command::MotionFindCharForward => {
            input.pending_find_char = Some((true, false));
            vec![]
        }
        Command::MotionFindCharBackward => {
            input.pending_find_char = Some((false, false));
            vec![]
        }
        Command::MotionFindCharForwardBefore => {
            input.pending_find_char = Some((true, true));
            vec![]
        }
        Command::MotionFindCharBackwardBefore => {
            input.pending_find_char = Some((false, true));
            vec![]
        }
        Command::MotionRepeatFindChar => {
            if let Some((ch, forward, stop_before)) = input.last_find_char {
                vec![EditorAction::MoveCursor {
                    motion: Motion::FindChar {
                        ch,
                        forward,
                        stop_before,
                    },
                    count,
                }]
            } else {
                vec![]
            }
        }
        Command::MotionRepeatFindCharReverse => {
            if let Some((ch, forward, stop_before)) = input.last_find_char {
                vec![EditorAction::MoveCursor {
                    motion: Motion::FindChar {
                        ch,
                        forward: !forward,
                        stop_before,
                    },
                    count,
                }]
            } else {
                vec![]
            }
        }

        Command::BufferSave => vec![EditorAction::Save],
        Command::BufferQuit => vec![EditorAction::Quit { force: false }],
        Command::BufferForceQuit => vec![EditorAction::Quit { force: true }],
        Command::BufferWriteQuit => vec![EditorAction::WriteQuit],

        Command::WindowVsplit => vec![EditorAction::VSplit],
        Command::WindowHsplit => vec![EditorAction::HSplit],
        Command::WindowClose => vec![EditorAction::CloseWindow],
        Command::WindowFocusLeft => vec![EditorAction::FocusLeft],
        Command::WindowFocusRight => vec![EditorAction::FocusRight],
        Command::WindowFocusUp => vec![EditorAction::FocusUp],
        Command::WindowFocusDown => vec![EditorAction::FocusDown],

        Command::SystemCopy => vec![EditorAction::SystemCopy],
        Command::SystemCut => vec![EditorAction::SystemCut],
        Command::SystemPaste => vec![EditorAction::SystemPaste],
    }
}
