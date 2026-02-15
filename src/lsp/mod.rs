use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use async_process::{Child, ChildStdin, Command};
use lsp_types::{ClientCapabilities, ClientInfo, InitializeParams, Uri};
use smol::{
    channel::{Receiver, unbounded},
    io::{AsyncReadExt, AsyncWriteExt},
    lock::Mutex,
    spawn,
};
use tracing::{debug, info};

pub struct LspManager {
    pub servers: Vec<LspServer>,
}

pub struct LspServer {
    pub id: usize,
    language_id: String,
    process: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    pub rx: Receiver<String>,
    root_uri: Uri, // TODO: This should be set dynamically based on the nearest toml or something per language & fallback to workspace
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
        let mut process = Command::new(lsp_config.cmd)
            .args(lsp_config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to start LSP server");

        let stdin = process.stdin.take().unwrap();
        let mut stdout = process.stdout.take().unwrap();
        let (tx, rx) = unbounded();

        let root_uri_str = format!("file://{}", root_path.display());
        let root_uri: Uri = root_uri_str.parse().expect("Failed to parse URI");
        let server_id = self.servers.len();
        let server = LspServer {
            id: server_id,
            language_id: language_id.to_string(),
            process,
            stdin: Arc::new(Mutex::new(stdin)),
            root_uri,
            rx,

            initialized: false,
        };

        spawn(async move {
            let mut buffer = [0; 1024];
            loop {
                match stdout.read(&mut buffer).await {
                    Ok(n) if n > 0 => {
                        let output = String::from_utf8_lossy(&buffer[..n]).to_string();
                        debug!(output, "LSP server output");
                        tx.send(output).await.unwrap();
                    }
                    Ok(_) => break, // EOF
                    Err(e) => {
                        eprintln!("Error reading from LSP server: {}", e);
                        break;
                    }
                }
            }
        })
        .detach();
        let init_params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(server.root_uri.clone()), // TODO: Should be using workspace_folders ?
            capabilities: ClientCapabilities::default(),
            client_info: Some(ClientInfo {
                name: "remax".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            ..Default::default()
        };

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": init_params,
        });
        let body = serde_json::to_string(&request).unwrap();
        let content_length = body.len();
        let message = format!("Content-Length: {}\r\n\r\n{}", content_length, body);
        debug!(message, "sending initialize request to LSP server");

        let stdin_handle = Arc::clone(&server.stdin);
        spawn(async move {
            let mut stdin = stdin_handle.lock().await;
            stdin.write_all(message.as_bytes()).await.unwrap();
            stdin.flush().await.unwrap();
            debug!("initialize request sent to LSP server");
        })
        .detach();

        self.servers.push(server);
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
