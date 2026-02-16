use crate::buffer::Buffer;
use crate::syntax::highlight::HighlightSpan;
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
    cursor: usize,
    scroll_y: usize,
    scroll_x: usize,
    selection: Option<(usize, usize)>,
    search_matches: &'a [usize],
    search_len: usize,
    window_id: usize,
    is_active: bool,
    highlights: Vec<HighlightSpan>,
}

#[allow(clippy::too_many_arguments)]
pub fn text_grid<'a>(
    buffer: &'a Buffer,
    cursor: usize,
    scroll_y: usize,
    scroll_x: usize,
    selection: Option<(usize, usize)>,
    search_matches: &'a [usize],
    search_len: usize,
    window_id: usize,
    is_active: bool,
    highlights: Vec<HighlightSpan>,
) -> Element<'a, crate::app::Message> {
    Element::new(TextGrid {
        buffer,
        cursor,
        scroll_y,
        scroll_x,
        selection,
        search_matches,
        search_len,
        window_id,
        is_active,
        highlights,
    })
}

#[derive(Debug, Default)]
struct TextGridState {
    last_visible_lines: usize,
    last_visible_cols: usize,
}

impl<'a> Widget<crate::app::Message, iced::Theme, iced::Renderer> for TextGrid<'a> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<TextGridState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(TextGridState::default())
    }

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
        tree: &mut widget::Tree,
        event: &Event,
        layout: iced::advanced::Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, crate::app::Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let wid = self.window_id;

        let visible_lines = (bounds.height / LINE_HEIGHT) as usize;
        let text_area_width = bounds.width - GUTTER_WIDTH - 8.0;
        let visible_cols = (text_area_width / CHAR_WIDTH).max(1.0) as usize;

        let state = tree.state.downcast_mut::<TextGridState>();
        if state.last_visible_lines != visible_lines || state.last_visible_cols != visible_cols {
            state.last_visible_lines = visible_lines;
            state.last_visible_cols = visible_cols;
            shell.publish(crate::app::Message::ViewportResized {
                lines: visible_lines,
                cols: visible_cols,
                window_id: wid,
            });
        }

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
                        shell.publish(crate::app::Message::ScrollCols {
                            delta: cols,
                            window_id: wid,
                        });
                    }
                    shell.publish(crate::app::Message::ScrollLines {
                        delta: lines,
                        window_id: wid,
                    });
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    shell.publish(crate::app::Message::MouseClick {
                        x: pos.x,
                        y: pos.y,
                        window_id: wid,
                    });
                }
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

        // Clip all rendering to this widget's bounds so text doesn't bleed into adjacent splits
        renderer.with_layer(bounds, |renderer| {
            let rope = self.buffer.rope();
            let (cursor_line, cursor_col) = self.buffer.cursor_position(self.cursor);

            let visible_lines = (bounds.height / LINE_HEIGHT) as usize;
            let total_lines = rope.len_lines();

            let scroll_y = self.scroll_y;
            let scroll_x = self.scroll_x;

            let end_line = (scroll_y + visible_lines + 1).min(total_lines);

            for (i, line_idx) in (scroll_y..end_line).enumerate() {
                let y = bounds.y + (i as f32 * LINE_HEIGHT);

                if y + LINE_HEIGHT > bounds.y + bounds.height {
                    break;
                }

                if line_idx == cursor_line && self.is_active {
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

                let line_num = format!("{:>width$}", line_idx + 1, width = GUTTER_CHARS);
                let line_num_color = if line_idx == cursor_line && self.is_active {
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

                if let Some((sel_start, sel_end)) = self.selection {
                    let line_start_char = rope.line_to_char(line_idx);
                    let line_end_char = if line_idx + 1 < total_lines {
                        rope.line_to_char(line_idx + 1)
                    } else {
                        rope.len_chars()
                    };

                    if sel_start < line_end_char && sel_end > line_start_char {
                        let sel_col_start = sel_start.saturating_sub(line_start_char);
                        let sel_col_end = if sel_end < line_end_char {
                            sel_end - line_start_char
                        } else {
                            rope.line(line_idx).len_chars()
                        };

                        if sel_col_end > scroll_x
                            && sel_col_start < scroll_x + (bounds.width / CHAR_WIDTH) as usize
                        {
                            let draw_start = sel_col_start.saturating_sub(scroll_x);
                            let draw_end = sel_col_end.saturating_sub(scroll_x);
                            let text_x = bounds.x + GUTTER_WIDTH + 8.0;
                            let sel_x = text_x + (draw_start as f32 * CHAR_WIDTH);
                            let sel_w = ((draw_end - draw_start) as f32 * CHAR_WIDTH)
                                .min(bounds.width - (sel_x - bounds.x));

                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle {
                                        x: sel_x,
                                        y,
                                        width: sel_w,
                                        height: LINE_HEIGHT,
                                    },
                                    ..renderer::Quad::default()
                                },
                                Color::from_rgba(0.3, 0.5, 0.8, 0.4),
                            );
                        }
                    }
                }

                if self.search_len > 0 {
                    let line_start_char = rope.line_to_char(line_idx);
                    let line_end_char = if line_idx + 1 < total_lines {
                        rope.line_to_char(line_idx + 1)
                    } else {
                        rope.len_chars()
                    };

                    for &match_pos in self.search_matches {
                        let match_end = match_pos + self.search_len;
                        if match_pos < line_end_char && match_end > line_start_char {
                            let col_start = match_pos.saturating_sub(line_start_char);
                            let col_end = if match_end < line_end_char {
                                match_end - line_start_char
                            } else {
                                line_end_char - line_start_char
                            };

                            if col_end > scroll_x {
                                let draw_start = col_start.saturating_sub(scroll_x);
                                let draw_end = col_end.saturating_sub(scroll_x);
                                let text_x = bounds.x + GUTTER_WIDTH + 8.0;
                                let hl_x = text_x + (draw_start as f32 * CHAR_WIDTH);
                                let hl_w = (draw_end - draw_start) as f32 * CHAR_WIDTH;

                                renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: Rectangle {
                                            x: hl_x,
                                            y,
                                            width: hl_w,
                                            height: LINE_HEIGHT,
                                        },
                                        ..renderer::Quad::default()
                                    },
                                    Color::from_rgba(0.9, 0.7, 0.2, 0.3),
                                );
                            }
                        }
                    }
                }

                let line = rope.line(line_idx);
                let line_str: String = line.chars().collect();
                let display_str = line_str.trim_end_matches('\n').trim_end_matches('\r');

                let text_x = bounds.x + GUTTER_WIDTH + 8.0;
                let text_area_width = bounds.width - GUTTER_WIDTH - 8.0;
                let default_color = Color::from_rgb(0.85, 0.85, 0.85);

                // Find highlight spans that overlap this line
                let line_byte_start = rope.line_to_byte(line_idx) as u32;
                let line_byte_end = if line_idx + 1 < total_lines {
                    rope.line_to_byte(line_idx + 1) as u32
                } else {
                    rope.len_bytes() as u32
                };

                // Collect spans for this line using binary search
                let first = self
                    .highlights
                    .partition_point(|s| s.byte_end <= line_byte_start);
                let last = self
                    .highlights
                    .partition_point(|s| s.byte_start < line_byte_end);
                let line_spans = &self.highlights[first..last];

                if line_spans.is_empty() {
                    // No syntax — single fill_text in default color
                    let scrolled_str: String = display_str.chars().skip(scroll_x).collect();
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
                        default_color,
                        *viewport,
                    );
                } else {
                    // Build colored segments for the visible portion of this line
                    // Convert byte offsets to char columns relative to line start
                    let line_char_start = rope.line_to_char(line_idx);
                    let display_len = display_str.chars().count();

                    // Build a color array for each char in the line
                    // Start with default, then paint spans over it
                    let mut char_colors: Vec<Color> = vec![default_color; display_len];

                    for span in line_spans {
                        let span_start = span.byte_start.max(line_byte_start);
                        let span_end = span.byte_end.min(line_byte_end);
                        if span_start >= span_end {
                            continue;
                        }
                        let col_start = rope.byte_to_char(span_start as usize) - line_char_start;
                        let col_end = rope.byte_to_char(span_end as usize) - line_char_start;
                        let col_start = col_start.min(display_len);
                        let col_end = col_end.min(display_len);
                        for col in char_colors.iter_mut().take(col_end).skip(col_start) {
                            *col = span.color;
                        }
                    }

                    // Now render runs of same-colored characters
                    let visible_start = scroll_x.min(display_len);
                    let chars: Vec<char> = display_str.chars().collect();

                    if visible_start < display_len {
                        let mut run_start = visible_start;
                        while run_start < display_len {
                            let run_color = char_colors[run_start];
                            let mut run_end = run_start + 1;
                            while run_end < display_len && char_colors[run_end] == run_color {
                                run_end += 1;
                            }

                            let run_text: String = chars[run_start..run_end].iter().collect();
                            let x_offset = (run_start - scroll_x) as f32 * CHAR_WIDTH;

                            renderer.fill_text(
                                iced_text::Text {
                                    content: run_text,
                                    bounds: Size::new(text_area_width - x_offset, LINE_HEIGHT),
                                    size: FONT_SIZE.into(),
                                    line_height: iced_text::LineHeight::Absolute(
                                        LINE_HEIGHT.into(),
                                    ),
                                    font: iced::Font::MONOSPACE,
                                    align_x: iced::Alignment::Start.into(),
                                    align_y: alignment::Vertical::Top,
                                    shaping: iced_text::Shaping::Basic,
                                    wrapping: iced_text::Wrapping::None,
                                },
                                iced::Point::new(text_x + x_offset, y),
                                run_color,
                                *viewport,
                            );

                            run_start = run_end;
                        }
                    }
                }

                if self.is_active && line_idx == cursor_line && cursor_col >= scroll_x {
                    let cursor_x = text_x + ((cursor_col - scroll_x) as f32 * CHAR_WIDTH);

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
        }); // end with_layer clip
    }
}

impl<'a> From<TextGrid<'a>> for Element<'a, crate::app::Message> {
    fn from(grid: TextGrid<'a>) -> Self {
        Element::new(grid)
    }
}
