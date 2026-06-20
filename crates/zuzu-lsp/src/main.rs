use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lsp_server::{Connection, ErrorCode, Message, Request, RequestId, Response, ResponseError};
use lsp_types::notification::{
    Cancel, DidChangeConfiguration, DidChangeTextDocument, DidChangeWatchedFiles,
    DidChangeWorkspaceFolders, DidCloseTextDocument, DidOpenTextDocument, Initialized, LogMessage,
    Notification, Progress as ProgressNotification,
};
use lsp_types::request::{
    CallHierarchyIncomingCalls, CallHierarchyOutgoingCalls, CallHierarchyPrepare,
    CodeActionRequest, CodeLensRequest, Completion, DocumentDiagnosticRequest,
    DocumentHighlightRequest, DocumentLinkRequest, DocumentSymbolRequest, ExecuteCommand,
    FoldingRangeRequest, Formatting, GotoDefinition, HoverRequest, InlayHintRequest,
    PrepareRenameRequest, References, Rename, Request as LspRequest, ResolveCompletionItem,
    SelectionRangeRequest, SemanticTokensFullRequest, Shutdown, SignatureHelpRequest,
    TypeHierarchyPrepare, TypeHierarchySubtypes, TypeHierarchySupertypes,
    WorkspaceDiagnosticRequest, WorkspaceSymbolRequest,
};
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    CallHierarchyServerCapability, CancelParams, CodeAction, CodeActionKind, CodeActionOptions,
    CodeActionOrCommand, CodeActionProviderCapability, CodeLens, CodeLensOptions,
    Command as LspCommand, CompletionItemKind, CompletionOptions, CompletionResponse, CreateFile,
    CreateFileOptions, Diagnostic as LspDiagnostic, DiagnosticOptions,
    DiagnosticServerCapabilities, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentChangeOperation, DocumentChanges, DocumentDiagnosticParams,
    DocumentDiagnosticReport, DocumentDiagnosticReportResult, DocumentFormattingParams,
    DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams, DocumentLink,
    DocumentLinkOptions, DocumentLinkParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, Documentation, ExecuteCommandOptions, FoldingRange, FoldingRangeKind,
    FoldingRangeParams, FullDocumentDiagnosticReport, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, InitializeParams, InitializeResult, InlayHint, InlayHintKind, InlayHintLabel,
    InlayHintOptions, InlayHintParams, InlayHintServerCapabilities, Location, LogMessageParams,
    MarkedString, MarkupContent, MarkupKind, MessageType, NumberOrString, OneOf,
    ParameterInformation, ParameterLabel, Position, PrepareRenameResponse, ProgressParams,
    ProgressParamsValue, ProgressToken, PublishDiagnosticsParams, Range, ReferenceParams,
    RelatedFullDocumentDiagnosticReport, RenameOptions, RenameParams, ResourceOp,
    SelectionRange as LspSelectionRange, SelectionRangeParams, SemanticToken as LspSemanticToken,
    SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, SignatureHelp, SignatureHelpOptions,
    SignatureInformation, TextDocumentContentChangeEvent, TextDocumentEdit,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextEdit, TypeHierarchyItem, TypeHierarchyPrepareParams,
    TypeHierarchySubtypesParams, TypeHierarchySupertypesParams, Uri, WorkDoneProgress,
    WorkDoneProgressBegin, WorkDoneProgressEnd, WorkDoneProgressOptions, WorkDoneProgressReport,
    WorkspaceDiagnosticParams, WorkspaceDiagnosticReport, WorkspaceDiagnosticReportResult,
    WorkspaceDocumentDiagnosticReport, WorkspaceEdit, WorkspaceFoldersServerCapabilities,
    WorkspaceFullDocumentDiagnosticReport, WorkspaceServerCapabilities, WorkspaceSymbol,
    WorkspaceSymbolParams,
};
use ropey::Rope;
use serde_json::json;
use tree_sitter::{InputEdit, Parser as TreeParser, Point, Tree};
use zuzu_analysis::{
    distribution_metadata_diagnostics, Analyzer, Diagnostic,
    DiagnosticSeverity as AnalysisSeverity, ImportFix, ImportFixAction, ModuleTarget,
    PackageDiagnostic, RenameError,
};
use zuzu_toolchain::{ParserDiagnostic, ParserDiagnosticSeverity, ToolOutput, Toolchain};

#[derive(Debug, Parser)]
#[command(name = "zuzu-lsp")]
#[command(about = "ZuzuScript language server")]
#[command(version)]
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
    let (initialize_id, initialize_params_value) = connection.initialize_start()?;
    let workspace_trusted = initialize_workspace_trusted(&initialize_params_value);
    let initialize_settings = RawServerSettings::from_initialize_params(&initialize_params_value);
    let initialize_params: InitializeParams = serde_json::from_value(initialize_params_value)
        .context("client sent invalid initialize params")?;

    let roots = initialize_roots(&initialize_params);
    let settings = initialize_settings.resolve(&roots);
    let capabilities = capabilities();
    let initialize_result = InitializeResult {
        capabilities,
        server_info: Some(lsp_types::ServerInfo {
            name: "zuzu-lsp".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    };
    let mut initialize_value =
        serde_json::to_value(initialize_result).context("could not serialize initialize result")?;
    initialize_value["capabilities"]["typeHierarchyProvider"] = json!(true);
    connection.initialize_finish(initialize_id, initialize_value)?;

    let mut server = Server::new(roots, connection, workspace_trusted, settings);
    let result = server.run();
    drop(server);
    io_threads.join()?;
    result
}

#[derive(Debug, Clone, Default)]
struct RawServerSettings {
    module_roots: Vec<String>,
    runtime_parser_diagnostics: Option<bool>,
}

impl RawServerSettings {
    fn from_initialize_params(params: &serde_json::Value) -> Self {
        [
            params.pointer("/initializationOptions/zuzu"),
            params.pointer("/initializationOptions/settings/zuzu"),
        ]
        .into_iter()
        .flatten()
        .find_map(Self::from_value)
        .unwrap_or_default()
    }

    fn from_configuration_params(params: &serde_json::Value) -> Option<Self> {
        [
            params.pointer("/settings/zuzu"),
            params.pointer("/zuzu"),
            Some(params),
        ]
        .into_iter()
        .flatten()
        .find_map(Self::from_value)
    }

    fn from_value(value: &serde_json::Value) -> Option<Self> {
        let object = value.as_object()?;
        let module_roots: Vec<String> = object
            .get("moduleRoots")
            .or_else(|| object.get("module_roots"))
            .and_then(|value| value.as_array())
            .map(|roots| {
                roots
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let runtime_parser_diagnostics = object
            .get("runtimeParserDiagnostics")
            .or_else(|| object.get("runtime_parser_diagnostics"))
            .and_then(|value| value.as_bool());
        if module_roots.is_empty() && runtime_parser_diagnostics.is_none() {
            return None;
        }
        Some(Self {
            module_roots,
            runtime_parser_diagnostics,
        })
    }

    fn resolve(self, roots: &[PathBuf]) -> ServerSettings {
        ServerSettings {
            module_roots: resolve_configured_module_roots(&self.module_roots, roots),
            runtime_parser_diagnostics: self.runtime_parser_diagnostics.unwrap_or(true),
        }
    }
}

#[derive(Debug, Clone)]
struct ServerSettings {
    module_roots: Vec<PathBuf>,
    runtime_parser_diagnostics: bool,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            module_roots: Vec::new(),
            runtime_parser_diagnostics: true,
        }
    }
}

fn initialize_workspace_trusted(params: &serde_json::Value) -> bool {
    params
        .pointer("/capabilities/workspace/workspaceTrust/trusted")
        .or_else(|| params.pointer("/initializationOptions/workspaceTrust/trusted"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                ..Default::default()
            },
        )),
        document_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(lsp_types::FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(lsp_types::SelectionRangeProviderCapability::Simple(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(true),
            trigger_characters: Some(vec!["/".to_string(), ":".to_string()]),
            ..Default::default()
        }),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),
        inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
            InlayHintOptions {
                resolve_provider: Some(false),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            },
        ))),
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX, CodeActionKind::SOURCE]),
            resolve_provider: Some(false),
            ..Default::default()
        })),
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
            identifier: Some("zuzu".to_string()),
            inter_file_dependencies: true,
            workspace_diagnostics: true,
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
                legend: semantic_tokens_legend(),
                range: Some(false),
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        document_formatting_provider: Some(OneOf::Left(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec![
                "zuzu.doctor".to_string(),
                "zuzu.formatDocument".to_string(),
                "zuzu.testFile".to_string(),
                "zuzu.testWorkspace".to_string(),
                "zuzu.renderDocs".to_string(),
                "zuzu.verifyPackage".to_string(),
                "zuzu.packageReport".to_string(),
                "zuzu.dependencyGraph".to_string(),
                "zuzu.replInstructions".to_string(),
            ],
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),
        workspace: Some(WorkspaceServerCapabilities {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: None,
        }),
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

    normalize_roots(roots)
}

fn normalize_roots(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
    if roots.is_empty() {
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd);
        }
    }

    roots.sort();
    roots.dedup();
    roots
}

fn request_id_from_number_or_string(id: NumberOrString) -> RequestId {
    match id {
        NumberOrString::Number(id) => RequestId::from(id),
        NumberOrString::String(id) => RequestId::from(id),
    }
}

fn configured_module_roots(settings: &ServerSettings, toolchain: &Toolchain) -> Vec<PathBuf> {
    let mut roots = settings.module_roots.clone();
    roots.extend(toolchain.module_search_paths.clone());
    roots.extend(toolchain.installed_modules.clone());
    dedup_path_order(&mut roots);
    roots
}

fn resolve_configured_module_roots(roots: &[String], workspace_roots: &[PathBuf]) -> Vec<PathBuf> {
    let base = workspace_roots.first();
    let mut resolved: Vec<PathBuf> = roots
        .iter()
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else if let Some(base) = base {
                base.join(path)
            } else {
                path
            }
        })
        .collect();
    dedup_path_order(&mut resolved);
    resolved
}

fn dedup_path_order(paths: &mut Vec<PathBuf>) {
    let mut seen = Vec::new();
    paths.retain(|path| {
        if seen.iter().any(|seen_path| seen_path == path) {
            false
        } else {
            seen.push(path.clone());
            true
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DocumentKind {
    Script,
    Module,
    ExtensionlessScript,
    DistributionMetadata,
    Other,
}

#[derive(Debug, Clone)]
struct DocumentSnapshot {
    uri: String,
    language_id: String,
    version: Option<i32>,
    text: Rope,
    tree: Tree,
    kind: DocumentKind,
}

impl DocumentSnapshot {
    fn new(uri: String, language_id: String, version: Option<i32>, text: String) -> Self {
        let kind = classify_document_with_language(&uri, &language_id, &text);
        let tree = parse_tree(&text, None);
        Self {
            uri,
            language_id,
            version,
            text: Rope::from_str(&text),
            tree,
            kind,
        }
    }

    fn text(&self) -> String {
        self.text.to_string()
    }

    fn is_zuzu_document(&self) -> bool {
        matches!(
            self.kind,
            DocumentKind::Script | DocumentKind::Module | DocumentKind::ExtensionlessScript
        )
    }

    fn is_distribution_metadata(&self) -> bool {
        self.kind == DocumentKind::DistributionMetadata
    }

    fn apply_change(&mut self, change: TextDocumentContentChangeEvent) -> Result<()> {
        if let Some(range) = change.range {
            let start = char_offset_for_position(&self.text, range.start)
                .with_context(|| format!("invalid change start for {}", self.uri))?;
            let end = char_offset_for_position(&self.text, range.end)
                .with_context(|| format!("invalid change end for {}", self.uri))?;
            if start > end {
                anyhow::bail!("invalid change range for {}", self.uri);
            }
            let start_byte = self.text.char_to_byte(start);
            let old_end_byte = self.text.char_to_byte(end);
            let start_position = point_for_char(&self.text, start);
            let old_end_position = point_for_char(&self.text, end);
            let new_end_byte = start_byte + change.text.len();
            let new_end_position = point_after_text(start_position, &change.text);
            self.tree.edit(&InputEdit {
                start_byte,
                old_end_byte,
                new_end_byte,
                start_position,
                old_end_position,
                new_end_position,
            });
            self.text.remove(start..end);
            self.text.insert(start, &change.text);
            let text = self.text();
            self.tree = parse_tree(&text, Some(&self.tree));
        } else {
            self.text = Rope::from_str(&change.text);
            self.tree = parse_tree(&change.text, None);
        }
        self.kind = classify_document_with_language(&self.uri, &self.language_id, &self.text());
        Ok(())
    }

    #[cfg(test)]
    fn has_syntax_error(&self) -> bool {
        self.tree.root_node().has_error()
    }
}

struct Server {
    analyzer: Analyzer,
    toolchain: Toolchain,
    settings: ServerSettings,
    doc_cache: HashMap<PathBuf, Option<String>>,
    text_documents: HashMap<String, DocumentSnapshot>,
    queued_messages: VecDeque<Message>,
    cancelled_requests: BTreeSet<RequestId>,
    connection: Connection,
    shutdown_requested: bool,
    workspace_trusted: bool,
}

impl Server {
    fn new(
        roots: Vec<PathBuf>,
        connection: Connection,
        workspace_trusted: bool,
        settings: ServerSettings,
    ) -> Self {
        let toolchain = Toolchain::discover(&roots);
        let analyzer = Analyzer::with_module_roots(
            roots.clone(),
            configured_module_roots(&settings, &toolchain),
        );
        Self {
            analyzer,
            toolchain,
            settings,
            doc_cache: HashMap::new(),
            text_documents: HashMap::new(),
            queued_messages: VecDeque::new(),
            cancelled_requests: BTreeSet::new(),
            connection,
            shutdown_requested: false,
            workspace_trusted,
        }
    }

    fn run(&mut self) -> Result<()> {
        while let Some(message) = self.next_message()? {
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

    fn next_message(&mut self) -> Result<Option<Message>> {
        if let Some(message) = self.queued_messages.pop_front() {
            return Ok(Some(message));
        }
        match self.connection.receiver.recv() {
            Ok(message) => Ok(Some(message)),
            Err(_) => Ok(None),
        }
    }

    fn handle_request(&mut self, request: Request) -> Result<()> {
        match request.method.as_str() {
            DocumentSymbolRequest::METHOD => self.document_symbol(request),
            FoldingRangeRequest::METHOD => self.folding_range(request),
            SelectionRangeRequest::METHOD => self.selection_range(request),
            HoverRequest::METHOD => self.hover(request),
            Completion::METHOD => self.completion(request),
            ResolveCompletionItem::METHOD => self.resolve_completion_item(request),
            SignatureHelpRequest::METHOD => self.signature_help(request),
            InlayHintRequest::METHOD => self.inlay_hint(request),
            CodeLensRequest::METHOD => self.code_lens(request),
            CallHierarchyPrepare::METHOD => self.prepare_call_hierarchy(request),
            CallHierarchyIncomingCalls::METHOD => self.call_hierarchy_incoming(request),
            CallHierarchyOutgoingCalls::METHOD => self.call_hierarchy_outgoing(request),
            TypeHierarchyPrepare::METHOD => self.prepare_type_hierarchy(request),
            TypeHierarchySupertypes::METHOD => self.type_hierarchy_supertypes(request),
            TypeHierarchySubtypes::METHOD => self.type_hierarchy_subtypes(request),
            GotoDefinition::METHOD => self.definition(request),
            References::METHOD => self.references(request),
            DocumentHighlightRequest::METHOD => self.document_highlight(request),
            PrepareRenameRequest::METHOD => self.prepare_rename(request),
            Rename::METHOD => self.rename(request),
            DocumentLinkRequest::METHOD => self.document_link(request),
            CodeActionRequest::METHOD => self.code_action(request),
            DocumentDiagnosticRequest::METHOD => self.document_diagnostic(request),
            WorkspaceDiagnosticRequest::METHOD => self.workspace_diagnostic(request),
            SemanticTokensFullRequest::METHOD => self.semantic_tokens_full(request),
            Formatting::METHOD => self.formatting(request),
            ExecuteCommand::METHOD => self.execute_command(request),
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
            Cancel::METHOD => self.cancel_request(params),
            DidOpenTextDocument::METHOD => {
                let params: DidOpenTextDocumentParams = serde_json::from_value(params)?;
                let uri = params.text_document.uri.to_string();
                self.invalidate_documentation_cache(&uri);
                let version = Some(params.text_document.version);
                let document = DocumentSnapshot::new(
                    uri.clone(),
                    params.text_document.language_id,
                    version,
                    params.text_document.text,
                );
                let text = document.text();
                let should_analyze = document.is_zuzu_document();
                let is_distribution_metadata = document.is_distribution_metadata();
                self.text_documents.insert(uri.clone(), document);
                if should_analyze {
                    let diagnostics = self.analyzer.upsert_document(&uri, text.as_str());
                    let diagnostics = self.with_toolchain_diagnostics(&uri, &text, diagnostics);
                    self.publish_diagnostics(uri, version, diagnostics)
                } else if is_distribution_metadata {
                    self.publish_diagnostics(uri, version, self.metadata_diagnostics(&text))
                } else {
                    self.publish_diagnostics(uri, version, Vec::new())
                }
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams = serde_json::from_value(params)?;
                let uri = params.text_document.uri.to_string();
                self.invalidate_documentation_cache(&uri);
                let Some(document) = self.text_documents.get_mut(&uri) else {
                    return Ok(());
                };
                let version = Some(params.text_document.version);
                document.version = version;
                for change in params.content_changes {
                    document.apply_change(change)?;
                }
                let text = document.text();
                if document.is_zuzu_document() {
                    let diagnostics = self.analyzer.upsert_document(&uri, text.as_str());
                    let diagnostics = self.with_toolchain_diagnostics(&uri, &text, diagnostics);
                    self.publish_diagnostics(uri, version, diagnostics)?;
                } else if document.is_distribution_metadata() {
                    self.publish_diagnostics(uri, version, self.metadata_diagnostics(&text))?;
                } else {
                    self.analyzer.remove_document(&uri);
                    self.publish_diagnostics(uri, version, Vec::new())?;
                }
                Ok(())
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams = serde_json::from_value(params)?;
                let uri = params.text_document.uri.to_string();
                self.invalidate_documentation_cache(&uri);
                let version = self.document_version(&uri);
                self.text_documents.remove(&uri);
                self.analyzer.remove_document(&uri);
                self.publish_diagnostics(uri, version, Vec::new())
            }
            DidChangeWorkspaceFolders::METHOD => {
                let params: DidChangeWorkspaceFoldersParams = serde_json::from_value(params)?;
                self.change_workspace_folders(params)
            }
            DidChangeWatchedFiles::METHOD => {
                let _params: DidChangeWatchedFilesParams = serde_json::from_value(params)?;
                self.refresh_workspace_configuration()
            }
            DidChangeConfiguration::METHOD => {
                let roots = self.analyzer.workspace().roots().to_vec();
                if let Some(settings) = RawServerSettings::from_configuration_params(&params) {
                    self.settings = settings.resolve(&roots);
                }
                self.refresh_workspace_configuration()
            }
            _ => Ok(()),
        }
    }

    fn cancel_request(&mut self, params: serde_json::Value) -> Result<()> {
        let params: CancelParams = serde_json::from_value(params)?;
        self.cancelled_requests
            .insert(request_id_from_number_or_string(params.id));
        Ok(())
    }

    fn drain_cancellation_notifications(&mut self) -> Result<()> {
        while let Ok(message) = self.connection.receiver.try_recv() {
            match message {
                Message::Notification(notification) if notification.method == Cancel::METHOD => {
                    self.cancel_request(notification.params)?;
                }
                other => self.queued_messages.push_back(other),
            }
        }
        Ok(())
    }

    fn consume_cancelled_request(&mut self, id: &RequestId) -> bool {
        self.cancelled_requests.remove(id)
    }

    fn invalidate_documentation_cache(&mut self, uri: &str) {
        let Some(path) = uri_to_path(uri) else {
            return;
        };
        self.doc_cache.remove(&path);
        if let Ok(canonical) = path.canonicalize() {
            self.doc_cache.remove(&canonical);
        }
    }

    fn change_workspace_folders(&mut self, params: DidChangeWorkspaceFoldersParams) -> Result<()> {
        let mut roots = self.analyzer.workspace().roots().to_vec();
        for removed in params.event.removed {
            if let Some(path) = uri_to_file_path(&removed.uri) {
                roots.retain(|root| root != &path);
            }
        }
        for added in params.event.added {
            if let Some(path) = uri_to_file_path(&added.uri) {
                roots.push(path);
            }
        }
        self.reset_workspace(normalize_roots(roots))
    }

    fn refresh_workspace_configuration(&mut self) -> Result<()> {
        self.reset_workspace(self.analyzer.workspace().roots().to_vec())
    }

    fn reset_workspace(&mut self, roots: Vec<PathBuf>) -> Result<()> {
        self.toolchain = Toolchain::discover(&roots);
        self.analyzer = Analyzer::with_module_roots(
            roots,
            configured_module_roots(&self.settings, &self.toolchain),
        );
        self.doc_cache.clear();

        let open_documents: Vec<(String, String, Option<i32>, bool, bool)> = self
            .text_documents
            .iter()
            .map(|(uri, document)| {
                (
                    uri.clone(),
                    document.text(),
                    document.version,
                    document.is_zuzu_document(),
                    document.is_distribution_metadata(),
                )
            })
            .collect();
        for (uri, text, version, is_zuzu_document, is_distribution_metadata) in open_documents {
            if is_zuzu_document {
                let diagnostics = self.analyzer.upsert_document(&uri, text.as_str());
                let diagnostics = self.with_toolchain_diagnostics(&uri, &text, diagnostics);
                self.publish_diagnostics(uri, version, diagnostics)?;
            } else if is_distribution_metadata {
                self.publish_diagnostics(uri, version, self.metadata_diagnostics(&text))?;
            }
        }
        Ok(())
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

    fn selection_range(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, SelectionRangeParams) =
            request.extract(SelectionRangeRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let positions: Vec<zuzu_analysis::Position> =
            params.positions.into_iter().map(from_position).collect();
        let ranges: Vec<LspSelectionRange> = self
            .analyzer
            .selection_ranges(&uri, &positions)
            .into_iter()
            .map(to_selection_range)
            .collect();
        self.send_response(Response::new_ok(id, Some(ranges)))
    }

    fn prepare_call_hierarchy(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, CallHierarchyPrepareParams) =
            request.extract(CallHierarchyPrepare::METHOD)?;
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = from_position(params.text_document_position_params.position);
        let items: Vec<CallHierarchyItem> = self
            .analyzer
            .prepare_call_hierarchy(&uri, position)
            .into_iter()
            .filter_map(to_call_hierarchy_item)
            .collect();
        self.send_response(Response::new_ok(id, Some(items)))
    }

    fn call_hierarchy_incoming(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, CallHierarchyIncomingCallsParams) =
            request.extract(CallHierarchyIncomingCalls::METHOD)?;
        let Some(item) = from_call_hierarchy_item(params.item) else {
            return self.send_response(Response::new_ok(
                id,
                Option::<Vec<CallHierarchyIncomingCall>>::None,
            ));
        };
        let calls: Vec<CallHierarchyIncomingCall> = self
            .analyzer
            .incoming_calls(&item)
            .into_iter()
            .filter_map(to_incoming_call)
            .collect();
        self.send_response(Response::new_ok(id, Some(calls)))
    }

    fn call_hierarchy_outgoing(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, CallHierarchyOutgoingCallsParams) =
            request.extract(CallHierarchyOutgoingCalls::METHOD)?;
        let Some(item) = from_call_hierarchy_item(params.item) else {
            return self.send_response(Response::new_ok(
                id,
                Option::<Vec<CallHierarchyOutgoingCall>>::None,
            ));
        };
        let calls: Vec<CallHierarchyOutgoingCall> = self
            .analyzer
            .outgoing_calls(&item)
            .into_iter()
            .filter_map(to_outgoing_call)
            .collect();
        self.send_response(Response::new_ok(id, Some(calls)))
    }

    fn prepare_type_hierarchy(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, TypeHierarchyPrepareParams) =
            request.extract(TypeHierarchyPrepare::METHOD)?;
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = from_position(params.text_document_position_params.position);
        let items: Vec<TypeHierarchyItem> = self
            .analyzer
            .prepare_type_hierarchy(&uri, position)
            .into_iter()
            .filter_map(to_type_hierarchy_item)
            .collect();
        self.send_response(Response::new_ok(id, Some(items)))
    }

    fn type_hierarchy_supertypes(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, TypeHierarchySupertypesParams) =
            request.extract(TypeHierarchySupertypes::METHOD)?;
        let Some(item) = from_type_hierarchy_item(params.item) else {
            return self
                .send_response(Response::new_ok(id, Option::<Vec<TypeHierarchyItem>>::None));
        };
        let items: Vec<TypeHierarchyItem> = self
            .analyzer
            .supertypes(&item)
            .into_iter()
            .filter_map(to_type_hierarchy_item)
            .collect();
        self.send_response(Response::new_ok(id, Some(items)))
    }

    fn type_hierarchy_subtypes(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, TypeHierarchySubtypesParams) =
            request.extract(TypeHierarchySubtypes::METHOD)?;
        let Some(item) = from_type_hierarchy_item(params.item) else {
            return self
                .send_response(Response::new_ok(id, Option::<Vec<TypeHierarchyItem>>::None));
        };
        let items: Vec<TypeHierarchyItem> = self
            .analyzer
            .subtypes(&item)
            .into_iter()
            .filter_map(to_type_hierarchy_item)
            .collect();
        self.send_response(Response::new_ok(id, Some(items)))
    }

    fn hover(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, HoverParams) = request.extract(HoverRequest::METHOD)?;
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = from_position(params.text_document_position_params.position);
        let hover = self
            .analyzer
            .module_target_at(&uri, position)
            .and_then(|target| self.module_documentation_hover(target))
            .or_else(|| {
                self.analyzer
                    .symbol_documentation_target_at(&uri, position)
                    .and_then(|target| self.symbol_documentation_hover(target))
            })
            .or_else(|| {
                self.analyzer.hover(&uri, position).map(|hover| Hover {
                    contents: HoverContents::Scalar(MarkedString::String(hover.markdown)),
                    range: Some(to_range(hover.range)),
                })
            });
        self.send_response(Response::new_ok(id, hover))
    }

    fn completion(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, lsp_types::CompletionParams) =
            request.extract(Completion::METHOD)?;
        let uri = params.text_document_position.text_document.uri.to_string();
        let position = from_position(params.text_document_position.position);
        let mut items = Vec::new();
        for item in self.analyzer.completions(&uri, position) {
            let documentation = self.completion_documentation(&item);
            items.push(lsp_types::CompletionItem {
                label: item.label,
                detail: item.detail,
                kind: Some(to_completion_item_kind(item.kind)),
                documentation,
                ..Default::default()
            });
        }
        self.send_response(Response::new_ok(id, CompletionResponse::Array(items)))
    }

    fn resolve_completion_item(&mut self, request: Request) -> Result<()> {
        let (id, mut item): (RequestId, lsp_types::CompletionItem) =
            request.extract(ResolveCompletionItem::METHOD)?;
        if item.documentation.is_none() && item.kind == Some(CompletionItemKind::MODULE) {
            item.documentation = self.module_completion_documentation(&item.label, true);
        }
        self.send_response(Response::new_ok(id, item))
    }

    fn signature_help(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, lsp_types::SignatureHelpParams) =
            request.extract(SignatureHelpRequest::METHOD)?;
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = from_position(params.text_document_position_params.position);
        let help = self
            .analyzer
            .signature_help(&uri, position)
            .map(to_signature_help);
        self.send_response(Response::new_ok(id, help))
    }

    fn inlay_hint(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, InlayHintParams) =
            request.extract(InlayHintRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let hints: Vec<InlayHint> = self
            .analyzer
            .inlay_hints(&uri, from_range(params.range))
            .into_iter()
            .map(to_inlay_hint)
            .collect();
        self.send_response(Response::new_ok(id, Some(hints)))
    }

    fn code_lens(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, lsp_types::CodeLensParams) =
            request.extract(CodeLensRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        self.send_response(Response::new_ok(id, Some(code_lenses_for_uri(&uri))))
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

    fn references(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, ReferenceParams) = request.extract(References::METHOD)?;
        let uri = params.text_document_position.text_document.uri.to_string();
        let position = from_position(params.text_document_position.position);
        let locations: Vec<Location> = self
            .analyzer
            .references(&uri, position, params.context.include_declaration)
            .into_iter()
            .filter_map(to_location)
            .collect();
        self.send_response(Response::new_ok(id, Some(locations)))
    }

    fn document_highlight(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, DocumentHighlightParams) =
            request.extract(DocumentHighlightRequest::METHOD)?;
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = from_position(params.text_document_position_params.position);
        let highlights: Vec<DocumentHighlight> = self
            .analyzer
            .references(&uri, position, true)
            .into_iter()
            .filter(|location| location.uri == uri)
            .map(|location| DocumentHighlight {
                range: to_range(location.range),
                kind: Some(DocumentHighlightKind::READ),
            })
            .collect();
        self.send_response(Response::new_ok(id, Some(highlights)))
    }

    fn prepare_rename(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, TextDocumentPositionParams) =
            request.extract(PrepareRenameRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let position = from_position(params.position);
        let response = self
            .analyzer
            .prepare_rename(&uri, position)
            .map(|range| PrepareRenameResponse::Range(to_range(range)));
        self.send_response(Response::new_ok(id, response))
    }

    fn rename(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, RenameParams) = request.extract(Rename::METHOD)?;
        let uri = params.text_document_position.text_document.uri.to_string();
        let position = from_position(params.text_document_position.position);
        match self.analyzer.rename(&uri, position, &params.new_name) {
            Ok(edits) => self.send_response(Response::new_ok(id, workspace_edit(edits))),
            Err(RenameError::InvalidIdentifier(name)) => self.send_response(Response::new_err(
                id,
                ErrorCode::InvalidParams as i32,
                format!("`{name}` is not a valid ZuzuScript identifier"),
            )),
        }
    }

    fn document_link(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, DocumentLinkParams) =
            request.extract(DocumentLinkRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let links: Vec<DocumentLink> = if is_distribution_metadata_uri(&uri) {
            self.metadata_document_links(&uri)
        } else {
            self.zuzu_document_links(&uri)
        };
        self.send_response(Response::new_ok(id, Some(links)))
    }

    fn code_action(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, lsp_types::CodeActionParams) =
            request.extract(CodeActionRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let mut actions: Vec<CodeActionOrCommand> = self
            .analyzer
            .import_fixes(&uri, from_range(params.range))
            .into_iter()
            .filter_map(code_action_for_import_fix)
            .collect();
        if self.analyzer.document(&uri).is_some() {
            actions.push(format_document_code_action(&uri));
        }
        self.send_response(Response::new_ok(id, Some(actions)))
    }

    fn document_diagnostic(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, DocumentDiagnosticParams) =
            request.extract(DocumentDiagnosticRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let diagnostics = if let Some(document) = self.text_documents.get(&uri) {
            if document.is_distribution_metadata() {
                self.metadata_diagnostics(&document.text())
            } else {
                self.analyzer.diagnostics(&uri)
            }
        } else {
            self.analyzer.diagnostics(&uri)
        }
        .into_iter()
        .map(to_diagnostic)
        .collect();
        self.send_response(Response::new_ok(
            id,
            document_diagnostic_report(diagnostics),
        ))
    }

    fn workspace_diagnostic(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, WorkspaceDiagnosticParams) =
            request.extract(WorkspaceDiagnosticRequest::METHOD)?;
        let progress_token = params.work_done_progress_params.work_done_token;
        if let Some(token) = &progress_token {
            self.send_work_done_progress(
                token.clone(),
                WorkDoneProgress::Begin(WorkDoneProgressBegin {
                    title: "ZuzuScript workspace diagnostics".to_string(),
                    cancellable: Some(true),
                    message: Some("Collecting workspace diagnostics".to_string()),
                    percentage: Some(0),
                }),
            )?;
        }
        self.drain_cancellation_notifications()?;
        if self.consume_cancelled_request(&id) {
            return self.cancel_workspace_diagnostic(id, progress_token);
        }

        let mut grouped: BTreeMap<String, Vec<LspDiagnostic>> = BTreeMap::new();
        let package_report = self.analyzer.package_report(None);
        self.report_workspace_diagnostic_progress(
            &progress_token,
            Some("Collected package diagnostics".to_string()),
            50,
        )?;
        for diagnostic in package_report.diagnostics {
            self.drain_cancellation_notifications()?;
            if self.consume_cancelled_request(&id) {
                return self.cancel_workspace_diagnostic(id, progress_token);
            }
            grouped
                .entry(diagnostic.uri.clone())
                .or_default()
                .push(to_package_diagnostic(diagnostic));
        }
        let items = grouped
            .into_iter()
            .filter_map(|(uri, diagnostics)| {
                Some(WorkspaceDocumentDiagnosticReport::Full(
                    WorkspaceFullDocumentDiagnosticReport {
                        uri: parse_uri(&uri)?,
                        version: self.document_version(&uri).map(i64::from),
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: None,
                            items: diagnostics,
                        },
                    },
                ))
            })
            .collect();
        self.report_workspace_diagnostic_progress(
            &progress_token,
            Some("Prepared workspace diagnostic report".to_string()),
            100,
        )?;
        self.drain_cancellation_notifications()?;
        if self.consume_cancelled_request(&id) {
            return self.cancel_workspace_diagnostic(id, progress_token);
        }
        if let Some(token) = progress_token {
            self.send_work_done_progress(
                token,
                WorkDoneProgress::End(WorkDoneProgressEnd {
                    message: Some("Workspace diagnostics ready".to_string()),
                }),
            )?;
        }
        self.send_response(Response::new_ok(
            id,
            WorkspaceDiagnosticReportResult::Report(WorkspaceDiagnosticReport { items }),
        ))
    }

    fn report_workspace_diagnostic_progress(
        &self,
        progress_token: &Option<ProgressToken>,
        message: Option<String>,
        percentage: u32,
    ) -> Result<()> {
        let Some(token) = progress_token else {
            return Ok(());
        };
        self.send_work_done_progress(
            token.clone(),
            WorkDoneProgress::Report(WorkDoneProgressReport {
                cancellable: Some(true),
                message,
                percentage: Some(percentage),
            }),
        )
    }

    fn cancel_workspace_diagnostic(
        &self,
        id: RequestId,
        progress_token: Option<ProgressToken>,
    ) -> Result<()> {
        if let Some(token) = progress_token {
            self.send_work_done_progress(
                token,
                WorkDoneProgress::End(WorkDoneProgressEnd {
                    message: Some("Workspace diagnostics cancelled".to_string()),
                }),
            )?;
        }
        self.send_response(Response::new_err(
            id,
            ErrorCode::RequestCanceled as i32,
            "Workspace diagnostics cancelled".to_string(),
        ))
    }

    fn semantic_tokens_full(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, SemanticTokensParams) =
            request.extract(SemanticTokensFullRequest::METHOD)?;
        let uri = params.text_document.uri.to_string();
        let tokens = encode_semantic_tokens(self.analyzer.semantic_tokens(&uri));
        self.send_response(Response::new_ok(
            id,
            Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: tokens,
            })),
        ))
    }

    fn formatting(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, DocumentFormattingParams) =
            request.extract(Formatting::METHOD)?;
        if !self.workspace_trusted {
            return self.send_untrusted_workspace_error(id, "formatting");
        }
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

    fn execute_command(&mut self, request: Request) -> Result<()> {
        let (id, params): (RequestId, lsp_types::ExecuteCommandParams) =
            request.extract(ExecuteCommand::METHOD)?;
        match params.command.as_str() {
            "zuzu.doctor" => {
                self.send_response(Response::new_ok(id, Some(self.toolchain.doctor_lines())))
            }
            "zuzu.formatDocument" => {
                if !self.workspace_trusted {
                    return self.send_untrusted_workspace_error(id, "zuzu.formatDocument");
                }
                let Some(uri) = command_uri_arg(&params.arguments) else {
                    return self.send_response(Response::new_err(
                        id,
                        ErrorCode::InvalidParams as i32,
                        "zuzu.formatDocument requires a document URI argument".to_string(),
                    ));
                };
                let Some(document) = self.analyzer.document(&uri) else {
                    return self.send_response(Response::new_err(
                        id,
                        ErrorCode::InvalidParams as i32,
                        "zuzu.formatDocument requires an open document".to_string(),
                    ));
                };
                match self.toolchain.format_text(document.text()) {
                    Ok(text) => {
                        let Some(uri) = parse_uri(&uri) else {
                            return self.send_response(Response::new_err(
                                id,
                                ErrorCode::InvalidParams as i32,
                                "zuzu.formatDocument requires a valid document URI".to_string(),
                            ));
                        };
                        let edit = WorkspaceEdit::new(HashMap::from([(
                            uri,
                            vec![TextEdit {
                                range: to_range(document.full_range()),
                                new_text: text,
                            }],
                        )]));
                        self.send_response(Response::new_ok(id, edit))
                    }
                    Err(error) => self.send_response(Response::new_err(
                        id,
                        ErrorCode::InternalError as i32,
                        error.to_string(),
                    )),
                }
            }
            "zuzu.testFile" => {
                if !self.workspace_trusted {
                    return self.send_untrusted_workspace_error(id, "zuzu.testFile");
                }
                let Some(path) = command_path_arg(&params.arguments) else {
                    return self.send_response(Response::new_err(
                        id,
                        ErrorCode::InvalidParams as i32,
                        "zuzu.testFile requires a file URI or path argument".to_string(),
                    ));
                };
                self.respond_tool_output(id, self.toolchain.run_test_file(&path))
            }
            "zuzu.testWorkspace" => {
                if !self.workspace_trusted {
                    return self.send_untrusted_workspace_error(id, "zuzu.testWorkspace");
                }
                let path = distribution_command_path_arg(&params.arguments)
                    .or_else(|| self.analyzer.workspace().roots().first().cloned())
                    .unwrap_or_else(|| PathBuf::from("."));
                self.respond_tool_output(id, self.toolchain.run_workspace_tests(&path))
            }
            "zuzu.renderDocs" => {
                if !self.workspace_trusted {
                    return self.send_untrusted_workspace_error(id, "zuzu.renderDocs");
                }
                let Some(path) = command_path_arg(&params.arguments) else {
                    return self.send_response(Response::new_err(
                        id,
                        ErrorCode::InvalidParams as i32,
                        "zuzu.renderDocs requires a file URI or path argument".to_string(),
                    ));
                };
                self.respond_tool_output(id, self.toolchain.render_docs(&path))
            }
            "zuzu.verifyPackage" => {
                if !self.workspace_trusted {
                    return self.send_untrusted_workspace_error(id, "zuzu.verifyPackage");
                }
                let path = distribution_command_path_arg(&params.arguments)
                    .or_else(|| self.analyzer.workspace().roots().first().cloned())
                    .unwrap_or_else(|| PathBuf::from("."));
                self.respond_tool_output(id, self.toolchain.verify_distribution(&path))
            }
            "zuzu.packageReport" => {
                let path = command_path_arg(&params.arguments);
                self.send_response(Response::new_ok(
                    id,
                    self.analyzer.package_report(path.as_deref()),
                ))
            }
            "zuzu.dependencyGraph" => {
                self.send_response(Response::new_ok(id, self.analyzer.dependency_graph()))
            }
            "zuzu.replInstructions" => self.send_response(Response::new_ok(
                id,
                repl_instructions_value(&self.toolchain),
            )),
            command => self.send_response(Response::new_err(
                id,
                ErrorCode::InvalidParams as i32,
                format!("unsupported command `{command}`"),
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

    fn publish_diagnostics(
        &self,
        uri: String,
        version: Option<i32>,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<()> {
        let Some(uri) = parse_uri(&uri) else {
            return Ok(());
        };
        let params = PublishDiagnosticsParams {
            uri,
            diagnostics: diagnostics.into_iter().map(to_diagnostic).collect(),
            version,
        };
        self.connection
            .sender
            .send(Message::Notification(lsp_server::Notification::new(
                "textDocument/publishDiagnostics".to_string(),
                params,
            )))?;
        Ok(())
    }

    fn document_version(&self, uri: &str) -> Option<i32> {
        self.text_documents
            .get(uri)
            .and_then(|document| document.version)
    }

    fn send_work_done_progress(&self, token: ProgressToken, value: WorkDoneProgress) -> Result<()> {
        self.connection
            .sender
            .send(Message::Notification(lsp_server::Notification::new(
                ProgressNotification::METHOD.to_string(),
                ProgressParams {
                    token,
                    value: ProgressParamsValue::WorkDone(value),
                },
            )))?;
        Ok(())
    }

    fn send_response(&self, response: Response) -> Result<()> {
        self.connection.sender.send(Message::Response(response))?;
        Ok(())
    }

    fn send_untrusted_workspace_error(&self, id: RequestId, action: &str) -> Result<()> {
        self.send_response(Response::new_err(
            id,
            ErrorCode::RequestFailed as i32,
            format!("`{action}` is disabled because the client marked this workspace as untrusted"),
        ))
    }

    fn respond_tool_output(
        &self,
        id: RequestId,
        result: Result<ToolOutput, zuzu_toolchain::ToolchainError>,
    ) -> Result<()> {
        match result {
            Ok(output) => {
                self.log_tool_output(&output)?;
                self.send_response(Response::new_ok(id, tool_output_value(output)))
            }
            Err(error) => self.send_response(Response::new_err(
                id,
                ErrorCode::InternalError as i32,
                error.to_string(),
            )),
        }
    }

    fn log_tool_output(&self, output: &ToolOutput) -> Result<()> {
        let command = serde_json::to_string(&output.command)
            .unwrap_or_else(|_| format!("{:?}", output.command));
        self.send_log_message(format!(
            "Zuzu command: {command}; status: {}",
            output.status
        ))
    }

    fn send_log_message(&self, message: String) -> Result<()> {
        self.connection
            .sender
            .send(Message::Notification(lsp_server::Notification::new(
                LogMessage::METHOD.to_string(),
                LogMessageParams {
                    typ: MessageType::LOG,
                    message,
                },
            )))?;
        Ok(())
    }

    fn module_documentation_hover(&mut self, target: ModuleTarget) -> Option<Hover> {
        let markdown = self.documentation_markdown(&target.path)?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("## `{}`\n\n{}", target.module, markdown),
            }),
            range: Some(to_range(target.range)),
        })
    }

    fn symbol_documentation_hover(
        &mut self,
        target: zuzu_analysis::SymbolDocumentationTarget,
    ) -> Option<Hover> {
        let markdown = self.documentation_markdown(&target.path)?;
        let markdown = symbol_documentation_markdown(&markdown, &target.name).unwrap_or(markdown);
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("## `{}`\n\n{}", target.name, markdown),
            }),
            range: Some(to_range(target.range)),
        })
    }

    fn documentation_markdown(&mut self, path: &Path) -> Option<String> {
        if !self.workspace_trusted {
            return None;
        }
        if !self.doc_cache.contains_key(path) {
            let rendered = self.toolchain.render_pod_markdown(path).ok().flatten();
            self.doc_cache.insert(path.to_path_buf(), rendered);
        }
        self.doc_cache.get(path).cloned().flatten()
    }

    fn completion_documentation(
        &self,
        item: &zuzu_analysis::CompletionItem,
    ) -> Option<Documentation> {
        if item.kind != zuzu_analysis::CompletionKind::Module {
            return None;
        }
        let markdown = self.cached_module_documentation(&item.label)?;
        Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }))
    }

    fn cached_module_documentation(&self, module: &str) -> Option<String> {
        let path = self.analyzer.workspace().resolve_module_path(module)?;
        self.doc_cache.get(&path).cloned().flatten()
    }

    fn module_completion_documentation(
        &mut self,
        module: &str,
        render_if_missing: bool,
    ) -> Option<Documentation> {
        let path = self.analyzer.workspace().resolve_module_path(module)?;
        let markdown = if render_if_missing {
            self.documentation_markdown(&path)?
        } else {
            self.doc_cache.get(&path).cloned().flatten()?
        };
        Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }))
    }

    fn metadata_document_links(&self, uri: &str) -> Vec<DocumentLink> {
        let Some(text) = self.text_for_uri(uri) else {
            return Vec::new();
        };
        let mut links: Vec<DocumentLink> = metadata_dependency_links(&text)
            .into_iter()
            .filter_map(|dependency| {
                Some(DocumentLink {
                    range: dependency.range,
                    target: Some(parse_uri(
                        &self
                            .analyzer
                            .workspace()
                            .resolve_module_uri(&dependency.module)?,
                    )?),
                    tooltip: Some(format!("Open dependency module `{}`", dependency.module)),
                    data: None,
                })
            })
            .collect();
        links.extend(metadata_url_links(&text).into_iter().filter_map(|link| {
            Some(DocumentLink {
                range: link.range,
                target: Some(parse_uri(&link.target)?),
                tooltip: Some(format!("Open package metadata `{}` URL", link.field)),
                data: None,
            })
        }));
        links
    }

    fn zuzu_document_links(&self, uri: &str) -> Vec<DocumentLink> {
        let mut links: Vec<DocumentLink> = self
            .analyzer
            .document_links(uri)
            .into_iter()
            .filter_map(|link| {
                Some(DocumentLink {
                    range: to_range(link.range),
                    target: Some(parse_uri(&link.target)?),
                    tooltip: link.tooltip,
                    data: None,
                })
            })
            .collect();

        if let Some(text) = self.text_for_uri(uri) {
            links.extend(pod_document_links(&text, |module| {
                self.analyzer.workspace().resolve_module_uri(module)
            }));
        }

        links
    }

    fn text_for_uri(&self, uri: &str) -> Option<String> {
        if let Some(document) = self.text_documents.get(uri) {
            return Some(document.text());
        }
        let path = uri_to_path(uri)?;
        std::fs::read_to_string(path).ok()
    }

    fn with_toolchain_diagnostics(
        &self,
        uri: &str,
        text: &str,
        mut diagnostics: Vec<Diagnostic>,
    ) -> Vec<Diagnostic> {
        diagnostics.extend(toolchain_diagnostics(uri, &self.toolchain));
        diagnostics.extend(self.runtime_parser_diagnostics(text));
        diagnostics
    }

    fn metadata_diagnostics(&self, text: &str) -> Vec<Diagnostic> {
        let mut diagnostics = distribution_metadata_diagnostics(text);
        diagnostics.extend(package_toolchain_diagnostics(&self.toolchain));
        diagnostics
    }

    fn runtime_parser_diagnostics(&self, text: &str) -> Vec<Diagnostic> {
        if !self.workspace_trusted
            || !self.settings.runtime_parser_diagnostics
            || self.toolchain.zuzu.is_none()
        {
            return Vec::new();
        }

        self.toolchain
            .lint_text(text)
            .map(|diagnostics| {
                diagnostics
                    .into_iter()
                    .map(parser_diagnostic_to_diagnostic)
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn toolchain_diagnostics(uri: &str, toolchain: &Toolchain) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if toolchain.tidy.is_none() {
        diagnostics.push(toolchain_diagnostic(
            "missing-formatter",
            "`zuzu-tidy.pl` was not found; document formatting is unavailable",
        ));
    }

    if toolchain.pod_parse.is_none() && toolchain.zuzudoc.is_none() {
        diagnostics.push(toolchain_diagnostic(
            "missing-doc-renderer",
            "`pod_parse` and `zuzudoc.pl` were not found; POD hovers and rendered docs are unavailable",
        ));
    }

    if is_zuzu_test_uri(uri) && toolchain.zuzuprove.is_none() {
        diagnostics.push(toolchain_diagnostic(
            "missing-test-runner",
            "`zuzuprove` was not found; test commands are unavailable",
        ));
    }

    diagnostics
}

fn package_toolchain_diagnostics(toolchain: &Toolchain) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if toolchain.zuzubox.is_none() {
        diagnostics.push(toolchain_diagnostic(
            "missing-package-verifier",
            "`zuzubox` was not found; package verification commands are unavailable",
        ));
    }

    diagnostics
}

fn toolchain_diagnostic(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        range: zuzu_analysis::Range::new(
            zuzu_analysis::Position::new(0, 0),
            zuzu_analysis::Position::new(0, 0),
        ),
        severity: AnalysisSeverity::Hint,
        source: "zuzu-toolchain",
        code,
        message: message.into(),
    }
}

fn parser_diagnostic_to_diagnostic(diagnostic: ParserDiagnostic) -> Diagnostic {
    let line = diagnostic.line.saturating_sub(1) as u32;
    let column = diagnostic.column.saturating_sub(1) as u32;
    Diagnostic {
        range: zuzu_analysis::Range::new(
            zuzu_analysis::Position::new(line, column),
            zuzu_analysis::Position::new(line, column.saturating_add(1)),
        ),
        severity: match diagnostic.severity {
            ParserDiagnosticSeverity::Error => AnalysisSeverity::Error,
            ParserDiagnosticSeverity::Warning => AnalysisSeverity::Warning,
        },
        source: "zuzu-parser",
        code: parser_diagnostic_code(&diagnostic.kind),
        message: diagnostic.message,
    }
}

fn parser_diagnostic_code(kind: &str) -> &'static str {
    match kind {
        "lex error" => "lex-error",
        "parse error" => "parse-error",
        "incomplete parse error" => "incomplete-parse-error",
        "semantic error" => "semantic-error",
        "semantic warning" => "semantic-warning",
        _ => "parser-diagnostic",
    }
}

fn to_diagnostic(diagnostic: Diagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: to_range(diagnostic.range),
        severity: Some(to_diagnostic_severity(diagnostic.severity)),
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

fn to_package_diagnostic(diagnostic: PackageDiagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: to_range(diagnostic.range),
        severity: Some(to_diagnostic_severity(diagnostic.severity)),
        code: Some(lsp_types::NumberOrString::String(diagnostic.code)),
        code_description: None,
        source: Some(diagnostic.source),
        message: diagnostic.message,
        related_information: None,
        tags: None,
        data: None,
    }
}

fn to_diagnostic_severity(severity: AnalysisSeverity) -> DiagnosticSeverity {
    match severity {
        AnalysisSeverity::Error => DiagnosticSeverity::ERROR,
        AnalysisSeverity::Warning => DiagnosticSeverity::WARNING,
        AnalysisSeverity::Hint => DiagnosticSeverity::HINT,
    }
}

fn document_diagnostic_report(diagnostics: Vec<LspDiagnostic>) -> DocumentDiagnosticReportResult {
    DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(
        RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                result_id: None,
                items: diagnostics,
            },
        },
    ))
}

fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::CLASS,
            SemanticTokenType::INTERFACE,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::METHOD,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::PARAMETER,
        ],
        token_modifiers: vec![SemanticTokenModifier::DECLARATION],
    }
}

fn encode_semantic_tokens(mut tokens: Vec<zuzu_analysis::SemanticToken>) -> Vec<LspSemanticToken> {
    tokens.sort_by_key(|token| {
        (
            token.range.start.line,
            token.range.start.character,
            token.range.end.character,
        )
    });

    let mut encoded = Vec::new();
    let mut last_line = 0;
    let mut last_start = 0;
    for token in tokens {
        let Some(token_type) = semantic_token_type(token.kind) else {
            continue;
        };
        let start = token.range.start;
        let end = token.range.end;
        if start.line != end.line || end.character <= start.character {
            continue;
        }
        let delta_line = start.line.saturating_sub(last_line);
        let delta_start = if delta_line == 0 {
            start.character.saturating_sub(last_start)
        } else {
            start.character
        };
        encoded.push(LspSemanticToken {
            delta_line,
            delta_start,
            length: end.character - start.character,
            token_type,
            token_modifiers_bitset: u32::from(token.is_declaration),
        });
        last_line = start.line;
        last_start = start.character;
    }
    encoded
}

fn semantic_token_type(kind: zuzu_analysis::SymbolKind) -> Option<u32> {
    Some(match kind {
        zuzu_analysis::SymbolKind::Import | zuzu_analysis::SymbolKind::Module => 0,
        zuzu_analysis::SymbolKind::Class => 1,
        zuzu_analysis::SymbolKind::Trait => 2,
        zuzu_analysis::SymbolKind::Function => 3,
        zuzu_analysis::SymbolKind::Method => 4,
        zuzu_analysis::SymbolKind::Field => 5,
        zuzu_analysis::SymbolKind::Variable => 6,
        zuzu_analysis::SymbolKind::Parameter => 7,
    })
}

fn to_call_hierarchy_item(item: zuzu_analysis::CallHierarchyItem) -> Option<CallHierarchyItem> {
    Some(CallHierarchyItem {
        name: item.name,
        kind: symbol_kind(item.kind),
        tags: None,
        detail: item.detail,
        uri: parse_uri(&item.uri)?,
        range: to_range(item.range),
        selection_range: to_range(item.selection_range),
        data: None,
    })
}

fn from_call_hierarchy_item(item: CallHierarchyItem) -> Option<zuzu_analysis::CallHierarchyItem> {
    Some(zuzu_analysis::CallHierarchyItem {
        name: item.name,
        kind: analysis_symbol_kind(item.kind)?,
        uri: item.uri.to_string(),
        range: from_range(item.range),
        selection_range: from_range(item.selection_range),
        detail: item.detail,
    })
}

fn to_incoming_call(call: zuzu_analysis::IncomingCall) -> Option<CallHierarchyIncomingCall> {
    Some(CallHierarchyIncomingCall {
        from: to_call_hierarchy_item(call.from)?,
        from_ranges: call.from_ranges.into_iter().map(to_range).collect(),
    })
}

fn to_outgoing_call(call: zuzu_analysis::OutgoingCall) -> Option<CallHierarchyOutgoingCall> {
    Some(CallHierarchyOutgoingCall {
        to: to_call_hierarchy_item(call.to)?,
        from_ranges: call.from_ranges.into_iter().map(to_range).collect(),
    })
}

fn to_type_hierarchy_item(item: zuzu_analysis::TypeHierarchyItem) -> Option<TypeHierarchyItem> {
    Some(TypeHierarchyItem {
        name: item.name,
        kind: symbol_kind(item.kind),
        tags: None,
        detail: item.detail,
        uri: parse_uri(&item.uri)?,
        range: to_range(item.range),
        selection_range: to_range(item.selection_range),
        data: None,
    })
}

fn from_type_hierarchy_item(item: TypeHierarchyItem) -> Option<zuzu_analysis::TypeHierarchyItem> {
    Some(zuzu_analysis::TypeHierarchyItem {
        name: item.name,
        kind: analysis_symbol_kind(item.kind)?,
        uri: item.uri.to_string(),
        range: from_range(item.range),
        selection_range: from_range(item.selection_range),
        detail: item.detail,
    })
}

fn to_location(location: zuzu_analysis::Location) -> Option<Location> {
    Some(Location {
        uri: parse_uri(&location.uri)?,
        range: to_range(location.range),
    })
}

fn workspace_edit(edits: Vec<zuzu_analysis::WorkspaceTextEdit>) -> Option<WorkspaceEdit> {
    if edits.is_empty() {
        return None;
    }

    let mut changes: BTreeMap<String, Vec<TextEdit>> = BTreeMap::new();
    for edit in edits {
        if parse_uri(&edit.uri).is_none() {
            continue;
        }
        changes.entry(edit.uri).or_default().push(TextEdit {
            range: to_range(edit.edit.range),
            new_text: edit.edit.new_text,
        });
    }

    let document_changes = changes
        .into_iter()
        .filter_map(|(uri, edits)| {
            Some(DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                    uri: parse_uri(&uri)?,
                    version: None,
                },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            }))
        })
        .collect::<Vec<_>>();

    (!document_changes.is_empty()).then(|| WorkspaceEdit {
        document_changes: Some(DocumentChanges::Operations(document_changes)),
        ..Default::default()
    })
}

fn code_action_for_import_fix(fix: ImportFix) -> Option<CodeActionOrCommand> {
    let edit = match fix.action {
        ImportFixAction::Edit(edit) => workspace_edit(vec![edit]),
        ImportFixAction::CreateModule { path } => create_file_workspace_edit(&path),
    };

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: fix.title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![to_diagnostic(fix.diagnostic)]),
        edit,
        is_preferred: Some(false),
        ..Default::default()
    }))
}

fn format_document_code_action(uri: &str) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title: "Run Zuzu formatter".to_string(),
        kind: Some(CodeActionKind::SOURCE),
        command: Some(LspCommand {
            title: "Run Zuzu formatter".to_string(),
            command: "zuzu.formatDocument".to_string(),
            arguments: Some(vec![json!(uri)]),
        }),
        is_preferred: Some(false),
        ..Default::default()
    })
}

fn create_file_workspace_edit(path: &Path) -> Option<WorkspaceEdit> {
    let uri = path_to_new_file_uri(path)?;
    Some(WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Operations(vec![
            DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                uri,
                options: Some(CreateFileOptions {
                    overwrite: Some(false),
                    ignore_if_exists: Some(true),
                }),
                annotation_id: None,
            })),
        ])),
        change_annotations: None,
    })
}

fn path_to_new_file_uri(path: &Path) -> Option<Uri> {
    url::Url::from_file_path(path)
        .ok()?
        .to_string()
        .parse()
        .ok()
}

fn to_signature_help(help: zuzu_analysis::SignatureHelp) -> SignatureHelp {
    SignatureHelp {
        signatures: vec![SignatureInformation {
            label: help.label,
            documentation: None,
            parameters: Some(
                help.parameters
                    .into_iter()
                    .map(|parameter| ParameterInformation {
                        label: ParameterLabel::Simple(parameter),
                        documentation: None,
                    })
                    .collect(),
            ),
            active_parameter: Some(help.active_parameter),
        }],
        active_signature: Some(0),
        active_parameter: Some(help.active_parameter),
    }
}

fn to_inlay_hint(hint: zuzu_analysis::InlayHint) -> InlayHint {
    InlayHint {
        position: to_position(hint.position),
        label: InlayHintLabel::String(hint.label),
        kind: Some(InlayHintKind::PARAMETER),
        text_edits: None,
        tooltip: None,
        padding_left: None,
        padding_right: Some(true),
        data: None,
    }
}

fn to_selection_range(range: zuzu_analysis::SelectionRange) -> LspSelectionRange {
    LspSelectionRange {
        range: to_range(range.range),
        parent: range
            .parent
            .map(|parent| Box::new(to_selection_range(*parent))),
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

fn to_completion_item_kind(kind: zuzu_analysis::CompletionKind) -> CompletionItemKind {
    match kind {
        zuzu_analysis::CompletionKind::Keyword => CompletionItemKind::KEYWORD,
        zuzu_analysis::CompletionKind::BuiltinStatement => CompletionItemKind::KEYWORD,
        zuzu_analysis::CompletionKind::Module => CompletionItemKind::MODULE,
        zuzu_analysis::CompletionKind::Function => CompletionItemKind::FUNCTION,
        zuzu_analysis::CompletionKind::Method => CompletionItemKind::METHOD,
        zuzu_analysis::CompletionKind::Class => CompletionItemKind::CLASS,
        zuzu_analysis::CompletionKind::Trait => CompletionItemKind::INTERFACE,
        zuzu_analysis::CompletionKind::Field => CompletionItemKind::FIELD,
        zuzu_analysis::CompletionKind::Variable => CompletionItemKind::VARIABLE,
        zuzu_analysis::CompletionKind::Parameter => CompletionItemKind::VARIABLE,
        zuzu_analysis::CompletionKind::Import => CompletionItemKind::MODULE,
    }
}

fn analysis_symbol_kind(kind: lsp_types::SymbolKind) -> Option<zuzu_analysis::SymbolKind> {
    if kind == lsp_types::SymbolKind::FUNCTION {
        Some(zuzu_analysis::SymbolKind::Function)
    } else if kind == lsp_types::SymbolKind::METHOD {
        Some(zuzu_analysis::SymbolKind::Method)
    } else if kind == lsp_types::SymbolKind::CLASS {
        Some(zuzu_analysis::SymbolKind::Class)
    } else if kind == lsp_types::SymbolKind::INTERFACE {
        Some(zuzu_analysis::SymbolKind::Trait)
    } else {
        None
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

fn from_range(range: Range) -> zuzu_analysis::Range {
    zuzu_analysis::Range::new(from_position(range.start), from_position(range.end))
}

fn parse_uri(uri: &str) -> Option<Uri> {
    uri.parse().ok()
}

#[derive(Debug, Clone)]
struct MetadataDependencyLink {
    module: String,
    range: Range,
}

#[derive(Debug, Clone)]
struct MetadataUrlLink {
    field: &'static str,
    target: String,
    range: Range,
}

#[derive(Debug, Clone)]
struct PodLink {
    target: String,
    range: Range,
}

fn metadata_dependency_links(text: &str) -> Vec<MetadataDependencyLink> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(dependencies) = value
        .get("dependencies")
        .and_then(|value| value.as_object())
    else {
        return Vec::new();
    };
    let Some(dependencies_offset) = find_json_string(text, 0, "dependencies") else {
        return Vec::new();
    };
    let Some(object_offset) = text[dependencies_offset..].find('{') else {
        return Vec::new();
    };
    let search_start = dependencies_offset + object_offset + 1;

    dependencies
        .keys()
        .filter_map(|dependency| {
            let string_start = find_json_string(text, search_start, dependency)?;
            let content_start = string_start + 1;
            let content_end = string_start + serde_json::to_string(dependency).ok()?.len() - 1;
            Some(MetadataDependencyLink {
                module: dependency.clone(),
                range: range_for_byte_span(text, content_start, content_end),
            })
        })
        .collect()
}

fn metadata_url_links(text: &str) -> Vec<MetadataUrlLink> {
    const URL_FIELDS: &[&str] = &["repo", "homepage", "documentation"];

    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(metadata) = value.as_object() else {
        return Vec::new();
    };

    URL_FIELDS
        .iter()
        .filter_map(|field| {
            let target = metadata.get(*field)?.as_str()?;
            if !is_external_url(target) {
                return None;
            }
            let field_start = find_json_string(text, 0, field)?;
            let value_start = find_json_string(
                text,
                field_start + serde_json::to_string(field).ok()?.len(),
                target,
            )?;
            let content_start = value_start + 1;
            let content_end = value_start + serde_json::to_string(target).ok()?.len() - 1;
            Some(MetadataUrlLink {
                field,
                target: target.to_string(),
                range: range_for_byte_span(text, content_start, content_end),
            })
        })
        .collect()
}

fn is_external_url(target: &str) -> bool {
    target.starts_with("https://") || target.starts_with("http://")
}

fn symbol_documentation_markdown(markdown: &str, symbol: &str) -> Option<String> {
    let lines: Vec<_> = markdown.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let Some((level, heading)) = markdown_heading(line) else {
            continue;
        };
        if !markdown_heading_matches_symbol(heading, symbol) {
            continue;
        }

        let end = lines
            .iter()
            .enumerate()
            .skip(index + 1)
            .find_map(|(next_index, next_line)| {
                let (next_level, _) = markdown_heading(next_line)?;
                (next_level <= level).then_some(next_index)
            })
            .unwrap_or(lines.len());
        let section = lines[index..end].join("\n").trim().to_string();
        if !section.is_empty() {
            return Some(section);
        }
    }
    None
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let line = line.trim_start();
    let level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let title = line.get(level..)?;
    if !title.starts_with(' ') {
        return None;
    }
    Some((level, title.trim().trim_end_matches('#').trim_end()))
}

fn markdown_heading_matches_symbol(heading: &str, symbol: &str) -> bool {
    let heading = heading.trim_matches('`');
    heading == symbol
        || heading
            .strip_prefix("function ")
            .is_some_and(|suffix| heading_suffix_matches_symbol(suffix, symbol))
        || heading
            .strip_prefix("method ")
            .is_some_and(|suffix| heading_suffix_matches_symbol(suffix, symbol))
        || heading
            .strip_prefix("class ")
            .is_some_and(|suffix| heading_suffix_matches_symbol(suffix, symbol))
}

fn heading_suffix_matches_symbol(suffix: &str, symbol: &str) -> bool {
    let Some(rest) = suffix.strip_prefix(symbol) else {
        return false;
    };
    rest.is_empty()
        || rest
            .chars()
            .next()
            .is_some_and(|character| !character.is_alphanumeric() && character != '_')
}

fn find_json_string(text: &str, start: usize, value: &str) -> Option<usize> {
    let needle = serde_json::to_string(value).ok()?;
    text.get(start..)?
        .find(&needle)
        .map(|offset| start + offset)
}

fn pod_document_links(
    text: &str,
    resolve_module_uri: impl Fn(&str) -> Option<String>,
) -> Vec<DocumentLink> {
    pod_links(text)
        .into_iter()
        .filter_map(|link| {
            if link.target.starts_with("http://") || link.target.starts_with("https://") {
                return Some(DocumentLink {
                    range: link.range,
                    target: Some(parse_uri(&link.target)?),
                    tooltip: Some(format!("Open POD link `{}`", link.target)),
                    data: None,
                });
            }

            let target = resolve_module_uri(&link.target)?;
            Some(DocumentLink {
                range: link.range,
                target: Some(parse_uri(&target)?),
                tooltip: Some(format!("Open POD module link `{}`", link.target)),
                data: None,
            })
        })
        .collect()
}

fn pod_links(text: &str) -> Vec<PodLink> {
    let mut links = Vec::new();
    let mut search_start = 0;
    while let Some(relative_start) = text[search_start..].find("L<") {
        let link_start = search_start + relative_start;
        let content_start = link_start + 2;
        let Some(relative_end) = text[content_start..].find('>') else {
            break;
        };
        let content_end = content_start + relative_end;
        if let Some(target) = pod_link_target(&text[content_start..content_end]) {
            links.push(PodLink {
                target,
                range: range_for_byte_span(text, link_start, content_end + 1),
            });
        }
        search_start = content_end + 1;
    }
    links
}

fn pod_link_target(content: &str) -> Option<String> {
    let target = content
        .rsplit_once('|')
        .map(|(_, target)| target)
        .unwrap_or(content);
    let target = target.trim();
    (!target.is_empty()).then(|| target.to_string())
}

fn range_for_byte_span(text: &str, start: usize, end: usize) -> Range {
    let line_offsets = lsp_line_offsets(text);
    Range {
        start: position_for_byte(text, &line_offsets, start),
        end: position_for_byte(text, &line_offsets, end),
    }
}

fn lsp_line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(index + 1);
        }
    }
    offsets
}

fn position_for_byte(text: &str, line_offsets: &[usize], byte: usize) -> Position {
    let byte = byte.min(text.len());
    let line = match line_offsets.binary_search(&byte) {
        Ok(line) => line,
        Err(line) => line.saturating_sub(1),
    };
    let line_start = line_offsets.get(line).copied().unwrap_or_default();
    Position {
        line: line as u32,
        character: text[line_start..byte]
            .chars()
            .map(|ch| ch.len_utf16() as u32)
            .sum(),
    }
}

fn char_offset_for_position(text: &Rope, position: Position) -> Option<usize> {
    let line_index = position.line as usize;
    if line_index >= text.len_lines() {
        return None;
    }
    let line_start = text.line_to_char(line_index);
    let line = text.line(line_index);
    let mut utf16_offset = 0;
    let mut char_offset = 0;
    for ch in line.chars() {
        if ch == '\r' || ch == '\n' {
            break;
        }
        if utf16_offset == position.character {
            return Some(line_start + char_offset);
        }
        utf16_offset += ch.len_utf16() as u32;
        if utf16_offset > position.character {
            return None;
        }
        char_offset += 1;
    }

    (utf16_offset == position.character).then_some(line_start + char_offset)
}

fn parse_tree(text: &str, old_tree: Option<&Tree>) -> Tree {
    let mut parser = TreeParser::new();
    parser
        .set_language(&tree_sitter_zuzu::language())
        .expect("tree-sitter-zuzu language should load");
    parser
        .parse(text, old_tree)
        .expect("tree-sitter should parse")
}

fn point_for_char(text: &Rope, char_index: usize) -> Point {
    let byte = text.char_to_byte(char_index.min(text.len_chars()));
    point_for_byte(text, byte)
}

fn point_for_byte(text: &Rope, byte: usize) -> Point {
    let snapshot = text.to_string();
    let mut row = 0;
    let mut column = 0;
    for byte in snapshot.as_bytes().iter().take(byte.min(snapshot.len())) {
        if *byte == b'\n' {
            row += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    Point { row, column }
}

fn point_after_text(start: Point, text: &str) -> Point {
    let mut point = start;
    for byte in text.bytes() {
        if byte == b'\n' {
            point.row += 1;
            point.column = 0;
        } else {
            point.column += 1;
        }
    }
    point
}

fn command_path_arg(arguments: &[serde_json::Value]) -> Option<PathBuf> {
    let value = arguments.first()?.as_str()?;
    if value.starts_with("file:") {
        url::Url::parse(value).ok()?.to_file_path().ok()
    } else {
        Some(PathBuf::from(value))
    }
}

fn distribution_command_path_arg(arguments: &[serde_json::Value]) -> Option<PathBuf> {
    let path = command_path_arg(arguments)?;
    if path.file_name().and_then(|name| name.to_str()) == Some("zuzu-distribution.json") {
        return path.parent().map(Path::to_path_buf).or(Some(path));
    }
    Some(path)
}

fn command_uri_arg(arguments: &[serde_json::Value]) -> Option<String> {
    let value = arguments.first()?.as_str()?;
    parse_uri(value)?;
    Some(value.to_string())
}

fn tool_output_value(output: ToolOutput) -> serde_json::Value {
    json!({
        "command": output.command,
        "status": output.status,
        "success": output.success,
        "stdout": output.stdout,
        "stderr": output.stderr,
    })
}

fn repl_instructions_value(toolchain: &Toolchain) -> serde_json::Value {
    let executable = toolchain
        .zuzu
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "zuzu".to_string());
    json!({
        "available": toolchain.zuzu.is_some(),
        "command": [executable, "-R"],
        "message": "Run this command in a terminal to start the ZuzuScript REPL.",
    })
}

fn uri_to_file_path(uri: &Uri) -> Option<PathBuf> {
    url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

fn code_lenses_for_uri(uri: &str) -> Vec<CodeLens> {
    if is_zuzu_test_uri(uri) {
        return vec![command_lens("Run Zuzu test", "zuzu.testFile", uri)];
    }
    if is_distribution_metadata_uri(uri) {
        return vec![
            command_lens("Run distribution tests", "zuzu.testWorkspace", uri),
            command_lens("Verify package", "zuzu.verifyPackage", uri),
            command_lens("Package health", "zuzu.packageReport", uri),
        ];
    }
    Vec::new()
}

fn command_lens(title: &str, command: &str, uri: &str) -> CodeLens {
    CodeLens {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        },
        command: Some(LspCommand {
            title: title.to_string(),
            command: command.to_string(),
            arguments: Some(vec![json!(uri)]),
        }),
        data: None,
    }
}

#[cfg(test)]
fn is_zuzu_document(uri: &str, text: &str) -> bool {
    matches!(
        classify_document(uri, text),
        DocumentKind::Script | DocumentKind::Module | DocumentKind::ExtensionlessScript
    )
}

fn classify_document_with_language(uri: &str, language_id: &str, text: &str) -> DocumentKind {
    match classify_document(uri, text) {
        DocumentKind::Other if language_id == "zuzu" => DocumentKind::Script,
        kind => kind,
    }
}

fn classify_document(uri: &str, text: &str) -> DocumentKind {
    let Some(path) = uri_to_path(uri) else {
        return DocumentKind::Other;
    };
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("zzs") => DocumentKind::Script,
        Some("zzm") => DocumentKind::Module,
        None if has_zuzu_shebang(text) => DocumentKind::ExtensionlessScript,
        _ if path.file_name().and_then(|name| name.to_str()) == Some("zuzu-distribution.json") => {
            DocumentKind::DistributionMetadata
        }
        _ => DocumentKind::Other,
    }
}

fn has_zuzu_shebang(text: &str) -> bool {
    text.lines()
        .next()
        .is_some_and(|line| line.starts_with("#!") && line.contains("zuzu"))
}

fn is_zuzu_test_uri(uri: &str) -> bool {
    let Some(path) = uri_to_path(uri) else {
        return false;
    };
    if path.extension().and_then(|extension| extension.to_str()) != Some("zzs") {
        return false;
    }
    path.components()
        .any(|component| component.as_os_str().to_string_lossy() == "tests")
}

fn is_distribution_metadata_uri(uri: &str) -> bool {
    uri_to_path(uri)
        .as_deref()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some("zuzu-distribution.json")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_extensionless_zuzu_documents_from_shebang() {
        assert!(is_zuzu_document(
            "file:///tmp/tool",
            "#!/usr/bin/env zuzu\nsay 1;\n"
        ));
        assert!(!is_zuzu_document("file:///tmp/README", "# Fixture notes\n"));
        assert!(is_zuzu_document("file:///tmp/demo.zzs", "say 1;\n"));
    }

    #[test]
    fn uses_language_id_for_untitled_zuzu_documents() {
        let document = DocumentSnapshot::new(
            "untitled:Untitled-1".to_string(),
            "zuzu".to_string(),
            Some(1),
            "say 1;\n".to_string(),
        );
        assert_eq!(document.kind, DocumentKind::Script);
        assert!(document.is_zuzu_document());
    }

    #[test]
    fn reads_workspace_trust_from_initialize_payload() {
        assert!(!initialize_workspace_trusted(&json!({
            "capabilities": {
                "workspace": {
                    "workspaceTrust": {
                        "trusted": false
                    }
                }
            }
        })));
        assert!(!initialize_workspace_trusted(&json!({
            "initializationOptions": {
                "workspaceTrust": {
                    "trusted": false
                }
            }
        })));
        assert!(initialize_workspace_trusted(&json!({
            "capabilities": {
                "workspace": {
                    "workspaceTrust": {
                        "supported": true
                    }
                }
            }
        })));
    }

    #[test]
    fn reads_server_settings_from_initialize_payload() {
        let root = PathBuf::from("/workspace/project");
        let settings = RawServerSettings::from_initialize_params(&json!({
            "initializationOptions": {
                "zuzu": {
                    "moduleRoots": ["modules-extra", "/opt/zuzu/modules"],
                    "runtimeParserDiagnostics": false
                }
            }
        }))
        .resolve(std::slice::from_ref(&root));

        assert_eq!(
            settings.module_roots,
            vec![
                root.join("modules-extra"),
                PathBuf::from("/opt/zuzu/modules")
            ]
        );
        assert!(!settings.runtime_parser_diagnostics);
    }

    #[test]
    fn reads_server_settings_from_configuration_payload() {
        let settings = RawServerSettings::from_configuration_params(&json!({
            "settings": {
                "zuzu": {
                    "module_roots": ["vendor/modules"],
                    "runtime_parser_diagnostics": true
                }
            }
        }))
        .expect("settings");

        assert_eq!(settings.module_roots, vec!["vendor/modules"]);
        assert_eq!(settings.runtime_parser_diagnostics, Some(true));
    }

    #[test]
    fn applies_incremental_document_changes_using_lsp_positions() {
        let mut document = DocumentSnapshot::new(
            "file:///tmp/demo.zzs".to_string(),
            "zuzu".to_string(),
            Some(1),
            "say \"hi\";\nsay \"bye\";\n".to_string(),
        );
        document
            .apply_change(TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 1,
                        character: 5,
                    },
                    end: Position {
                        line: 1,
                        character: 8,
                    },
                }),
                range_length: None,
                text: "ciao".to_string(),
            })
            .unwrap();
        assert_eq!(document.text(), "say \"hi\";\nsay \"ciao\";\n");
        assert_eq!(document.kind, DocumentKind::Script);
        assert!(!document.has_syntax_error());
    }

    #[test]
    fn applies_incremental_document_changes_with_utf16_offsets() {
        let mut document = DocumentSnapshot::new(
            "file:///tmp/demo.zzs".to_string(),
            "zuzu".to_string(),
            Some(1),
            "say \"🙂\";\n".to_string(),
        );
        document
            .apply_change(TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 5,
                    },
                    end: Position {
                        line: 0,
                        character: 7,
                    },
                }),
                range_length: None,
                text: "ok".to_string(),
            })
            .unwrap();
        assert_eq!(document.text(), "say \"ok\";\n");
        assert!(!document.has_syntax_error());
    }

    #[test]
    fn updates_incremental_tree_after_syntax_fix() {
        let mut document = DocumentSnapshot::new(
            "file:///tmp/demo.zzs".to_string(),
            "zuzu".to_string(),
            Some(1),
            "let x := ;\n".to_string(),
        );
        assert!(document.has_syntax_error());

        document
            .apply_change(TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 9,
                    },
                    end: Position {
                        line: 0,
                        character: 9,
                    },
                }),
                range_length: None,
                text: "1".to_string(),
            })
            .unwrap();

        assert_eq!(document.text(), "let x := 1;\n");
        assert!(!document.has_syntax_error());
    }

    #[test]
    fn reports_missing_toolchain_hints_without_blocking_core_features() {
        let diagnostics = toolchain_diagnostics("file:///tmp/demo.zzs", &Toolchain::default());
        let codes: Vec<_> = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect();
        assert!(codes.contains(&"missing-formatter"));
        assert!(codes.contains(&"missing-doc-renderer"));
        assert!(!codes.contains(&"missing-test-runner"));
        assert!(diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity == AnalysisSeverity::Hint));

        let test_diagnostics =
            toolchain_diagnostics("file:///tmp/tests/example.zzs", &Toolchain::default());
        assert!(test_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "missing-test-runner"));

        let package_diagnostics = package_toolchain_diagnostics(&Toolchain::default());
        assert!(package_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "missing-package-verifier"));
    }

    #[test]
    fn module_roots_include_discovered_installed_modules() {
        let configured = PathBuf::from("/workspace/vendor/modules");
        let runtime = PathBuf::from("/runtime/modules");
        let installed = PathBuf::from("/home/example/.zuzu/modules");
        let settings = ServerSettings {
            module_roots: vec![configured.clone()],
            runtime_parser_diagnostics: true,
        };
        let toolchain = Toolchain {
            module_search_paths: vec![runtime.clone(), installed.clone()],
            installed_modules: vec![installed.clone()],
            ..Default::default()
        };

        assert_eq!(
            configured_module_roots(&settings, &toolchain),
            vec![configured, runtime, installed]
        );
    }
}
