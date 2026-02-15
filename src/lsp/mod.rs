use std::{path::Path, process::Stdio, sync::Arc};

use async_process::{Child, ChildStdin, Command};
use lsp_types::{
    ClientCapabilities, ClientInfo, InitializeParams, ServerCapabilities, Uri,
    notification::Notification,
};
use smol::{
    channel::{Receiver, unbounded},
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    lock::Mutex,
    spawn,
};
use tracing::{debug, info};

pub enum LspIncoming {
    Response {
        id: i64,
        result: serde_json::Value,
    },
    Error {
        id: i64,
        error: serde_json::Value,
    },
    Notification {
        method: String,
        params: serde_json::Value,
    },
    ServerRequest {
        id: i64,
        method: String,
        params: serde_json::Value,
    },
}

pub struct LspServer {
    pub id: usize,
    language_id: String,
    process: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    pub rx: Receiver<String>,
    root_uri: Uri, // TODO: This should be set dynamically based on the nearest toml or something per language & fallback to workspace
    pub capabilities: Option<ServerCapabilities>,
    pub initialized: bool,
}

struct LspConfig {
    cmd: String,
    args: Vec<String>,
}

pub struct LspManager {
    pub servers: Vec<LspServer>,
}

impl LspServer {
    pub fn send_notification<N: Notification>(&self, params: N::Params) -> anyhow::Result<()> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": N::METHOD,
            "params": params,
        });
        let body = serde_json::to_string(&request)?;
        let content_length = body.len();
        let message = format!("Content-Length: {}\r\n\r\n{}", content_length, body);
        debug!(message, "sending notification to LSP server");

        let stdin_handle = Arc::clone(&self.stdin);
        spawn(async move {
            let mut stdin = stdin_handle.lock().await;
            stdin.write_all(message.as_bytes()).await.unwrap();
            stdin.flush().await.unwrap();
            debug!("notification sent to LSP server");
        })
        .detach();
        Ok(())
    }
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
        let stdout = process.stdout.take().unwrap();
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

            capabilities: None,
            initialized: false,
        };

        spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut header_buf = String::new();

            loop {
                let mut content_length: Option<usize> = None;
                loop {
                    header_buf.clear();

                    match reader.read_line(&mut header_buf).await {
                        Ok(0) => return, // EOF
                        Ok(_) => {
                            debug!(raw_header = ?header_buf, "header line");
                            // Parsing the content length header
                            let line = header_buf.trim();
                            if line.is_empty() {
                                break; // blank line = end of header
                            }

                            if let Some(len_str) = line.strip_prefix("Content-Length:") {
                                content_length = len_str.trim().parse().ok()
                            }
                        }
                        Err(e) => {
                            debug!(?e, "error reading from LSP server stdout");
                            return;
                        }
                    }
                }

                let content_length = match content_length {
                    Some(len) => len,
                    None => {
                        debug!("missing Content-Length header");
                        continue;
                    }
                };

                let mut body = vec![0u8; content_length];
                if reader.read_exact(&mut body).await.is_err() {
                    debug!("lsp stdout EOF during body read");
                    return;
                }
                let body = String::from_utf8_lossy(&body).to_string();
                tx.send(body).await.unwrap();
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

    pub fn handle_message(&self, server_id: usize, raw: &str) -> Option<LspIncoming> {
        let json: serde_json::Value = serde_json::from_str(raw).ok()?;

        if let Some(id) = json.get("id") {
            if let Some(method) = json.get("method") {
                Some(LspIncoming::ServerRequest {
                    id: id.as_i64()?,
                    method: method.as_str()?.to_string(),
                    params: json.get("params").cloned().unwrap_or_default(),
                })
            } else if json.get("error").is_some() {
                Some(LspIncoming::Error {
                    id: id.as_i64()?,
                    error: json["error"].clone(),
                })
            } else {
                Some(LspIncoming::Response {
                    id: id.as_i64()?,
                    result: json.get("result").cloned().unwrap_or_default(),
                })
            }
        } else {
            Some(LspIncoming::Notification {
                method: json.get("method")?.as_str()?.to_string(),
                params: json.get("params").cloned().unwrap_or_default(),
            })
        }
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
