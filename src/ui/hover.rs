/// Extract displayable text from an LSP hover result.
pub fn extract_hover_text(value: &serde_json::Value) -> Vec<String> {
    // The hover result has a `contents` field which can be:
    // - MarkedString (string or { language, value })
    // - MarkedString[]
    // - MarkupContent { kind, value }
    let contents = match value.get("contents") {
        Some(c) => c,
        None => {
            // Just dump the raw JSON so user can see what we got
            return serde_json::to_string_pretty(value)
                .unwrap_or_default()
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
    serde_json::to_string_pretty(value)
        .unwrap_or_default()
        .lines()
        .map(|l| l.to_string())
        .collect()
}

/// Strip ```lang\n...\n``` fences so we display just the code.
pub fn strip_markdown_fences(text: &str) -> Vec<String> {
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
