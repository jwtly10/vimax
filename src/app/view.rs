use crate::layout::{LayoutNode, SplitDirection};
use crate::text_grid;

use iced::widget::Space;
use iced::widget::{column, container, row, text};
use iced::{Element, Length, Theme};

use super::{Message, Remax};

impl Remax {
    pub fn view(&self) -> Element<'_, Message> {
        let ws = self.editor.workspace();
        let active_win_id = ws.active_window;

        let editor_area = self.build_layout_view(&ws.layout, ws, active_win_id);

        let active_buf = self.editor.buffer();
        let active_win = ws.window();
        let (cursor_line, cursor_col) = active_buf.cursor_position(active_win.cursor);

        let (mr, mg, mb) = self.vim.mode_color();
        let mode_label = text(format!(" {} ", self.vim.mode()))
            .size(14)
            .color(iced::Color::from_rgb(mr, mg, mb));

        let modified_indicator = if active_buf.is_modified() { "[+]" } else { "" };

        let buffer_name = text(format!(" {} {}", active_buf.name(), modified_indicator))
            .size(14)
            .color(iced::Color::from_rgb(0.8, 0.8, 0.8));

        let position = text(format!(" {}:{} ", cursor_line + 1, cursor_col + 1))
            .size(14)
            .color(iced::Color::from_rgb(0.6, 0.6, 0.6));

        let modeline = container(
            row![
                mode_label,
                buffer_name,
                Space::new().width(Length::Fill),
                position
            ]
            .align_y(iced::Alignment::Center),
        )
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgb(
                0.15, 0.15, 0.2,
            ))),
            ..Default::default()
        })
        .width(Length::Fill)
        .padding([2, 0]);

        let bottom_section: Element<'_, Message> = if let Some(picker) = &self.picker {
            let prompt = container(
                text(format!(" {}", picker.status_line()))
                    .size(14)
                    .color(iced::Color::from_rgb(0.9, 0.9, 0.5)),
            )
            .width(Length::Fill)
            .padding([2, 0]);

            let mut items_col = column![];
            for (i, &item_idx) in picker.visible_items() {
                let item = &picker.items[item_idx];
                let is_selected = i == picker.selected;
                let label = if is_selected {
                    format!(" > {}", item.label)
                } else {
                    format!("   {}", item.label)
                };
                let text_color = if is_selected {
                    iced::Color::from_rgb(1.0, 1.0, 1.0)
                } else {
                    iced::Color::from_rgb(0.6, 0.6, 0.6)
                };
                let row_widget = container(text(label).size(14).color(text_color))
                    .width(Length::Fill)
                    .padding([1, 3]);
                let row_widget = if is_selected {
                    row_widget.style(|_theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgb(
                            0.25, 0.35, 0.5,
                        ))),
                        ..Default::default()
                    })
                } else {
                    row_widget
                };
                items_col = items_col.push(row_widget);
            }

            column![prompt, items_col].into()
        } else {
            let cmdline_text = self
                .vim
                .status_line_override()
                .unwrap_or_else(|| self.editor.status_message.clone());

            container(
                text(format!(" {}", cmdline_text))
                    .size(14)
                    .color(iced::Color::from_rgb(0.8, 0.8, 0.8)),
            )
            .width(Length::Fill)
            .padding([2, 0])
            .into()
        };

        let content = column![editor_area, modeline, bottom_section];

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.1, 0.1, 0.12,
                ))),
                ..Default::default()
            })
            .into()
    }

    pub(crate) fn build_layout_view<'a>(
        &'a self,
        node: &LayoutNode,
        ws: &'a crate::workspace::Workspace,
        active_win_id: usize,
    ) -> Element<'a, Message> {
        match node {
            LayoutNode::Leaf(win_id) => {
                let win = &ws.windows[*win_id];
                let buffer = &self.editor.buffers[win.buffer_id];
                let is_active = *win_id == active_win_id;

                let highlights =
                    if let Some(Some(state)) = self.editor.syntax_states.get(win.buffer_id) {
                        let rope = buffer.rope();
                        let start_byte = rope.line_to_byte(win.scroll_y) as u32;
                        let end_line = (win.scroll_y + win.visible_lines + 2).min(rope.len_lines());
                        let end_byte = if end_line < rope.len_lines() {
                            rope.line_to_byte(end_line) as u32
                        } else {
                            rope.len_bytes() as u32
                        };
                        state.highlights_for_range(rope, &self.editor.loader, start_byte, end_byte)
                    } else {
                        Vec::new()
                    };

                let grid = text_grid::text_grid(
                    buffer,
                    win.cursor,
                    win.scroll_y,
                    win.scroll_x,
                    win.selection,
                    &win.search_matches,
                    if is_active {
                        self.editor.search_len()
                    } else {
                        0
                    },
                    *win_id,
                    is_active,
                    highlights,
                );

                container(grid)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
            LayoutNode::Split {
                direction,
                children,
                ..
            } => {
                let first = self.build_layout_view(&children[0], ws, active_win_id);
                let second = self.build_layout_view(&children[1], ws, active_win_id);

                let separator_color = iced::Color::from_rgb(0.3, 0.3, 0.35);

                match direction {
                    SplitDirection::Vertical => {
                        let sep = container(Space::new())
                            .width(Length::Fixed(1.0))
                            .height(Length::Fill)
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(iced::Background::Color(separator_color)),
                                ..Default::default()
                            });
                        row![first, sep, second].into()
                    }
                    SplitDirection::Horizontal => {
                        let sep = container(Space::new())
                            .width(Length::Fill)
                            .height(Length::Fixed(1.0))
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(iced::Background::Color(separator_color)),
                                ..Default::default()
                            });
                        column![first, sep, second].into()
                    }
                }
            }
        }
    }
}
