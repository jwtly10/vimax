use crate::buffer::Buffer;
use iced::advanced::layout;
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::text::{self as iced_text, Renderer as _};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::alignment;
use iced::mouse;
use iced::{Color, Element, Event, Length, Rectangle, Size};

const FONT_SIZE: f32 = 16.0;
pub const LINE_HEIGHT: f32 = 22.0;
pub const CHAR_WIDTH: f32 = 9.6;
const GUTTER_CHARS: usize = 4;
pub const GUTTER_WIDTH: f32 = GUTTER_CHARS as f32 * CHAR_WIDTH + 8.0;

pub struct TextGrid<'a> {
    buffer: &'a Buffer,
    scroll_y: usize,
    scroll_x: usize,
}

pub fn text_grid(buffer: &Buffer, scroll_y: usize, scroll_x: usize) -> Element<'_, crate::Message> {
    Element::new(TextGrid {
        buffer,
        scroll_y,
        scroll_x,
    })
}

impl<'a> Widget<crate::Message, iced::Theme, iced::Renderer> for TextGrid<'a> {
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        layout::Node::new(size)
    }

    fn update(
        &mut self,
        _tree: &mut widget::Tree,
        event: &Event,
        layout: iced::advanced::Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, crate::Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let lines = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => *y,
                        mouse::ScrollDelta::Pixels { y, .. } => *y / LINE_HEIGHT,
                    };
                    let cols = match delta {
                        mouse::ScrollDelta::Lines { x, .. } => *x,
                        mouse::ScrollDelta::Pixels { x, .. } => *x / CHAR_WIDTH,
                    };
                    if cols.abs() > 0.1 {
                        shell.publish(crate::Message::ScrollCols(cols));
                    }
                    shell.publish(crate::Message::ScrollLines(lines));
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    shell.publish(crate::Message::MouseClick { x: pos.x, y: pos.y });
                }
            }
            // Report viewport size on window resize events
            Event::Window(iced::window::Event::Resized { .. }) => {
                let visible_lines = (bounds.height / LINE_HEIGHT) as usize;
                let text_area_width = bounds.width - GUTTER_WIDTH - 8.0;
                let visible_cols = (text_area_width / CHAR_WIDTH).max(1.0) as usize;
                shell.publish(crate::Message::ViewportResized {
                    lines: visible_lines,
                    cols: visible_cols,
                });
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let rope = self.buffer.rope();
        let (cursor_line, cursor_col) = self.buffer.cursor_position();

        let visible_lines = (bounds.height / LINE_HEIGHT) as usize;
        let total_lines = rope.len_lines();

        let scroll_y = self.scroll_y;
        let scroll_x = self.scroll_x;

        let end_line = (scroll_y + visible_lines + 1).min(total_lines);

        for (i, line_idx) in (scroll_y..end_line).enumerate() {
            let y = bounds.y + (i as f32 * LINE_HEIGHT);

            if y > bounds.y + bounds.height {
                break;
            }

            // Highlight current line background
            if line_idx == cursor_line {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x,
                            y,
                            width: bounds.width,
                            height: LINE_HEIGHT,
                        },
                        ..renderer::Quad::default()
                    },
                    Color::from_rgba(1.0, 1.0, 1.0, 0.03),
                );
            }

            // Line number
            let line_num = format!("{:>width$}", line_idx + 1, width = GUTTER_CHARS);
            let line_num_color = if line_idx == cursor_line {
                Color::from_rgb(0.9, 0.9, 0.5)
            } else {
                Color::from_rgb(0.4, 0.4, 0.4)
            };

            renderer.fill_text(
                iced_text::Text {
                    content: line_num,
                    bounds: Size::new(GUTTER_WIDTH, LINE_HEIGHT),
                    size: FONT_SIZE.into(),
                    line_height: iced_text::LineHeight::Absolute(LINE_HEIGHT.into()),
                    font: iced::Font::MONOSPACE,
                    align_x: iced::Alignment::Start.into(),
                    align_y: alignment::Vertical::Top,
                    shaping: iced_text::Shaping::Basic,
                    wrapping: iced_text::Wrapping::None,
                },
                iced::Point::new(bounds.x, y),
                line_num_color,
                *viewport,
            );

            // Line content (with horizontal scroll)
            let line = rope.line(line_idx);
            let line_str: String = line.chars().collect();
            let display_str = line_str.trim_end_matches('\n').trim_end_matches('\r');

            let text_x = bounds.x + GUTTER_WIDTH + 8.0;

            // Apply horizontal scroll: skip first scroll_x chars
            let scrolled_str: String = display_str.chars().skip(scroll_x).collect();

            let text_area_width = bounds.width - GUTTER_WIDTH - 8.0;

            renderer.fill_text(
                iced_text::Text {
                    content: scrolled_str,
                    bounds: Size::new(text_area_width, LINE_HEIGHT),
                    size: FONT_SIZE.into(),
                    line_height: iced_text::LineHeight::Absolute(LINE_HEIGHT.into()),
                    font: iced::Font::MONOSPACE,
                    align_x: iced::Alignment::Start.into(),
                    align_y: alignment::Vertical::Top,
                    shaping: iced_text::Shaping::Basic,
                    wrapping: iced_text::Wrapping::None,
                },
                iced::Point::new(text_x, y),
                Color::from_rgb(0.85, 0.85, 0.85),
                *viewport,
            );

            // Cursor block
            if line_idx == cursor_line && cursor_col >= scroll_x {
                let cursor_x = text_x + ((cursor_col - scroll_x) as f32 * CHAR_WIDTH);

                // Only draw cursor if it's within the visible text area
                if cursor_x < bounds.x + bounds.width {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: cursor_x,
                                y,
                                width: CHAR_WIDTH,
                                height: LINE_HEIGHT,
                            },
                            border: iced::border::rounded(1),
                            ..renderer::Quad::default()
                        },
                        Color::from_rgba(0.8, 0.8, 0.3, 0.7),
                    );

                    // Character under cursor
                    let char_count = display_str.chars().count();
                    let cursor_char: String = if cursor_col < char_count {
                        display_str.chars().nth(cursor_col).unwrap().to_string()
                    } else {
                        " ".to_string()
                    };

                    renderer.fill_text(
                        iced_text::Text {
                            content: cursor_char,
                            bounds: Size::new(CHAR_WIDTH, LINE_HEIGHT),
                            size: FONT_SIZE.into(),
                            line_height: iced_text::LineHeight::Absolute(LINE_HEIGHT.into()),
                            font: iced::Font::MONOSPACE,
                            align_x: iced::Alignment::Start.into(),
                            align_y: alignment::Vertical::Top,
                            shaping: iced_text::Shaping::Basic,
                            wrapping: iced_text::Wrapping::None,
                        },
                        iced::Point::new(cursor_x, y),
                        Color::from_rgb(0.1, 0.1, 0.1),
                        *viewport,
                    );
                }
            }
        }
    }
}

impl<'a> From<TextGrid<'a>> for Element<'a, crate::Message> {
    fn from(grid: TextGrid<'a>) -> Self {
        Element::new(grid)
    }
}
