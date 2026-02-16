use std::path::PathBuf;

use crate::action::PickerKind;
use crate::picker::{Picker, PickerItem};

use iced::keyboard;
use iced::Task;

use super::{Message, Remax};

impl Remax {
    pub(crate) fn open_picker(&mut self, kind: PickerKind) {
        self.active_picker_type = Some(kind);
        match kind {
            PickerKind::Buffers => {
                let buf_list = self.editor.buffer_list();
                let items: Vec<PickerItem> = buf_list
                    .into_iter()
                    .map(|(id, label)| PickerItem { id, label })
                    .collect();
                self.picker_restore_buffer = Some(self.editor.workspace().window().buffer_id);
                self.picker = Some(Picker::new("Buffers", items));
                if !self.picker.as_ref().unwrap().items.is_empty() {
                    self.preview_selected_buffer();
                }
            }
            PickerKind::ProjectFiles {
                show_ignored,
                max_results,
            } => {
                let cwd = &self.editor.workspace().cwd;
                let files = ignore::WalkBuilder::new(cwd)
                    .git_ignore(!show_ignored)
                    .git_exclude(!show_ignored)
                    .filter_entry(|entry| {
                        // TODO: this will be configurable
                        // there are some dirs we just never want to look at
                        let custom_ignores = [".git", "target", "node_modules", "dist", "build"];
                        let file_name = entry.file_name().to_string_lossy();
                        if custom_ignores.contains(&file_name.as_ref()) {
                            return false;
                        }

                        true
                    })
                    .build()
                    .filter_map(|entry| entry.ok())
                    .take(max_results)
                    .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
                    .map(|entry| {
                        let path = entry.path();
                        let label = path.strip_prefix(cwd).unwrap_or(path).display().to_string();
                        debug_assert!(!label.is_empty(), "file label should not be empty");
                        PickerItem { id: 0, label }
                    })
                    .collect::<Vec<_>>();
                self.picker_restore_buffer = Some(self.editor.workspace().window().buffer_id);
                self.picker = Some(Picker::new("Git Files", files));
            }
        }
    }

    fn preview_selected_buffer(&mut self) {
        if let Some(picker) = &self.picker
            && let Some(item) = picker.selected_item()
        {
            let buf_id = item.id;
            let len_chars = self.editor.buffers[buf_id].len_chars();
            self.editor.workspace_mut().switch_buffer(buf_id, len_chars);
        }
    }

    pub(crate) fn handle_picker_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        text: Option<&str>,
    ) -> Task<Message> {
        match key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                if let Some(buf_id) = self.picker_restore_buffer.take() {
                    let len_chars = self.editor.buffers[buf_id].len_chars();
                    self.editor.workspace_mut().switch_buffer(buf_id, len_chars);
                }
                self.picker = None;
                self.active_picker_type = None;
            }
            keyboard::Key::Named(keyboard::key::Named::Enter) => {
                if let Some(PickerKind::ProjectFiles {
                    show_ignored: _,
                    max_results: _,
                }) = self.active_picker_type
                    && let Some(picker) = &self.picker
                    && let Some(item) = picker.selected_item()
                {
                    self.editor.open_file(&PathBuf::from(&item.label));
                }

                self.picker = None;
                self.active_picker_type = None;
                self.picker_restore_buffer = None;
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                if let Some(picker) = &mut self.picker {
                    picker.move_up();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                if let Some(picker) = &mut self.picker {
                    picker.move_down();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            keyboard::Key::Character(c) if modifiers.control() && c.as_str() == "p" => {
                if let Some(picker) = &mut self.picker {
                    picker.move_up();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            keyboard::Key::Character(c) if modifiers.control() && c.as_str() == "n" => {
                if let Some(picker) = &mut self.picker {
                    picker.move_down();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                if let Some(picker) = &mut self.picker {
                    picker.backspace();
                }
                if let Some(PickerKind::Buffers) = self.active_picker_type {
                    self.preview_selected_buffer()
                }
            }
            _ => {
                if let Some(t) = text {
                    if let Some(picker) = &mut self.picker {
                        for ch in t.chars() {
                            if !ch.is_control() {
                                picker.type_char(ch);
                            }
                        }
                    }
                    if let Some(PickerKind::Buffers) = self.active_picker_type {
                        self.preview_selected_buffer();
                    }
                }
            }
        }
        Task::none()
    }
}
