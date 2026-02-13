use iced::Color;
use tree_house::highlighter::Highlight;

#[derive(Debug, Clone, Copy)]
pub enum HighlightGroup {
    Keyword,
    Function,
    Type,
    String,
    Comment,
    Number,
    Operator,
    Variable,
    Property,
    Punctuation,
    Attribute,
    Namespace,
    Constant,
    Escape,
}

pub fn capture_name_to_highlight(name: &str) -> Option<Highlight> {
    let mut name = name;
    loop {
        let result = match name {
            "keyword" => Some(Highlight::new(0)),
            "function" => Some(Highlight::new(1)),
            "type" => Some(Highlight::new(2)),
            "string" => Some(Highlight::new(3)),
            "comment" => Some(Highlight::new(4)),
            "number" | "constant.numeric" | "float" => Some(Highlight::new(5)),
            "operator" => Some(Highlight::new(6)),
            "variable" => Some(Highlight::new(7)),
            "property" | "field" => Some(Highlight::new(8)),
            "punctuation" => Some(Highlight::new(9)),
            "attribute" => Some(Highlight::new(10)),
            "namespace" | "module" => Some(Highlight::new(11)),
            "constant" | "boolean" => Some(Highlight::new(12)),
            "escape" | "character" => Some(Highlight::new(13)),
            "label" => Some(Highlight::new(7)), // reusing variable color
            "constructor" => Some(Highlight::new(2)), // reusing type color
            "tag" => Some(Highlight::new(1)),   // reusing function color
            _ => None,
        };
        if result.is_some() {
            return result;
        }
        match name.rsplit_once('.') {
            Some((parent, _)) => name = parent,
            None => return None,
        }
    }
}

pub fn highlight_to_group(h: Highlight) -> HighlightGroup {
    match h.idx() {
        0 => HighlightGroup::Keyword,
        1 => HighlightGroup::Function,
        2 => HighlightGroup::Type,
        3 => HighlightGroup::String,
        4 => HighlightGroup::Comment,
        5 => HighlightGroup::Number,
        6 => HighlightGroup::Operator,
        7 => HighlightGroup::Variable,
        8 => HighlightGroup::Property,
        9 => HighlightGroup::Punctuation,
        10 => HighlightGroup::Attribute,
        11 => HighlightGroup::Namespace,
        12 => HighlightGroup::Constant,
        13 => HighlightGroup::Escape,
        _ => HighlightGroup::Variable,
    }
}

/// One Dark style
pub fn color_for_group(group: HighlightGroup) -> Color {
    match group {
        HighlightGroup::Keyword => Color::from_rgb8(198, 120, 221), // purple
        HighlightGroup::Function => Color::from_rgb8(97, 175, 239), // blue
        HighlightGroup::Type => Color::from_rgb8(229, 192, 123),    // yellow
        HighlightGroup::String => Color::from_rgb8(152, 195, 121),  // green
        HighlightGroup::Comment => Color::from_rgb8(92, 99, 112),   // gray
        HighlightGroup::Number => Color::from_rgb8(209, 154, 102),  // orange
        HighlightGroup::Operator => Color::from_rgb8(86, 182, 194), // cyan
        HighlightGroup::Variable => Color::from_rgb8(224, 108, 117), // red
        HighlightGroup::Property => Color::from_rgb8(224, 108, 117), // red
        HighlightGroup::Punctuation => Color::from_rgb8(171, 178, 191), // light gray
        HighlightGroup::Attribute => Color::from_rgb8(209, 154, 102), // orange
        HighlightGroup::Namespace => Color::from_rgb8(229, 192, 123), // yellow
        HighlightGroup::Constant => Color::from_rgb8(209, 154, 102), // orange
        HighlightGroup::Escape => Color::from_rgb8(86, 182, 194),   // cyan
    }
}

pub struct HighlightSpan {
    pub byte_start: u32,
    pub byte_end: u32,
    pub color: Color,
}
