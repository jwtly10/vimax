use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

use ropey::Rope;

use crate::action::EditorAction;
use crate::buffer::Buffer;
use crate::picker::{DetailSpan, Picker, PickerItem};
use crate::syntax::SyntaxState;
use crate::syntax::loader::Loader;

/// Build a picker for switching between open buffers.
pub fn buffer_picker(buffers: &[Buffer], cwd: &Path, restore_buffer: Option<usize>) -> Picker {
    let items: Vec<PickerItem> = buffers
        .iter()
        .enumerate()
        .map(|(id, buf)| {
            let modified = if buf.is_modified() { " [+]" } else { "" };
            let name = format!("{}{}", buf.name(), modified);
            let detail = buf.file_path().map(|p| {
                Path::new(p)
                    .strip_prefix(cwd)
                    .unwrap_or(Path::new(p))
                    .display()
                    .to_string()
            });
            PickerItem {
                match_text: name.clone(),
                display: name,
                detail,
                detail_spans: vec![],
                group: None,
                action: EditorAction::SwitchToBuffer(id),
                preview_action: Some(EditorAction::SwitchToBuffer(id)),
            }
        })
        .collect();
    Picker::new("Buffers", items, restore_buffer)
}

/// Build a picker for project files using the ignore crate
pub fn file_picker(
    cwd: &Path,
    show_ignored: bool,
    max_results: usize,
    restore_buffer: Option<usize>,
) -> Picker {
    let items: Vec<PickerItem> = ignore::WalkBuilder::new(cwd)
        .git_ignore(!show_ignored)
        .git_exclude(!show_ignored)
        .filter_entry(|entry| {
            let custom_ignores = [".git", "target", "node_modules", "dist", "build"];
            let file_name = entry.file_name().to_string_lossy();
            !custom_ignores.contains(&file_name.as_ref())
        })
        .build()
        .filter_map(|entry| entry.ok())
        .take(max_results)
        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|entry| {
            let path = entry.into_path();
            let rel = path.strip_prefix(cwd).unwrap_or(&path).to_path_buf();
            let file_name = rel
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let dir = rel
                .parent()
                .map(|p| p.display().to_string())
                .filter(|s| !s.is_empty());
            PickerItem {
                match_text: rel.display().to_string(),
                display: file_name,
                detail: dir,
                detail_spans: vec![],
                group: None,
                action: EditorAction::OpenFile(path),
                preview_action: None,
            }
        })
        .collect();
    Picker::new("Project Files", items, restore_buffer)
}

/// Build a picker from a list of LSP locations (goto definition, references, etc).
pub fn lsp_location_picker(
    locations: &[lsp_types::Location],
    label: &str,
    cwd: &Path,
    buffers: &[Buffer],
    syntax_states: &[Option<SyntaxState>],
    loader: &Loader,
    restore_buffer: Option<usize>,
) -> Picker {
    let mut file_cache: HashMap<String, Option<(Rope, Option<SyntaxState>)>> = HashMap::new();

    let items: Vec<PickerItem> = locations
        .iter()
        .map(|loc| {
            let path = PathBuf::from(loc.uri.path().to_string());
            let display_path = path
                .strip_prefix(cwd)
                .unwrap_or(&path)
                .display()
                .to_string();
            let line = loc.range.start.line as usize;
            let col = loc.range.start.character as usize;
            let path_str = path.to_string_lossy().to_string();

            let (detail, detail_spans) = buffers
                .iter()
                .enumerate()
                .find(|(_, b)| b.file_path() == Some(&path_str))
                .and_then(|(buf_id, b)| {
                    highlighted_line(
                        b.rope(),
                        syntax_states.get(buf_id).and_then(|s| s.as_ref()),
                        loader,
                        line,
                    )
                })
                .or_else(|| {
                    let cached = file_cache
                        .entry(path_str.clone())
                        .or_insert_with(|| load_file_for_preview(&path, loader));
                    if let Some((rope, syntax)) = cached.as_ref() {
                        highlighted_line(rope, syntax.as_ref(), loader, line)
                    } else {
                        None
                    }
                })
                .unwrap_or((None, vec![]));

            PickerItem {
                match_text: format!("{}:{}:{}", display_path, line + 1, col + 1),
                display: format!("{}:{}", line + 1, col + 1),
                detail,
                detail_spans,
                group: Some(display_path),
                action: EditorAction::OpenFileAtPosition {
                    path: path.clone(),
                    line,
                    col,
                },
                preview_action: None,
            }
        })
        .collect();
    Picker::new(label, items, restore_buffer)
}

/// Parse LSP response JSON into a normalized list of Locations.
/// Handles single Location, arrays of Location, and LocationLink responses.
pub fn parse_lsp_locations(result: &serde_json::Value) -> Vec<lsp_types::Location> {
    if result.is_null() {
        return Vec::new();
    }
    if result.get("uri").is_some()
        && let Ok(loc) = serde_json::from_value::<lsp_types::Location>(result.clone())
    {
        return vec![loc];
    }
    if let Some(arr) = result.as_array() {
        if arr.is_empty() {
            return Vec::new();
        }
        if arr[0].get("targetUri").is_some() {
            return arr
                .iter()
                .filter_map(|v| serde_json::from_value::<lsp_types::LocationLink>(v.clone()).ok())
                .map(|l| lsp_types::Location {
                    uri: l.target_uri,
                    range: l.target_selection_range,
                })
                .collect();
        }
        return arr
            .iter()
            .filter_map(|v| serde_json::from_value::<lsp_types::Location>(v.clone()).ok())
            .collect();
    }
    Vec::new()
}

fn load_file_for_preview(path: &Path, loader: &Loader) -> Option<(Rope, Option<SyntaxState>)> {
    let content = read_to_string(path).ok()?;
    let rope = Rope::from_str(&content);
    let ext = path.extension().and_then(|e| e.to_str());
    let syntax = ext
        .and_then(|ext| loader.language_for_extension(ext))
        .and_then(|lang| SyntaxState::new(&rope, lang, loader));
    Some((rope, syntax))
}

fn highlighted_line(
    rope: &Rope,
    syntax: Option<&SyntaxState>,
    loader: &Loader,
    line: usize,
) -> Option<(Option<String>, Vec<DetailSpan>)> {
    if line >= rope.len_lines() {
        return None;
    }

    let line_text: String = rope.line(line).chars().collect();
    let trimmed = line_text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let detail = if trimmed.len() > 120 {
        format!("{}…", &trimmed[..119])
    } else {
        trimmed.to_string()
    };

    let syntax = match syntax {
        Some(s) => s,
        None => {
            let default_color = iced::Color::from_rgb(0.65, 0.65, 0.7);
            return Some((
                Some(detail.clone()),
                vec![DetailSpan {
                    text: detail,
                    color: default_color,
                }],
            ));
        }
    };

    let leading_ws = line_text.len() - line_text.trim_start().len();
    let line_byte_start = rope.line_to_byte(line);
    let line_byte_end = if line + 1 < rope.len_lines() {
        rope.line_to_byte(line + 1)
    } else {
        rope.len_bytes()
    };

    let highlights =
        syntax.highlights_for_range(rope, loader, line_byte_start as u32, line_byte_end as u32);

    let default_color = iced::Color::from_rgb(0.75, 0.75, 0.8);

    if highlights.is_empty() {
        return Some((
            Some(detail.clone()),
            vec![DetailSpan {
                text: detail,
                color: iced::Color::from_rgb(0.65, 0.65, 0.7),
            }],
        ));
    }

    let trim_byte_start = line_byte_start + leading_ws;
    let trim_byte_end =
        (line_byte_start + line_text.trim_end_matches('\n').len()).min(line_byte_end);

    if trim_byte_start >= trim_byte_end {
        return Some((
            Some(detail.clone()),
            vec![DetailSpan {
                text: detail,
                color: default_color,
            }],
        ));
    }

    let mut spans = Vec::new();
    let mut pos = trim_byte_start;

    for hl in &highlights {
        let span_start = (hl.byte_start as usize).max(trim_byte_start);
        let span_end = (hl.byte_end as usize).min(trim_byte_end);
        if span_start >= span_end {
            continue;
        }
        if pos < span_start {
            let chunk = rope_byte_slice(rope, pos, span_start);
            if !chunk.is_empty() {
                spans.push(DetailSpan {
                    text: chunk,
                    color: default_color,
                });
            }
        }
        let chunk = rope_byte_slice(rope, span_start, span_end);
        if !chunk.is_empty() {
            spans.push(DetailSpan {
                text: chunk,
                color: hl.color,
            });
        }
        pos = span_end;
    }

    if pos < trim_byte_end {
        let chunk = rope_byte_slice(rope, pos, trim_byte_end);
        if !chunk.is_empty() {
            spans.push(DetailSpan {
                text: chunk,
                color: default_color,
            });
        }
    }

    if spans.is_empty() {
        spans.push(DetailSpan {
            text: detail.clone(),
            color: default_color,
        });
    }

    Some((Some(detail), spans))
}

fn rope_byte_slice(rope: &Rope, start: usize, end: usize) -> String {
    let char_start = rope.byte_to_char(start);
    let char_end = rope.byte_to_char(end.min(rope.len_bytes()));
    rope.slice(char_start..char_end).chars().collect()
}
