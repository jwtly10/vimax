use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length, Theme};

use crate::app::Message;
use crate::text_grid;

const MAX_WIDTH: f32 = 520.0;
const MAX_LINES: usize = 20;
const LINE_HEIGHT: f32 = 20.0;
const PADDING: f32 = 8.0;

#[derive(Debug, Clone)]
pub enum HoverContent {
    /// Raw LSP hover result — just dump the JSON for now so we can inspect it
    LspHover(serde_json::Value),
    /// Diagnostic message(s) under cursor
    Diagnostic(Vec<DiagnosticEntry>),
}

#[derive(Debug, Clone)]
pub struct DiagnosticEntry {
    pub severity: crate::diagnostics::Severity,
    pub message: String,
    pub source: Option<String>,
}

pub struct HoverPopup {
    pub content: HoverContent,
    /// Cursor position (line, col) at the time the hover was triggered
    pub cursor_line: usize,
    pub cursor_col: usize,
    /// Window scroll offsets for positioning
    pub scroll_y: usize,
    pub scroll_x: usize,
}

impl HoverPopup {
    pub fn view(&self) -> Element<'_, Message> {
        let inner: Element<'_, Message> = match &self.content {
            HoverContent::LspHover(value) => self.render_lsp_hover(value),
            HoverContent::Diagnostic(entries) => self.render_diagnostics(entries),
        };

        // Position relative to cursor
        let pixel_x = text_grid::GUTTER_WIDTH
            + 8.0
            + (self.cursor_col as isize - self.scroll_x as isize).max(0) as f32
                * text_grid::CHAR_WIDTH;
        let pixel_y = ((self.cursor_line as isize - self.scroll_y as isize + 1).max(0) as f32)
            * text_grid::LINE_HEIGHT;

        container(column![
            Space::new().height(Length::Fixed(pixel_y)),
            row![Space::new().width(Length::Fixed(pixel_x)), inner]
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn render_lsp_hover<'a>(&self, value: &serde_json::Value) -> Element<'a, Message> {
        let lines = extract_hover_text(value);
        let visible_count = lines.len().min(MAX_LINES);
        let has_more = lines.len() > MAX_LINES;

        let mut content = column![].spacing(0);
        for line in lines.into_iter().take(MAX_LINES) {
            content = content.push(
                text(line)
                    .size(13)
                    .color(iced::Color::from_rgb(0.85, 0.85, 0.9)),
            );
        }
        if has_more {
            content = content.push(
                text(format!("  ... (truncated)"))
                    .size(12)
                    .color(iced::Color::from_rgb(0.5, 0.5, 0.6)),
            );
        }

        let row_count = visible_count + if has_more { 1 } else { 0 };
        let height = row_count as f32 * LINE_HEIGHT + PADDING * 2.0;

        Self::popup_container(content, height)
    }

    fn render_diagnostics<'a>(&self, entries: &[DiagnosticEntry]) -> Element<'a, Message> {
        let mut content = column![].spacing(4);
        let mut total_lines = 0usize;

        for entry in entries {
            let (icon, color) = match entry.severity {
                crate::diagnostics::Severity::Error => {
                    ("E", iced::Color::from_rgb(1.0, 0.35, 0.35))
                }
                crate::diagnostics::Severity::Warning => {
                    ("W", iced::Color::from_rgb(1.0, 0.8, 0.25))
                }
                crate::diagnostics::Severity::Info => {
                    ("I", iced::Color::from_rgb(0.4, 0.7, 1.0))
                }
                crate::diagnostics::Severity::Hint => {
                    ("H", iced::Color::from_rgb(0.5, 0.8, 0.5))
                }
            };

            let severity_badge = text(format!(" {} ", icon)).size(12).color(color);

            let msg_lines: Vec<String> = entry.message.lines().map(|l| l.to_string()).collect();
            let mut msg_col = column![].spacing(0);
            for (i, line) in msg_lines.into_iter().take(MAX_LINES).enumerate() {
                let t = if i == 0 {
                    text(line)
                        .size(13)
                        .color(iced::Color::from_rgb(0.85, 0.85, 0.9))
                } else {
                    text(format!("  {}", line))
                        .size(13)
                        .color(iced::Color::from_rgb(0.75, 0.75, 0.8))
                };
                msg_col = msg_col.push(t);
                total_lines += 1;
            }

            if let Some(src) = &entry.source {
                msg_col = msg_col.push(
                    text(format!("[{}]", src))
                        .size(11)
                        .color(iced::Color::from_rgb(0.5, 0.5, 0.6)),
                );
                total_lines += 1;
            }

            content = content.push(row![severity_badge, msg_col].spacing(4));
        }

        let height =
            total_lines as f32 * LINE_HEIGHT + (entries.len() as f32 * 4.0) + PADDING * 2.0;

        Self::popup_container(content, height)
    }

    fn popup_container<'a>(
        content: iced::widget::Column<'a, Message>,
        height: f32,
    ) -> Element<'a, Message> {
        let border_color = iced::Color::from_rgb(0.3, 0.35, 0.45);
        container(content)
            .width(Length::Shrink)
            .max_width(MAX_WIDTH)
            .height(Length::Fixed(height.min(MAX_LINES as f32 * LINE_HEIGHT + PADDING * 2.0)))
            .padding(PADDING)
            .style(move |_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.1, 0.1, 0.14,
                ))),
                border: iced::Border {
                    width: 1.0,
                    color: border_color,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    }
}

/// Extract displayable text from an LSP hover result.
fn extract_hover_text(value: &serde_json::Value) -> Vec<String> {
    // The hover result has a `contents` field which can be:
    // - MarkedString (string or { language, value })
    // - MarkedString[]
    // - MarkupContent { kind, value }
    let contents = match value.get("contents") {
        Some(c) => c,
        None => {
            // Just dump the raw JSON so user can see what we got
            return format!("{}", serde_json::to_string_pretty(value).unwrap_or_default())
                .lines()
                .map(|l| l.to_string())
                .collect();
        }
    };

    if let Some(s) = contents.as_str() {
        return s.lines().map(|l| l.to_string()).collect();
    }

    // MarkupContent { kind, value }
    if let Some(val) = contents.get("value").and_then(|v| v.as_str()) {
        return strip_markdown_fences(val);
    }

    // Array of MarkedString
    if let Some(arr) = contents.as_array() {
        let mut lines = Vec::new();
        for item in arr {
            if let Some(s) = item.as_str() {
                lines.extend(s.lines().map(|l| l.to_string()));
            } else if let Some(val) = item.get("value").and_then(|v| v.as_str()) {
                lines.extend(strip_markdown_fences(val));
            }
        }
        return lines;
    }

    // Fallback: dump raw
    format!("{}", serde_json::to_string_pretty(value).unwrap_or_default())
        .lines()
        .map(|l| l.to_string())
        .collect()
}

/// Strip ```lang\n...\n``` fences so we display just the code.
fn strip_markdown_fences(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        lines.push(line.to_string());
    }
    lines
}
