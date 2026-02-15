use std::{collections::HashMap, path::Path, process::Stdio, sync::Arc};

use async_process::{Child, ChildStdin, Command};
use lsp_types::{
    notification::Notification, request::Request, ClientCapabilities, ClientInfo, InitializeParams,
    ServerCapabilities, Uri,
};
use smol::{
    channel::{unbounded, Receiver},
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    lock::Mutex,
    spawn,
};
use tracing::{debug, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
}

impl Language {
    /// Gets the LSP compatible language ID
    pub fn id(&self) -> &'static str {
        match self {
            Language::Rust => "rust",
        }
    }
    // Parse a language from a file path, returning None if the extension is not recognized or missing
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }

    // Parse a language from a file extension, returning None if it's not recognized
    fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Language::Rust),
            _ => None,
        }
    }

    pub fn get_lsp_config(&self) -> LspConfig {
        match self {
            Language::Rust => LspConfig {
                cmd: "rust-analyzer".to_string(),
                args: vec![],
            },
        }
    }
}

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
    pub language: Language,
    _process: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    pub rx: Receiver<String>,
    root_uri: Uri, // TODO: This should be set dynamically based on the nearest toml or something per language & fallback to workspace
    pub capabilities: Option<ServerCapabilities>,
    pub initialized: bool,
}

pub struct LspConfig {
    cmd: String,
    args: Vec<String>,
}

pub struct LspManager {
    pub servers: Vec<LspServer>,
    next_request_id: i64,
    pending_requests: HashMap<i64, PendingRequest>,
}

pub struct PendingRequest {
    pub method: String,
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

    pub fn send_request<N: Request>(&self, id: i64, params: N::Params) -> anyhow::Result<()> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": N::METHOD,
            "params": params,
        });
        let body = serde_json::to_string(&request)?;
        let content_length = body.len();
        let message = format!("Content-Length: {}\r\n\r\n{}", content_length, body);
        debug!(message, "sending request to LSP server");

        let stdin_handle = Arc::clone(&self.stdin);
        spawn(async move {
            let mut stdin = stdin_handle.lock().await;
            stdin.write_all(message.as_bytes()).await.unwrap();
            stdin.flush().await.unwrap();
            debug!("request sent to LSP server");
        })
        .detach();

        Ok(())
    }
}

impl LspManager {
    pub fn new() -> Self {
        Self {
            servers: Vec::new(),
            next_request_id: 1,
            pending_requests: HashMap::new(),
        }
    }

    pub fn get_inited_server_for_language(&self, language: Language) -> Option<&LspServer> {
        self.servers
            .iter()
            .find(|s| s.language == language && s.initialized)
    }

    pub fn take_pending_request(&mut self, id: i64) -> Option<PendingRequest> {
        self.pending_requests.remove(&id)
    }

    pub fn start_server(&mut self, language: Language, root_path: &Path) {
        if self.servers.iter().any(|s| s.language == language) {
            info!(?language, "LSP server already running");
            return;
        }

        info!(?language, ?root_path, "starting LSP server from root");
        let lsp_config = language.get_lsp_config();
        let mut process = Command::new(lsp_config.cmd)
            .args(lsp_config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to start LSP server");

        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        let (tx, rx) = unbounded();

        let root_uri = path_to_uri(root_path).expect("Failed to convert root path to URI");
        let server_id = self.servers.len();
        let server = LspServer {
            id: server_id,
            language,
            _process: process,
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
            #[allow(deprecated)]
            root_uri: Some(server.root_uri.clone()), // TODO: Should be using workspace_folders ?
            capabilities: ClientCapabilities::default(),
            client_info: Some(ClientInfo {
                name: "remax".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            ..Default::default()
        };

        let req_id = self.next_request_id;
        self.pending_requests.insert(
            req_id,
            PendingRequest {
                method: "initialize".to_string(),
            },
        );
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "method": "initialize",
            "params": init_params,
        });
        self.next_request_id += 1;
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

    pub fn handle_message(&self, raw: &str) -> Option<LspIncoming> {
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

    pub fn send_request<R: Request>(&mut self, server_id: usize, params: R::Params) -> Option<i64> {
        let server = self.servers.iter().find(|s| s.id == server_id)?;
        if !server.initialized {
            return None;
        }

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        self.pending_requests.insert(
            request_id,
            PendingRequest {
                method: R::METHOD.to_string(),
            },
        );
        server.send_request::<R>(request_id, params).ok()?;
        Some(request_id)
    }
}

/// Convert rope char offset to LSP Position (0-indexed line, 0-indexed UTF-16 column).
pub fn offset_to_lsp_position(rope: &ropey::Rope, offset: usize) -> lsp_types::Position {
    let line = rope.char_to_line(offset);
    let line_start = rope.line_to_char(line);
    let col_chars = offset - line_start;
    // Convert char offset to UTF-16 code units
    let utf16_col: usize = rope
        .line(line)
        .chars()
        .take(col_chars)
        .map(|c| c.len_utf16())
        .sum();
    lsp_types::Position::new(line as u32, utf16_col as u32)
}

/// Convert LSP Position to rope char offset.
pub fn lsp_position_to_offset(rope: &ropey::Rope, pos: &lsp_types::Position) -> usize {
    let line = pos.line as usize;
    if line >= rope.len_lines() {
        return rope.len_chars();
    }
    let line_start = rope.line_to_char(line);
    let mut utf16_count = 0u32;
    let mut char_count = 0usize;
    for ch in rope.line(line).chars() {
        if utf16_count >= pos.character {
            break;
        }
        utf16_count += ch.len_utf16() as u32;
        char_count += 1;
    }
    line_start + char_count
}

/// Build a file:// URI from a path, canonicalizing to absolute.
pub fn path_to_uri(path: &Path) -> Option<Uri> {
    let abs = std::fs::canonicalize(path).ok()?;
    let uri_str = format!("file://{}", abs.display());
    uri_str.parse().ok()
}
