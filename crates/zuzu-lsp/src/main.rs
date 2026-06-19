use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lsp_server::{Connection, ErrorCode, Message, Request, RequestId, Response, ResponseError};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Initialized, Notification,
};
use lsp_types::request::{
    Completion, DocumentSymbolRequest, FoldingRangeRequest, Formatting, GotoDefinition,
    HoverRequest, Request as LspRequest, Shutdown, WorkspaceSymbolRequest,
};
use lsp_types::{
    CompletionOptions, CompletionResponse, Diagnostic as LspDiagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    FoldingRange, FoldingRangeKind, FoldingRangeParams, GotoDefinitionResponse, Hover,
    HoverContents, HoverParams, InitializeParams, InitializeResult, Location, MarkedString, OneOf,
    Position, PublishDiagnosticsParams, Range, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Uri, WorkspaceSymbol, WorkspaceSymbolParams,
};
use zuzu_analysis::{Analyzer, Diagnostic, DiagnosticSeverity as AnalysisSeverity};
use zuzu_toolchain::Toolchain;

#[derive(Debug, Parser)]
#[command(name = "zuzu-lsp")]
#[command(about = "ZuzuScript language server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long)]
    stdio: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    Doctor,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Doctor) => doctor(),
        None if cli.stdio => run_stdio(),
        None => {
            eprintln!("Use --stdio to start the language server, or run `zuzu-lsp doctor`.");
            Ok(())
        }
    }
}

fn doctor() -> Result<()> {
    let cwd = std::env::current_dir().context("could not get current directory")?;
    let toolchain = Toolchain::discover(&[cwd]);
    for line in toolchain.doctor_lines() {
        println!("{line}");
    }
    Ok(())
}

fn run_stdio() -> Result<()> {
    let (connection, io_threads) = Connection::stdio();
    let (initialize_id, initialize_params) = connection.initialize_start()?;
    let initialize_params: InitializeParams = serde_json::from_value(initialize_params)
        .context("client sent invalid initialize params")?;

    let roots = initialize_roots(&initialize_params);
    let capabilities = capabilities();
    let initialize_result = InitializeResult {
        capabilities,
        server_info: Some(lsp_types::ServerInfo {
            name: "zuzu-lsp".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    };
    connection.initialize_finish(
        initialize_id,
        serde_json::to_value(initialize_result).context("could not serialize initialize result")?,
    )?;

    let mut server = Server::new(roots, connection);
    let result = server.run();
    drop(server);
    io_threads.join()?;
    result
}

fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(lsp_types::FoldingRangeProviderCapability::Simple(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec!["/".to_string(), ":".to_string()]),
            ..Default::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

fn initialize_roots(params: &InitializeParams) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(workspace_folders) = &params.workspace_folders {
        roots.extend(
            workspace_folders
                .iter()
                .filter_map(|folder| uri_to_file_path(&folder.uri)),
        );
    }

    #[allow(deprecated)]
    {
        if let Some(root_uri) = &params.root_uri {
            if let Some(path) = uri_to_file_path(root_uri) {
                roots.push(path);
            }
        }
    }

    if roots.is_empty() {
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd);
        }
    }

    roots.sort();
    roots.dedup();
    roots
}

struct Server {
    analyzer: Analyzer,
    toolchain: Toolchain,
    connection: Connection,
    shutdown_requested: bool,
}

impl Server {
    fn new(roots: Vec<PathBuf>, connection: Connection) -> Self {
        let analyzer = Analyzer::new(roots.clone());
        let toolchain = Toolchain::discover(&roots);
        Self {
            analyzer,
            toolchain,
            connection,
            shutdown_requested: false,
        }
    }

    fn run(&mut self) -> Result<()> {
        while let Ok(message) = self.connection.receiver.recv() {
            match message {
                Message::Request(request) => {
                    self.handle_request(request)?;
                }
                Message::Response(_) => {}
                Message::Notification(notification) => {
                    if notification.method == "exit" {
                        break;
                    }
                    if notification.method == Initialized::METHOD {
                        continue;
                    }
                    self.handle_notification(notification.method, notification.params)?;
                }
            }
        }
        Ok(())
    }

    fn handle_request(&mut self, request: Request) -> Result<()> {
        match request.method.as_str() {
            DocumentSymbolRequest::METHOD => self.document_symbol(request),
            FoldingRangeRequest::METHOD => self.folding_range(request),
            HoverRequest::METHOD => self.hover(request),
            Completion::METHOD => self.completion(request),
            GotoDefinition::METHOD => self.definition(request),
            Formatting::METHOD => self.formatting(request),
            WorkspaceSymbolRequest::METHOD => self.workspace_symbol(request),
            Shutdown::METHOD => {
                self.shutdown_requested = true;
                self.send_response(Response::new_ok(request.id, serde_json::Value::Null))
            }
            _ => self.send_response(Response::new_err(
                request.id,
                ErrorCode::MethodNotFound as i32,
                format!("unsupported request `{}`", request.method),
            )),
        }
    }

    fn handle_notification(&mut self, method: String, params: serde_json::Value) -> Result<()> {
        match method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: DidOpenTextDocumentParams = serde_json::from_value(params)?;
                let uri = params.text_document.uri.to_string();
                let diagnostics = self
                    .analyzer
                    .upsert_document(&uri, params.text_document.text);
                self.publish_diagnostics(uri, diagnostics)
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams = serde_json::from_value(params)?;
                if let Some(change) = params.content_changes.into_iter().last() {
                    let uri = params.text_document.uri.to_string();
                    let diagnostics = self.analyzer.upsert_document(&uri, change.text);
                    self.publish_diagnostics(uri, diagnostics)?;
                }
                Ok(())
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams = serde_json::from_value(params)?;
                let uri = params.text_document.uri.to_string();
                self.analyzer.remove_document(&uri);
                self.publish_diagnostics(uri, Vec::new())
            }
            _ => Ok(()),
        }
    }

    fn document_symbol(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, DocumentSymbolParams) =
            request.extract(DocumentSymbolRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let symbols = self
            .analyzer
            .document_symbols(&uri)
            .into_iter()
            .map(to_document_symbol)
            .collect();
        self.send_response(Response::new_ok(
            id,
            DocumentSymbolResponse::Nested(symbols),
        ))
    }

    fn folding_range(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, FoldingRangeParams) =
            request.extract(FoldingRangeRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let ranges: Vec<FoldingRange> = self
            .analyzer
            .folding_ranges(&uri)
            .into_iter()
            .map(|range| FoldingRange {
                start_line: range.start_line,
                start_character: Some(range.start_character),
                end_line: range.end_line,
                end_character: Some(range.end_character),
                kind: range.kind.and_then(folding_kind),
                collapsed_text: None,
            })
            .collect();
        self.send_response(Response::new_ok(id, ranges))
    }

    fn hover(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, HoverParams) = request.extract(HoverRequest::METHOD)?;
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = from_position(params.text_document_position_params.position);
        let hover = self.analyzer.hover(&uri, position).map(|hover| Hover {
            contents: HoverContents::Scalar(MarkedString::String(hover.markdown)),
            range: Some(to_range(hover.range)),
        });
        self.send_response(Response::new_ok(id, hover))
    }

    fn completion(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, lsp_types::CompletionParams) =
            request.extract(Completion::METHOD)?;
        let uri = params.text_document_position.text_document.uri.to_string();
        let position = from_position(params.text_document_position.position);
        let items: Vec<lsp_types::CompletionItem> = self
            .analyzer
            .completions(&uri, position)
            .into_iter()
            .map(|item| lsp_types::CompletionItem {
                label: item.label,
                detail: item.detail,
                ..Default::default()
            })
            .collect();
        self.send_response(Response::new_ok(id, CompletionResponse::Array(items)))
    }

    fn definition(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, lsp_types::GotoDefinitionParams) =
            request.extract(GotoDefinition::METHOD)?;
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = from_position(params.text_document_position_params.position);
        let location = self
            .analyzer
            .definition(&uri, position)
            .and_then(|location| {
                parse_uri(&location.uri).map(|uri| Location {
                    uri,
                    range: to_range(location.range),
                })
            });
        self.send_response(Response::new_ok(
            id,
            location.map(GotoDefinitionResponse::Scalar),
        ))
    }

    fn formatting(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, DocumentFormattingParams) =
            request.extract(Formatting::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let Some(document) = self.analyzer.document(&uri) else {
            return self.send_response(Response::new_ok(id, Option::<Vec<TextEdit>>::None));
        };

        match self.toolchain.format_text(document.text()) {
            Ok(text) => {
                let edits = vec![TextEdit {
                    range: to_range(document.full_range()),
                    new_text: text,
                }];
                self.send_response(Response::new_ok(id, Some(edits)))
            }
            Err(error) => self.send_response(Response::new_err(
                id,
                ErrorCode::InternalError as i32,
                error.to_string(),
            )),
        }
    }

    fn workspace_symbol(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, WorkspaceSymbolParams) =
            request.extract(WorkspaceSymbolRequest::METHOD)?;
        let symbols: Vec<WorkspaceSymbol> = self
            .analyzer
            .workspace_symbols(&params.query)
            .into_iter()
            .filter_map(|symbol| {
                Some(WorkspaceSymbol {
                    name: symbol.name,
                    kind: symbol_kind(symbol.kind),
                    tags: None,
                    container_name: symbol.detail,
                    location: OneOf::Left(Location {
                        uri: parse_uri(&symbol.uri)?,
                        range: to_range(symbol.selection_range),
                    }),
                    data: None,
                })
            })
            .collect();
        self.send_response(Response::new_ok(id, symbols))
    }

    fn publish_diagnostics(&self, uri: String, diagnostics: Vec<Diagnostic>) -> Result<()> {
        let Some(uri) = parse_uri(&uri) else {
            return Ok(());
        };
        let params = PublishDiagnosticsParams {
            uri,
            diagnostics: diagnostics.into_iter().map(to_diagnostic).collect(),
            version: None,
        };
        self.connection
            .sender
            .send(Message::Notification(lsp_server::Notification::new(
                "textDocument/publishDiagnostics".to_string(),
                params,
            )))?;
        Ok(())
    }

    fn send_response(&self, response: Response) -> Result<()> {
        self.connection.sender.send(Message::Response(response))?;
        Ok(())
    }
}

fn to_diagnostic(diagnostic: Diagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: to_range(diagnostic.range),
        severity: Some(match diagnostic.severity {
            AnalysisSeverity::Error => DiagnosticSeverity::ERROR,
            AnalysisSeverity::Warning => DiagnosticSeverity::WARNING,
            AnalysisSeverity::Hint => DiagnosticSeverity::HINT,
        }),
        code: Some(lsp_types::NumberOrString::String(
            diagnostic.code.to_string(),
        )),
        code_description: None,
        source: Some(diagnostic.source.to_string()),
        message: diagnostic.message,
        related_information: None,
        tags: None,
        data: None,
    }
}

#[allow(deprecated)]
fn to_document_symbol(symbol: zuzu_analysis::Symbol) -> DocumentSymbol {
    DocumentSymbol {
        name: symbol.name,
        detail: symbol.detail,
        kind: symbol_kind(symbol.kind),
        tags: None,
        deprecated: None,
        range: to_range(symbol.range),
        selection_range: to_range(symbol.selection_range),
        children: None,
    }
}

fn symbol_kind(kind: zuzu_analysis::SymbolKind) -> lsp_types::SymbolKind {
    match kind {
        zuzu_analysis::SymbolKind::Module => lsp_types::SymbolKind::MODULE,
        zuzu_analysis::SymbolKind::Function => lsp_types::SymbolKind::FUNCTION,
        zuzu_analysis::SymbolKind::Method => lsp_types::SymbolKind::METHOD,
        zuzu_analysis::SymbolKind::Class => lsp_types::SymbolKind::CLASS,
        zuzu_analysis::SymbolKind::Trait => lsp_types::SymbolKind::INTERFACE,
        zuzu_analysis::SymbolKind::Field => lsp_types::SymbolKind::FIELD,
        zuzu_analysis::SymbolKind::Variable => lsp_types::SymbolKind::VARIABLE,
        zuzu_analysis::SymbolKind::Parameter => lsp_types::SymbolKind::VARIABLE,
        zuzu_analysis::SymbolKind::Import => lsp_types::SymbolKind::MODULE,
    }
}

fn to_range(range: zuzu_analysis::Range) -> Range {
    Range {
        start: to_position(range.start),
        end: to_position(range.end),
    }
}

fn to_position(position: zuzu_analysis::Position) -> Position {
    Position {
        line: position.line,
        character: position.character,
    }
}

fn from_position(position: Position) -> zuzu_analysis::Position {
    zuzu_analysis::Position::new(position.line, position.character)
}

fn parse_uri(uri: &str) -> Option<Uri> {
    uri.parse().ok()
}

fn uri_to_file_path(uri: &Uri) -> Option<PathBuf> {
    url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

fn folding_kind(kind: &str) -> Option<FoldingRangeKind> {
    match kind {
        "comment" => Some(FoldingRangeKind::Comment),
        "imports" => Some(FoldingRangeKind::Imports),
        "region" => Some(FoldingRangeKind::Region),
        _ => None,
    }
}

#[allow(dead_code)]
fn _response_error(message: String) -> ResponseError {
    ResponseError {
        code: ErrorCode::InternalError as i32,
        message,
        data: None,
    }
}
