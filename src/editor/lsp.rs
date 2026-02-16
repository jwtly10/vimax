use std::path::Path;

use lsp_types::notification::{DidOpenTextDocument, Initialized};
use lsp_types::request::GotoDefinition;
use lsp_types::{
    DidOpenTextDocumentParams, GotoDefinitionParams, InitializedParams, TextDocumentIdentifier,
    TextDocumentPositionParams,
};
use tracing::debug;

use crate::lsp::{lsp_position_to_offset, offset_to_lsp_position, path_to_uri, LspIncoming};

use super::Editor;

impl Editor {
    /// Handle an incoming LSP message from a server.
    /// This was previously the `Message::Lsp` arm in app.rs update().
    pub fn handle_lsp_message(&mut self, server_id: usize, raw: &str) {
        if let Some(incoming) = self.workspace().lsp_manager.handle_message(raw) {
            match incoming {
                LspIncoming::Response { id, result } => {
                    debug!(id, "got lsp response");
                    if let Some(pending) =
                        self.workspace_mut().lsp_manager.take_pending_request(id)
                    {
                        match pending.method.as_str() {
                            "initialize" => {
                                self.handle_lsp_initialize(server_id, result);
                            }
                            "textDocument/definition" => {
                                debug!(?result, "definition response");
                                self.handle_definition_response(result);
                            }
                            _ => {
                                debug!(method = %pending.method, "response matched pending request");
                            }
                        }
                    }
                }
                LspIncoming::Notification { method, params } => {
                    debug!(?method, ?params, "got notification");
                }
                LspIncoming::ServerRequest { id, method, params } => {
                    debug!(?id, ?method, ?params, "got server request");
                }
                LspIncoming::Error { id, error } => {
                    debug!(id, ?error, "got error response");
                }
            }
        }
        debug!(server_id, raw, "LSP message received in update");
    }

    fn handle_lsp_initialize(&mut self, server_id: usize, result: serde_json::Value) {
        debug!(?result, "server initialized");
        let capabilities: lsp_types::ServerCapabilities =
            serde_json::from_value(result["capabilities"].clone()).unwrap();
        debug!(?capabilities, "server capabilities");

        if let Some(server) = self
            .workspace_mut()
            .lsp_manager
            .servers
            .iter_mut()
            .find(|s| s.id == server_id)
        {
            server.capabilities = Some(capabilities);

            if server
                .send_notification::<Initialized>(InitializedParams {})
                .is_ok()
            {
                server.initialized = true;
                let server_lang = server.language;

                for buf in &self.buffers {
                    if buf.language() != Some(server_lang) {
                        continue;
                    }
                    if let Some(file_path) = buf.file_path()
                        && let Some(uri) = path_to_uri(Path::new(file_path))
                    {
                        let text = buf.rope().to_string();
                        if let Some(server) = self
                            .workspace()
                            .lsp_manager
                            .get_inited_server_for_language(server_lang)
                        {
                            server
                                .send_notification::<DidOpenTextDocument>(
                                    DidOpenTextDocumentParams {
                                        text_document: lsp_types::TextDocumentItem {
                                            uri,
                                            language_id: server_lang.id().to_string(),
                                            version: buf.version() as i32,
                                            text,
                                        },
                                    },
                                )
                                .ok();
                        }
                    }
                }
            }
        }
    }

    /// https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_definition
    fn handle_definition_response(&mut self, result: serde_json::Value) {
        // Response can be: null, Location, Location[], LocationLink[]
        let location: Option<lsp_types::Location> = if result.is_null() {
            None
        } else if result.get("uri").is_some() {
            serde_json::from_value(result).ok()
        } else if let Some(arr) = result.as_array() {
            if let Some(first) = arr.first() {
                if first.get("targetUri").is_some() {
                    let link: Option<lsp_types::LocationLink> =
                        serde_json::from_value(first.clone()).ok();
                    link.map(|l| lsp_types::Location {
                        uri: l.target_uri,
                        range: l.target_selection_range,
                    })
                } else {
                    serde_json::from_value(first.clone()).ok()
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some(location) = location {
            let path_str = location.uri.path().to_string();
            debug!(uri = ?location.uri, path = %path_str, line = location.range.start.line, col = location.range.start.character, "jumping to definition");
            let path = std::path::Path::new(&path_str);

            self.open_file(path);

            let buf = self.buffer();
            let offset = lsp_position_to_offset(buf.rope(), &location.range.start);
            self.window_mut().cursor = buf.clamp_cursor(offset);
            self.ensure_cursor_visible();
            self.status_message = format!(
                "Definition: {}:{}:{}",
                path.display(),
                location.range.start.line + 1,
                location.range.start.character + 1
            );
        } else {
            self.status_message = String::from("No definition found");
        }
    }

    pub(crate) fn execute_lsp_goto_definition(&mut self) {
        self.push_jump();
        let buf = self.buffer();
        let cursor = self.cursor();
        if let Some(file_path) = buf.file_path() {
            let position = offset_to_lsp_position(buf.rope(), cursor);
            let path = Path::new(file_path);
            if let Some(lang) = buf.language()
                && let Some(uri) = path_to_uri(path)
            {
                if let Some(server) = self
                    .workspace()
                    .lsp_manager
                    .get_inited_server_for_language(lang)
                {
                    let server_id = server.id;
                    self.workspace_mut()
                        .lsp_manager
                        .send_request::<GotoDefinition>(
                            server_id,
                            GotoDefinitionParams {
                                text_document_position_params: TextDocumentPositionParams {
                                    text_document: TextDocumentIdentifier { uri },
                                    position,
                                },
                                work_done_progress_params: Default::default(),
                                partial_result_params: Default::default(),
                            },
                        );
                } else {
                    self.status_message = String::from("LSP not ready");
                }
            }
        }
    }
}
