use iced::advanced::layout;
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::mouse;
use iced::{Color, Element, Event, Length, Rectangle, Size};

pub const SCROLLBAR_WIDTH: f32 = 14.0;
const MARKER_HEIGHT: f32 = 2.0;
const MIN_THUMB_HEIGHT: f32 = 20.0;

pub struct ScrollbarMarker {
    pub line: usize,
    pub color: Color,
}

pub struct Scrollbar {
    scroll_y: usize,
    visible_lines: usize,
    total_lines: usize,
    markers: Vec<ScrollbarMarker>,
    window_id: usize,
    is_active: bool,
}

#[derive(Debug, Default)]
struct ScrollbarState {
    dragging: bool,
    drag_offset: f32,
}

pub fn scrollbar(
    scroll_y: usize,
    visible_lines: usize,
    total_lines: usize,
    markers: Vec<ScrollbarMarker>,
    window_id: usize,
    is_active: bool,
) -> Element<'static, crate::app::Message> {
    Element::new(Scrollbar {
        scroll_y,
        visible_lines,
        total_lines,
        markers,
        window_id,
        is_active,
    })
}

impl Scrollbar {
    fn thumb_geometry(&self, track_height: f32) -> (f32, f32) {
        if self.total_lines == 0 {
            return (0.0, track_height);
        }
        let ratio = (self.visible_lines as f32 / self.total_lines as f32).min(1.0);
        let thumb_height = (ratio * track_height)
            .max(MIN_THUMB_HEIGHT)
            .min(track_height);
        let scrollable = self.total_lines.saturating_sub(self.visible_lines);
        let thumb_y = if scrollable > 0 {
            (self.scroll_y as f32 / scrollable as f32) * (track_height - thumb_height)
        } else {
            0.0
        };
        (thumb_y, thumb_height)
    }

    fn line_from_y(&self, y: f32, track_height: f32) -> usize {
        if track_height <= 0.0 || self.total_lines == 0 {
            return 0;
        }
        let ratio = (y / track_height).clamp(0.0, 1.0);
        let line = (ratio * self.total_lines as f32) as usize;
        line.min(self.total_lines.saturating_sub(1))
    }

    fn scroll_y_from_thumb_y(&self, thumb_y: f32, track_height: f32) -> usize {
        let (_, thumb_height) = self.thumb_geometry(track_height);
        let max_thumb_y = track_height - thumb_height;
        if max_thumb_y <= 0.0 {
            return 0;
        }
        let ratio = (thumb_y / max_thumb_y).clamp(0.0, 1.0);
        let scrollable = self.total_lines.saturating_sub(self.visible_lines);
        (ratio * scrollable as f32) as usize
    }
}

impl Widget<crate::app::Message, iced::Theme, iced::Renderer> for Scrollbar {
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(SCROLLBAR_WIDTH),
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(Length::Fixed(SCROLLBAR_WIDTH));
        let size = limits.resolve(Length::Fixed(SCROLLBAR_WIDTH), Length::Fill, Size::ZERO);
        layout::Node::new(size)
    }

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<ScrollbarState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(ScrollbarState::default())
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
        let state = tree.state.downcast_mut::<ScrollbarState>();
        let wid = self.window_id;

        if state.dragging {
            match event {
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    if let Some(pos) = cursor.position() {
                        let relative_y = pos.y - bounds.y;
                        let new_thumb_y = relative_y - state.drag_offset;
                        let scroll_y = self.scroll_y_from_thumb_y(new_thumb_y, bounds.height);
                        shell.publish(crate::app::Message::ScrollbarDrag {
                            scroll_y,
                            window_id: wid,
                        });
                        shell.capture_event();
                    }
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    state.dragging = false;
                    shell.capture_event();
                }
                _ => {}
            }
            return;
        }

        if shell.is_event_captured() {
            return;
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let track_height = bounds.height;
                    let (thumb_y, thumb_height) = self.thumb_geometry(track_height);

                    if pos.y >= thumb_y && pos.y <= thumb_y + thumb_height {
                        // Thumb drag
                        state.dragging = true;
                        state.drag_offset = pos.y - thumb_y;
                    } else {
                        // Jump to pos
                        let line = self.line_from_y(pos.y, track_height);
                        shell.publish(crate::app::Message::ScrollbarJump {
                            line,
                            window_id: wid,
                        });
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let lines = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => *y,
                        mouse::ScrollDelta::Pixels { y, .. } => *y / crate::text_grid::LINE_HEIGHT,
                    };
                    shell.publish(crate::app::Message::ScrollLines {
                        delta: lines,
                        window_id: wid,
                    });
                    shell.capture_event();
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
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        let track_alpha = if self.is_active { 0.5 } else { 0.3 };
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            Color::from_rgba(0.15, 0.15, 0.2, track_alpha),
        );

        let track_height = bounds.height;

        if self.total_lines > 0 {
            for marker in &self.markers {
                let y_ratio = marker.line as f32 / self.total_lines as f32;
                let marker_y = bounds.y + y_ratio * track_height;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x,
                            y: marker_y,
                            width: SCROLLBAR_WIDTH,
                            height: MARKER_HEIGHT,
                        },
                        ..renderer::Quad::default()
                    },
                    marker.color,
                );
            }
        }

        let (thumb_y, thumb_height) = self.thumb_geometry(track_height);
        let thumb_alpha = if self.is_active { 0.5 } else { 0.3 };
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x + 2.0,
                    y: bounds.y + thumb_y,
                    width: SCROLLBAR_WIDTH - 4.0,
                    height: thumb_height,
                },
                border: iced::border::rounded(3),
                ..renderer::Quad::default()
            },
            Color::from_rgba(0.6, 0.6, 0.6, thumb_alpha),
        );
    }
}

impl From<Scrollbar> for Element<'static, crate::app::Message> {
    fn from(sb: Scrollbar) -> Self {
        Element::new(sb)
    }
}
