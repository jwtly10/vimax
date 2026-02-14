use std::path::Path;

use tracing::{debug, info};

pub struct LspManager {
    servers: Vec<LspServer>,
}

pub struct LspServer {
    language_id: String,
    process: std::process::Child,
    // stdin: std::process::ChildStdin,
    // // stdout is moved into the iced Subscription
    // capabilities: Option<ServerCapabilities>,
    root_path: String, // TODO: This should be set dynamically based on the nearest toml or something per language & fallback to workspace
    initialized: bool,
}

struct LspConfig {
    cmd: String,
    args: Vec<String>,
}

impl LspManager {
    pub fn new() -> Self {
        Self {
            servers: Vec::new(),
        }
    }

    pub fn start_server(&mut self, language_id: &str, root_path: &Path) {
        if self.servers.iter().any(|s| s.language_id == language_id) {
            info!(language_id, "LSP server already running");
            return;
        }

        info!(?language_id, ?root_path, "starting LSP server from root");
        let lsp_config = get_config(language_id).expect("Unsupported language");
        // let process = std::process::Command::new(lsp_config.cmd)
        //     .args(lsp_config.args)
        //     .stdin(std::process::Stdio::piped())
        //     .stdout(std::process::Stdio::piped())
        //     .spawn()
        //     .expect("Failed to start LSP server");
        //
        // let server = LspServer {
        //     language_id: language_id.to_string(),
        //     process,
        //     root_uri: root_uri.to_string(),
        //     initialized: false,
        // };
        //
        // self.servers.push(server);
    }
}

fn get_config(language_id: &str) -> Option<LspConfig> {
    match language_id {
        "rust" => Some(LspConfig {
            cmd: "rust-analyzer".to_string(),
            args: vec![],
        }),
        _ => None,
    }
}

pub fn detect_language_from_path(path: &Path) -> Option<String> {
    debug!(?path, "detecting language from path");
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        match ext {
            "rs" => Some("rust".to_string()),
            _ => None,
        }
    } else {
        None
    }
}
