pub mod commands;
pub mod input;
pub mod keymap;
pub mod mode;

use crate::action::EditorAction;
use crate::buffer::Buffer;

use self::input::InputState;
use self::keymap::Keymaps;
use self::mode::VimMode;

use iced::keyboard;

pub struct VimLayer {
    input: InputState,
    keymaps: Keymaps,
}

impl VimLayer {
    pub fn new() -> Self {
        Self {
            input: InputState::new(),
            keymaps: Keymaps::new(),
        }
    }

    pub fn handle_key(
        &mut self,
        key: &keyboard::Key,
        modified_key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        text: Option<&str>,
        buffer: &Buffer,
        cursor: usize,
    ) -> Vec<EditorAction> {
        let mode = self.input.mode;
        let global = self.keymaps.global();
        let mode_keymap = self.keymaps.for_mode(mode);

        let actions =
            self.input
                .handle_key(key, modified_key, modifiers, text, global, mode_keymap, buffer, cursor);

        if matches!(self.input.mode, VimMode::Visual | VimMode::VisualLine)
            && !actions.iter().any(|a| matches!(a, EditorAction::SetSelection(_)))
            && let Some(anchor) = self.input.selection_anchor
        {
            let mut result = actions;
            result.push(EditorAction::UpdateVisualSelection {
                anchor,
                mode: self.input.mode,
            });
            return result;
        }

        actions
    }

    pub fn force_mode(&mut self, mode: VimMode) {
        self.input.mode = mode;
        self.input.selection_anchor = None;
    }

    pub fn mode(&self) -> VimMode {
        self.input.mode
    }

    pub fn status_line_override(&self) -> Option<String> {
        self.input.status_line_override()
    }

    pub fn mode_color(&self) -> (f32, f32, f32) {
        self.input.mode_color()
    }
}
