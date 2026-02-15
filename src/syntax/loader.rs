use std::collections::HashMap;

use tree_house::{InjectionLanguageMarker, Language, LanguageConfig, LanguageLoader};

use crate::syntax::highlight::capture_name_to_highlight;

pub struct Loader {
    configs: Vec<LanguageConfig>,
    extensions: HashMap<String, Language>,
}

impl Loader {
    pub fn new() -> Self {
        let grammar = tree_sitter_rust::LANGUAGE
            .try_into()
            .expect("Failed to load Rust grammar");
        let rust_config = LanguageConfig::new(
            grammar,
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "", // Don't have any bundled
        )
        .expect("Failed to create Rust language config");

        rust_config.configure(capture_name_to_highlight);

        let rust = Language::new(0);
        Loader {
            configs: vec![rust_config],
            extensions: HashMap::from([("rs".to_string(), rust)]),
        }
    }

    pub fn language_for_extension(&self, ext: &str) -> Option<Language> {
        self.extensions.get(ext).copied()
    }
}

impl LanguageLoader for Loader {
    fn language_for_marker(&self, marker: InjectionLanguageMarker) -> Option<Language> {
        match marker {
            #[allow(clippy::collapsible_match)]
            InjectionLanguageMarker::Name(name) => match name {
                "rust" => Some(Language::new(0)),
                _ => None,
            },
            _ => None,
        }
    }

    fn get_config(&self, lang: Language) -> Option<&LanguageConfig> {
        self.configs.get(lang.idx())
    }
}
