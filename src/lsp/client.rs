use std::path::Path;

use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
};
use lsp_types::request::{
    GotoDeclaration, GotoDefinition, GotoImplementation, References, Request,
};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionParams, ReferenceContext, ReferenceParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, VersionedTextDocumentIdentifier,
};

use super::{LspManager, offset_to_lsp_position, path_to_uri};
use crate::buffer::Buffer;

impl LspManager {
    pub fn goto_definition(&mut self, buf: &Buffer, cursor: usize) -> Option<i64> {
        self.send_position_request::<GotoDefinition>(buf, cursor)
    }

    pub fn goto_implementation(&mut self, buf: &Buffer, cursor: usize) -> Option<i64> {
        self.send_position_request::<GotoImplementation>(buf, cursor)
    }

    pub fn goto_declaration(&mut self, buf: &Buffer, cursor: usize) -> Option<i64> {
        self.send_position_request::<GotoDeclaration>(buf, cursor)
    }

    pub fn find_references(&mut self, buf: &Buffer, cursor: usize) -> Option<i64> {
        let (server_id, uri, position) = self.resolve_position(buf, cursor)?;
        self.send_request::<References>(
            server_id,
            ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            },
        )
    }

    pub fn notify_did_open(&self, buf: &Buffer, content: &str) {
        let Some((server, uri)) = self.resolve_server(buf) else {
            return;
        };
        server
            .send_notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: buf.language().unwrap().id().to_string(),
                    version: buf.version() as i32,
                    text: content.to_string(),
                },
            })
            .ok();
    }

    pub fn notify_did_change(&self, buf: &Buffer) {
        let Some((server, uri)) = self.resolve_server(buf) else {
            return;
        };
        server
            .send_notification::<DidChangeTextDocument>(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri,
                    version: buf.version() as i32,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: buf.rope().to_string(),
                }],
            })
            .ok();
    }

    pub fn notify_did_save(&self, buf: &Buffer) {
        let Some((server, uri)) = self.resolve_server(buf) else {
            return;
        };
        server
            .send_notification::<DidSaveTextDocument>(DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
                text: Some(buf.rope().to_string()),
            })
            .ok();
    }

    pub fn notify_did_close(&self, buf: &Buffer) {
        let Some((server, uri)) = self.resolve_server(buf) else {
            return;
        };
        server
            .send_notification::<DidCloseTextDocument>(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
            })
            .ok();
    }

    fn resolve_server(&self, buf: &Buffer) -> Option<(&super::LspServer, lsp_types::Uri)> {
        let lang = buf.language()?;
        let file_path = buf.file_path()?;
        let server = self.get_inited_server_for_language(lang)?;
        let uri = path_to_uri(Path::new(file_path))?;
        Some((server, uri))
    }

    fn resolve_position(
        &self,
        buf: &Buffer,
        cursor: usize,
    ) -> Option<(usize, lsp_types::Uri, lsp_types::Position)> {
        let lang = buf.language()?;
        let file_path = buf.file_path()?;
        let server = self.get_inited_server_for_language(lang)?;
        let uri = path_to_uri(Path::new(file_path))?;
        let position = offset_to_lsp_position(buf.rope(), cursor);
        Some((server.id, uri, position))
    }

    fn send_position_request<R: Request<Params = GotoDefinitionParams>>(
        &mut self,
        buf: &Buffer,
        cursor: usize,
    ) -> Option<i64> {
        let (server_id, uri, position) = self.resolve_position(buf, cursor)?;
        self.send_request::<R>(
            server_id,
            GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        )
    }
}
