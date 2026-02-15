use std::time::Duration;

use ropey::Rope;
use tracing::error;
use tree_house::Language;
use tree_house::Syntax;
use tree_house::highlighter::Highlighter;

use crate::syntax::highlight::{HighlightSpan, color_for_group, highlight_to_group};
use crate::syntax::loader::Loader;

pub mod highlight;
pub mod loader;

const TIMEOUT: Duration = Duration::from_millis(500);

pub struct SyntaxState {
    pub syntax: Syntax,
    pub cached_version: u64,
    pub language: Language,
}

impl SyntaxState {
    pub fn new(rope: &Rope, language: Language, loader: &Loader) -> Option<Self> {
        match Syntax::new(rope.slice(..), language, TIMEOUT, loader) {
            Ok(syntax) => Some(Self {
                syntax,
                cached_version: 0,
                language,
            }),
            Err(e) => {
                error!(?e, "error creating syntax");
                None
            }
        }
    }

    pub fn ensure_parsed(&mut self, rope: &Rope, version: u64, loader: &Loader) {
        if self.cached_version != version {
            match Syntax::new(rope.slice(..), self.language, TIMEOUT, loader) {
                Ok(syntax) => {
                    self.syntax = syntax;
                    self.cached_version = version;
                }
                Err(e) => error!(?e, "error reparsing syntax"),
            }
        }
    }

    pub fn highlights_for_range(
        &self,
        rope: &Rope,
        loader: &Loader,
        start_byte: u32,
        end_byte: u32,
    ) -> Vec<HighlightSpan> {
        let mut hl = Highlighter::new(&self.syntax, rope.slice(..), loader, start_byte..end_byte);

        let mut spans = Vec::new();
        let mut prev_offset = start_byte;

        loop {
            let offset = hl.next_event_offset();
            if offset >= end_byte {
                if prev_offset < end_byte
                    && let Some(h) = hl.active_highlights().next_back()
                {
                    spans.push(HighlightSpan {
                        byte_start: prev_offset,
                        byte_end: end_byte,
                        color: color_for_group(highlight_to_group(h)),
                    });
                }
                break;
            }

            if offset > prev_offset
                && let Some(h) = hl.active_highlights().next_back()
            {
                spans.push(HighlightSpan {
                    byte_start: prev_offset,
                    byte_end: offset,
                    color: color_for_group(highlight_to_group(h)),
                });
            }

            prev_offset = offset;
            let (_event, _active) = hl.advance();
        }

        spans
    }
}
