use crate::buffer::Buffer;

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub severity: Severity,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// Per-buffer diagnostic storage. Indexed by buffer ID
pub struct DiagnosticStore {
    entries: Vec<Vec<Diagnostic>>,
}

impl DiagnosticStore {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn ensure_capacity(&mut self, buf_id: usize) {
        if self.entries.len() <= buf_id {
            self.entries.resize_with(buf_id + 1, Vec::new);
        }
    }

    pub fn set_for_buffer(&mut self, buf_id: usize, mut diags: Vec<Diagnostic>) {
        self.ensure_capacity(buf_id);
        diags.sort_by_key(|d| (d.line, d.col_start));
        self.entries[buf_id] = diags;
    }

    pub fn get_for_buffer(&self, buf_id: usize) -> &[Diagnostic] {
        self.entries
            .get(buf_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn counts_for_buffer(&self, buf_id: usize) -> (usize, usize) {
        let diags = self.get_for_buffer(buf_id);
        let errors = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let warnings = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();
        (errors, warnings)
    }

    pub fn for_line_range(
        &self,
        buf_id: usize,
        start_line: usize,
        end_line: usize,
    ) -> &[Diagnostic] {
        let diags = self.get_for_buffer(buf_id);
        if diags.is_empty() {
            return &[];
        }
        let first = diags.partition_point(|d| d.line < start_line);
        let last = diags.partition_point(|d| d.line <= end_line);
        &diags[first..last]
    }

    /// Remove diagnostics when a buffer is deleted, fix indices.
    pub fn remove_buffer(&mut self, buf_id: usize) {
        if buf_id < self.entries.len() {
            self.entries.remove(buf_id);
        }
    }
}

/// Parse a `textDocument/publishDiagnostics` notification params.
/// Returns (buffer_id, diagnostics) if the URI matches a known buffer.
pub fn parse_publish_diagnostics(
    params: &serde_json::Value,
    buffers: &[Buffer],
) -> Option<(usize, Vec<Diagnostic>)> {
    let uri_str = params.get("uri")?.as_str()?;

    let path = uri_str.strip_prefix("file://")?;

    let buf_id = buffers.iter().position(|b| {
        b.file_path()
            .map(|fp| fp == path || std::path::Path::new(fp) == std::path::Path::new(path))
            .unwrap_or(false)
    })?;

    let diag_array = params.get("diagnostics")?.as_array()?;
    let buffer = &buffers[buf_id];

    let diags = diag_array
        .iter()
        .filter_map(|d| parse_one_diagnostic(d, buffer))
        .collect();

    Some((buf_id, diags))
}

fn parse_one_diagnostic(value: &serde_json::Value, buffer: &Buffer) -> Option<Diagnostic> {
    let range = value.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;

    let start_line = start.get("line")?.as_u64()? as usize;
    let start_char = start.get("character")?.as_u64()? as usize;
    let end_line = end.get("line")?.as_u64()? as usize;
    let end_char = end.get("character")?.as_u64()? as usize;

    let col_start = lsp_col_to_char_col(buffer, start_line, start_char);
    let col_end = if end_line == start_line {
        lsp_col_to_char_col(buffer, end_line, end_char)
    } else {
        // Multi-line diagnostic: underline to end of start line
        let rope = buffer.rope();
        if start_line < rope.len_lines() {
            rope.line(start_line).len_chars().saturating_sub(1)
        } else {
            col_start + 1
        }
    };

    // Ensure at least 1 char wide
    let col_end = col_end.max(col_start + 1);

    let severity_num = value.get("severity").and_then(|s| s.as_u64()).unwrap_or(1);

    let severity = match severity_num {
        1 => Severity::Error,
        2 => Severity::Warning,
        3 => Severity::Info,
        4 => Severity::Hint,
        _ => Severity::Error,
    };

    let message = value.get("message")?.as_str()?.to_string();
    let source = value
        .get("source")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());

    Some(Diagnostic {
        line: start_line,
        col_start,
        col_end,
        severity,
        message,
        source,
    })
}

/// Convert LSP UTF-16 column offset to char column for a given line.
fn lsp_col_to_char_col(buffer: &Buffer, line: usize, utf16_col: usize) -> usize {
    let rope = buffer.rope();
    if line >= rope.len_lines() {
        return 0;
    }
    let line_text = rope.line(line);
    let mut utf16_offset = 0;
    let mut char_col = 0;
    for ch in line_text.chars() {
        if utf16_offset >= utf16_col {
            break;
        }
        utf16_offset += ch.len_utf16();
        char_col += 1;
    }
    char_col
}
