use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, Point, Tree, TreeCursor};
use url::Url;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: DiagnosticSeverity,
    pub source: &'static str,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Module,
    Function,
    Method,
    Class,
    Trait,
    Field,
    Variable,
    Parameter,
    Import,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    pub detail: Option<String>,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoldingRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub kind: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionRange {
    pub range: Range,
    pub parent: Option<Box<SelectionRange>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticToken {
    pub range: Range,
    pub kind: SymbolKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallHierarchyItem {
    pub name: String,
    pub kind: SymbolKind,
    pub uri: String,
    pub range: Range,
    pub selection_range: Range,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingCall {
    pub from: CallHierarchyItem,
    pub from_ranges: Vec<Range>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingCall {
    pub to: CallHierarchyItem,
    pub from_ranges: Vec<Range>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeHierarchyItem {
    pub name: String,
    pub kind: SymbolKind,
    pub uri: String,
    pub range: Range,
    pub selection_range: Range,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureHelp {
    pub label: String,
    pub parameters: Vec<String>,
    pub active_parameter: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlayHint {
    pub position: Position,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hover {
    pub range: Range,
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTextEdit {
    pub uri: String,
    pub edit: TextEdit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentLink {
    pub range: Range,
    pub target: String,
    pub tooltip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportFix {
    pub title: String,
    pub diagnostic: Diagnostic,
    pub action: ImportFixAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportFixAction {
    Edit(WorkspaceTextEdit),
    CreateModule { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleTarget {
    pub module: String,
    pub range: Range,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDocumentationTarget {
    pub name: String,
    pub range: Range,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageReport {
    pub root: Option<String>,
    pub dependencies: Vec<String>,
    pub module_roots: Vec<String>,
    pub diagnostics: Vec<PackageDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDiagnostic {
    pub uri: String,
    pub range: Range,
    pub severity: DiagnosticSeverity,
    pub source: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyGraph {
    pub nodes: Vec<DependencyNode>,
    pub edges: Vec<DependencyEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyNode {
    pub id: String,
    pub uri: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub range: Range,
    pub resolved: bool,
    pub target_uri: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Document {
    uri: String,
    text: String,
    line_offsets: Vec<usize>,
    tree: Tree,
    diagnostics: Vec<Diagnostic>,
    symbols: Vec<Symbol>,
    imports: Vec<Import>,
    signatures: Vec<CallableSignature>,
    type_relations: Vec<TypeRelation>,
}

#[derive(Debug, Clone)]
struct Import {
    module: String,
    module_range: Range,
    try_range: Option<Range>,
    statement_range: Range,
    delete_range: Range,
    imported_names: Vec<ImportedName>,
}

#[derive(Debug, Clone)]
struct ImportedName {
    name: String,
}

#[derive(Debug, Clone)]
struct CallableSignature {
    name: String,
    label: String,
    parameters: Vec<String>,
}

#[derive(Debug, Clone)]
struct TypeRelation {
    subtype: String,
    supertype: String,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    roots: Vec<PathBuf>,
    module_roots: Vec<PathBuf>,
    distributions: Vec<Distribution>,
    metadata_diagnostics: BTreeMap<String, Vec<Diagnostic>>,
    known_modules: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct Distribution {
    root: PathBuf,
    dependencies: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct Analyzer {
    workspace: Workspace,
    documents: BTreeMap<String, Document>,
    indexed_documents: BTreeMap<String, Document>,
}

impl Analyzer {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self::with_module_roots(roots, Vec::new())
    }

    pub fn with_module_roots(roots: Vec<PathBuf>, runtime_module_roots: Vec<PathBuf>) -> Self {
        let workspace = Workspace::with_module_roots(roots, runtime_module_roots);
        let indexed_documents = index_workspace_documents(&workspace);
        Self {
            workspace,
            documents: BTreeMap::new(),
            indexed_documents,
        }
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn upsert_document(
        &mut self,
        uri: impl Into<String>,
        text: impl Into<String>,
    ) -> Vec<Diagnostic> {
        let uri = uri.into();
        let document = parse_workspace_document(&self.workspace, uri.clone(), text.into());
        let diagnostics = document.diagnostics.clone();
        self.documents.insert(uri, document);
        diagnostics
    }

    pub fn remove_document(&mut self, uri: &str) {
        self.documents.remove(uri);
    }

    pub fn document(&self, uri: &str) -> Option<&Document> {
        self.documents
            .get(uri)
            .or_else(|| self.indexed_documents.get(uri))
    }

    pub fn diagnostics(&self, uri: &str) -> Vec<Diagnostic> {
        self.document(uri)
            .map(|document| document.diagnostics.clone())
            .unwrap_or_else(|| self.workspace.metadata_diagnostics(uri))
    }

    pub fn document_symbols(&self, uri: &str) -> Vec<Symbol> {
        self.document(uri)
            .map(|document| document.symbols.clone())
            .unwrap_or_default()
    }

    pub fn workspace_symbols(&self, query: &str) -> Vec<Symbol> {
        let query = query.to_lowercase();
        self.all_documents()
            .into_iter()
            .flat_map(|document| document.symbols.iter())
            .filter(|symbol| query.is_empty() || symbol.name.to_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    pub fn folding_ranges(&self, uri: &str) -> Vec<FoldingRange> {
        self.document(uri)
            .map(Document::folding_ranges)
            .unwrap_or_default()
    }

    pub fn selection_ranges(&self, uri: &str, positions: &[Position]) -> Vec<SelectionRange> {
        let Some(document) = self.document(uri) else {
            return Vec::new();
        };
        positions
            .iter()
            .filter_map(|position| document.selection_range(*position))
            .collect()
    }

    pub fn semantic_tokens(&self, uri: &str) -> Vec<SemanticToken> {
        let Some(document) = self.document(uri) else {
            return Vec::new();
        };
        document
            .symbols
            .iter()
            .filter(|symbol| symbol.selection_range.start.line == symbol.selection_range.end.line)
            .map(|symbol| SemanticToken {
                range: symbol.selection_range,
                kind: symbol.kind.clone(),
            })
            .collect()
    }

    pub fn prepare_call_hierarchy(&self, uri: &str, position: Position) -> Vec<CallHierarchyItem> {
        let Some(document) = self.document(uri) else {
            return Vec::new();
        };
        document
            .symbols
            .iter()
            .filter(|symbol| is_callable_kind(&symbol.kind))
            .filter(|symbol| position_in_range(position, symbol.selection_range))
            .map(call_hierarchy_item)
            .collect()
    }

    pub fn outgoing_calls(&self, item: &CallHierarchyItem) -> Vec<OutgoingCall> {
        let Some(document) = self.document(&item.uri) else {
            return Vec::new();
        };
        let Some(caller) = document.callable_by_selection_range(item.selection_range) else {
            return Vec::new();
        };

        let mut calls: BTreeMap<CallKey, (CallHierarchyItem, Vec<Range>)> = BTreeMap::new();
        for call in document.call_sites_in_range(caller.range) {
            let Some(target) = self.callable_named(&call.name) else {
                continue;
            };
            let key = call_key(&target);
            calls
                .entry(key)
                .or_insert_with(|| (target, Vec::new()))
                .1
                .push(call.range);
        }

        calls
            .into_values()
            .map(|(to, from_ranges)| OutgoingCall { to, from_ranges })
            .collect()
    }

    pub fn incoming_calls(&self, item: &CallHierarchyItem) -> Vec<IncomingCall> {
        let mut calls: BTreeMap<CallKey, (CallHierarchyItem, Vec<Range>)> = BTreeMap::new();
        for document in self.all_documents() {
            for call in document.call_sites_to(&item.name) {
                let Some(caller) = document.callable_containing(call.range.start) else {
                    continue;
                };
                let caller = call_hierarchy_item(caller);
                let key = call_key(&caller);
                calls
                    .entry(key)
                    .or_insert_with(|| (caller, Vec::new()))
                    .1
                    .push(call.range);
            }
        }

        calls
            .into_values()
            .map(|(from, from_ranges)| IncomingCall { from, from_ranges })
            .collect()
    }

    fn callable_named(&self, name: &str) -> Option<CallHierarchyItem> {
        self.all_documents()
            .into_iter()
            .flat_map(|document| document.symbols.iter())
            .filter(|symbol| is_callable_kind(&symbol.kind))
            .find(|symbol| symbol.name == name)
            .map(call_hierarchy_item)
    }

    pub fn prepare_type_hierarchy(&self, uri: &str, position: Position) -> Vec<TypeHierarchyItem> {
        let Some(document) = self.document(uri) else {
            return Vec::new();
        };
        document
            .symbols
            .iter()
            .filter(|symbol| is_type_kind(&symbol.kind))
            .filter(|symbol| position_in_range(position, symbol.selection_range))
            .map(type_hierarchy_item)
            .collect()
    }

    pub fn supertypes(&self, item: &TypeHierarchyItem) -> Vec<TypeHierarchyItem> {
        let Some(document) = self.document(&item.uri) else {
            return Vec::new();
        };
        let mut types = BTreeMap::new();
        for relation in &document.type_relations {
            if relation.subtype == item.name {
                if let Some(supertype) = self.type_named(&relation.supertype) {
                    types.insert(type_key(&supertype), supertype);
                }
            }
        }
        types.into_values().collect()
    }

    pub fn subtypes(&self, item: &TypeHierarchyItem) -> Vec<TypeHierarchyItem> {
        let mut types = BTreeMap::new();
        for document in self.all_documents() {
            for relation in &document.type_relations {
                if relation.supertype == item.name {
                    if let Some(subtype) = self.type_named(&relation.subtype) {
                        types.insert(type_key(&subtype), subtype);
                    }
                }
            }
        }
        types.into_values().collect()
    }

    fn type_named(&self, name: &str) -> Option<TypeHierarchyItem> {
        self.all_documents()
            .into_iter()
            .flat_map(|document| document.symbols.iter())
            .filter(|symbol| is_type_kind(&symbol.kind))
            .find(|symbol| symbol.name == name)
            .map(type_hierarchy_item)
    }

    pub fn hover(&self, uri: &str, position: Position) -> Option<Hover> {
        let document = self.document(uri)?;

        if let Some(word) = document.word_at(position) {
            if let Some(description) = describe_keyword_or_builtin(&word) {
                return Some(Hover {
                    range: document.word_range(position)?,
                    markdown: description.to_string(),
                });
            }

            if let Some(description) = describe_word_operator(&word) {
                return Some(Hover {
                    range: document.word_range(position)?,
                    markdown: description.to_string(),
                });
            }

            if let Some(symbol) = document
                .symbol_at(position)
                .or_else(|| document.symbols.iter().find(|symbol| symbol.name == word))
            {
                return Some(Hover {
                    range: symbol.selection_range,
                    markdown: format_symbol(symbol),
                });
            }
        }

        document.operator_at(position).map(|operator| Hover {
            range: operator.range,
            markdown: operator.description.to_string(),
        })
    }

    pub fn completions(&self, uri: &str, position: Position) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        for keyword in KEYWORDS {
            items.push(CompletionItem {
                label: (*keyword).to_string(),
                detail: Some("ZuzuScript keyword".to_string()),
            });
        }

        for builtin in BUILTIN_STATEMENTS {
            items.push(CompletionItem {
                label: (*builtin).to_string(),
                detail: Some("ZuzuScript builtin statement".to_string()),
            });
        }

        if let Some(document) = self.document(uri) {
            for symbol in document.visible_symbols(position) {
                items.push(CompletionItem {
                    label: symbol.name.clone(),
                    detail: Some(format!("{:?}", symbol.kind).to_lowercase()),
                });
            }
        }

        for module in self.workspace.known_modules() {
            items.push(CompletionItem {
                label: module.to_string(),
                detail: Some("ZuzuScript module".to_string()),
            });
        }

        items.sort_by(|a, b| a.label.cmp(&b.label).then(a.detail.cmp(&b.detail)));
        items.dedup_by(|a, b| a.label == b.label && a.detail == b.detail);
        items
    }

    pub fn signature_help(&self, uri: &str, position: Position) -> Option<SignatureHelp> {
        let document = self.document(uri)?;
        let context = document.call_context(position)?;
        let signature = self.signature_for(document, &context.name)?;

        Some(SignatureHelp {
            label: signature.label.clone(),
            parameters: signature.parameters.clone(),
            active_parameter: context
                .active_parameter
                .min(signature.parameters.len().saturating_sub(1) as u32),
        })
    }

    pub fn inlay_hints(&self, uri: &str, range: Range) -> Vec<InlayHint> {
        let Some(document) = self.document(uri) else {
            return Vec::new();
        };

        document
            .call_argument_positions()
            .into_iter()
            .filter(|call| ranges_overlap(range, call.range))
            .filter_map(|call| {
                let signature = self.signature_for(document, &call.name)?;
                Some(
                    call.arguments
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, position)| {
                            let label = signature.parameters.get(index)?;
                            Some(InlayHint {
                                position,
                                label: format!("{label}:"),
                            })
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .flatten()
            .collect()
    }

    fn signature_for<'a>(
        &'a self,
        document: &'a Document,
        name: &str,
    ) -> Option<&'a CallableSignature> {
        document
            .signatures
            .iter()
            .find(|signature| signature.name == name)
            .or_else(|| {
                self.all_documents()
                    .into_iter()
                    .flat_map(|document| document.signatures.iter())
                    .find(|signature| signature.name == name)
            })
    }

    pub fn definition(&self, uri: &str, position: Position) -> Option<Location> {
        let document = self.document(uri)?;
        let word = document.word_at(position)?;
        document
            .symbols
            .iter()
            .find(|symbol| symbol.name == word)
            .or_else(|| {
                self.all_documents()
                    .into_iter()
                    .flat_map(|candidate| candidate.symbols.iter())
                    .find(|symbol| symbol.name == word)
            })
            .map(|symbol| Location {
                uri: symbol.uri.clone(),
                range: symbol.selection_range,
            })
    }

    pub fn references(
        &self,
        uri: &str,
        position: Position,
        include_declaration: bool,
    ) -> Vec<Location> {
        let Some(document) = self.document(uri) else {
            return Vec::new();
        };
        let Some(word) = document.word_at(position) else {
            return Vec::new();
        };
        let declaration_range = self
            .definition(uri, position)
            .map(|definition| definition.range);

        let mut locations = Vec::new();
        for document in self.all_documents() {
            for range in document.word_ranges(&word) {
                if !include_declaration && Some(range) == declaration_range {
                    continue;
                }
                locations.push(Location {
                    uri: document.uri.clone(),
                    range,
                });
            }
        }
        locations
    }

    pub fn prepare_rename(&self, uri: &str, position: Position) -> Option<Range> {
        let document = self.document(uri)?;
        let word = document.word_at(position)?;
        let range = document.word_range(position)?;
        is_identifier(&word).then_some(range)
    }

    pub fn rename(
        &self,
        uri: &str,
        position: Position,
        new_name: &str,
    ) -> Result<Vec<WorkspaceTextEdit>, RenameError> {
        if !is_identifier(new_name) {
            return Err(RenameError::InvalidIdentifier(new_name.to_string()));
        }

        let Some(document) = self.document(uri) else {
            return Ok(Vec::new());
        };
        if self.prepare_rename(uri, position).is_none() {
            return Ok(Vec::new());
        }
        let Some(old_name) = document.word_at(position) else {
            return Ok(Vec::new());
        };

        Ok(self
            .references(uri, position, true)
            .into_iter()
            .filter(|location| {
                self.document(&location.uri)
                    .and_then(|document| document.text_for_range(location.range))
                    .is_some_and(|text| text == old_name)
            })
            .map(|location| WorkspaceTextEdit {
                uri: location.uri,
                edit: TextEdit {
                    range: location.range,
                    new_text: new_name.to_string(),
                },
            })
            .collect())
    }

    pub fn document_links(&self, uri: &str) -> Vec<DocumentLink> {
        let Some(document) = self.document(uri) else {
            return Vec::new();
        };
        document
            .imports
            .iter()
            .filter_map(|import| {
                let target = self.workspace.resolve_module_uri(&import.module)?;
                Some(DocumentLink {
                    range: import.module_range,
                    target,
                    tooltip: Some(format!("Open module `{}`", import.module)),
                })
            })
            .collect()
    }

    pub fn module_target_at(&self, uri: &str, position: Position) -> Option<ModuleTarget> {
        let document = self.document(uri)?;
        document.imports.iter().find_map(|import| {
            if !position_in_range(position, import.module_range) {
                return None;
            }
            Some(ModuleTarget {
                module: import.module.clone(),
                range: import.module_range,
                path: self.workspace.resolve_module_path(&import.module)?,
            })
        })
    }

    pub fn symbol_documentation_target_at(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<SymbolDocumentationTarget> {
        let document = self.document(uri)?;
        let name = document.word_at(position)?;
        let range = document.word_range(position)?;
        let definition = self.definition(uri, position)?;
        Some(SymbolDocumentationTarget {
            name,
            range,
            path: uri_to_path(&definition.uri)?,
        })
    }

    pub fn import_fixes(&self, uri: &str, range: Range) -> Vec<ImportFix> {
        let Some(document) = self.document(uri) else {
            return Vec::new();
        };
        let mut fixes: Vec<ImportFix> = document
            .imports
            .iter()
            .filter(|import| ranges_overlap(range, import.statement_range))
            .flat_map(|import| {
                if import.try_range.is_none() && !self.workspace.resolve_module(&import.module) {
                    let diagnostic = self.workspace.unresolved_import_diagnostic(import);
                    let mut fixes = vec![ImportFix {
                        title: format!("Remove unresolved import `{}`", import.module),
                        diagnostic: diagnostic.clone(),
                        action: ImportFixAction::Edit(WorkspaceTextEdit {
                            uri: uri.to_string(),
                            edit: TextEdit {
                                range: import.delete_range,
                                new_text: String::new(),
                            },
                        }),
                    }];
                    if let Some(path) = self.workspace.new_workspace_module_path(&import.module) {
                        fixes.push(ImportFix {
                            title: format!("Create module `{}`", import.module),
                            diagnostic,
                            action: ImportFixAction::CreateModule { path },
                        });
                    }
                    return fixes;
                }

                if let Some(diagnostic) = self
                    .workspace
                    .missing_dependency_diagnostic(&document.uri, import)
                {
                    return self
                        .workspace
                        .add_dependency_edit(&document.uri, &import.module)
                        .map(|edit| {
                            vec![ImportFix {
                                title: format!(
                                    "Add `{}` to zuzu-distribution.json dependencies",
                                    import.module
                                ),
                                diagnostic,
                                action: ImportFixAction::Edit(edit),
                            }]
                        })
                        .unwrap_or_default();
                }

                if import.imported_names.is_empty()
                    || !self.workspace.resolve_module(&import.module)
                    || !document.import_is_unused(import)
                {
                    return Vec::new();
                }

                vec![ImportFix {
                    title: format!("Remove unused import `{}`", import.module),
                    diagnostic: Diagnostic {
                        range: import.statement_range,
                        severity: DiagnosticSeverity::Warning,
                        source: "zuzu-semantic",
                        code: "unused-import",
                        message: format!("Imported symbols from `{}` are unused", import.module),
                    },
                    action: ImportFixAction::Edit(WorkspaceTextEdit {
                        uri: uri.to_string(),
                        edit: TextEdit {
                            range: import.delete_range,
                            new_text: String::new(),
                        },
                    }),
                }]
            })
            .collect();
        fixes.extend(self.missing_import_fixes(document, range));
        fixes
    }

    fn missing_import_fixes(&self, document: &Document, range: Range) -> Vec<ImportFix> {
        document
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code == "undefined-local" && ranges_overlap(range, diagnostic.range)
            })
            .filter_map(|diagnostic| {
                let name = document.text_for_range(diagnostic.range)?;
                let module = self.unambiguous_exporting_module(name, &document.uri)?;
                let edit = document.add_import_edit(&module, name)?;
                Some(ImportFix {
                    title: format!("Import `{name}` from `{module}`"),
                    diagnostic: diagnostic.clone(),
                    action: ImportFixAction::Edit(WorkspaceTextEdit {
                        uri: document.uri.clone(),
                        edit,
                    }),
                })
            })
            .collect()
    }

    fn unambiguous_exporting_module(&self, name: &str, current_uri: &str) -> Option<String> {
        let mut modules = BTreeSet::new();
        for document in self.all_documents() {
            if document.uri == current_uri || !document.exports_name(name) {
                continue;
            }
            if let Some(module) = self.workspace.module_name_for_uri(&document.uri) {
                modules.insert(module);
            }
        }
        (modules.len() == 1)
            .then(|| modules.into_iter().next())
            .flatten()
    }

    pub fn package_report(&self, path: Option<&Path>) -> PackageReport {
        let distribution = path
            .and_then(|path| self.workspace.distribution_for_path(path))
            .or_else(|| self.workspace.distributions.first());
        let root = distribution.map(|distribution| path_display(&distribution.root));
        let dependencies = distribution
            .map(|distribution| distribution.dependencies.iter().cloned().collect())
            .unwrap_or_default();
        let module_roots = self
            .workspace
            .module_roots()
            .iter()
            .map(|path| path_display(path))
            .collect();
        let diagnostics = self
            .all_documents()
            .into_iter()
            .flat_map(|document| {
                document
                    .diagnostics
                    .iter()
                    .map(|diagnostic| package_diagnostic(&document.uri, diagnostic))
            })
            .chain(
                self.workspace
                    .metadata_diagnostics
                    .iter()
                    .flat_map(|(uri, diagnostics)| {
                        diagnostics
                            .iter()
                            .map(|diagnostic| package_diagnostic(uri, diagnostic))
                    }),
            )
            .collect();

        PackageReport {
            root,
            dependencies,
            module_roots,
            diagnostics,
        }
    }

    pub fn dependency_graph(&self) -> DependencyGraph {
        let mut nodes = BTreeMap::new();
        let mut edges = Vec::new();

        for document in self.all_documents() {
            let from = self.dependency_node_id(document);
            let from_kind = if self.workspace.module_name_for_uri(&document.uri).is_some() {
                "module"
            } else {
                "document"
            };
            nodes.entry(from.clone()).or_insert_with(|| DependencyNode {
                id: from.clone(),
                uri: Some(document.uri.clone()),
                kind: from_kind.to_string(),
            });

            for import in &document.imports {
                let target_uri = self.workspace.resolve_module_uri(&import.module);
                let resolved = target_uri.is_some();
                nodes
                    .entry(import.module.clone())
                    .or_insert_with(|| DependencyNode {
                        id: import.module.clone(),
                        uri: target_uri.clone(),
                        kind: if resolved { "module" } else { "unresolved" }.to_string(),
                    });
                edges.push(DependencyEdge {
                    from: from.clone(),
                    to: import.module.clone(),
                    range: import.module_range,
                    resolved,
                    target_uri,
                });
            }
        }

        edges.sort_by(|left, right| {
            left.from
                .cmp(&right.from)
                .then_with(|| left.to.cmp(&right.to))
                .then_with(|| compare_positions(left.range.start, right.range.start))
        });

        DependencyGraph {
            nodes: nodes.into_values().collect(),
            edges,
        }
    }

    fn dependency_node_id(&self, document: &Document) -> String {
        self.workspace
            .module_name_for_uri(&document.uri)
            .unwrap_or_else(|| document.uri.clone())
    }

    fn all_documents(&self) -> Vec<&Document> {
        let mut documents: Vec<&Document> = self.documents.values().collect();
        let open_paths: BTreeSet<PathBuf> = self
            .documents
            .keys()
            .filter_map(|uri| canonical_uri_path(uri))
            .collect();
        documents.extend(
            self.indexed_documents
                .iter()
                .filter(|(uri, _)| {
                    !self.documents.contains_key(*uri)
                        && canonical_uri_path(uri).is_none_or(|path| !open_paths.contains(&path))
                })
                .map(|(_, document)| document),
        );
        documents
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameError {
    InvalidIdentifier(String),
}

impl Workspace {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self::with_module_roots(roots, Vec::new())
    }

    pub fn with_module_roots(mut roots: Vec<PathBuf>, runtime_module_roots: Vec<PathBuf>) -> Self {
        roots.sort();
        roots.dedup();

        let mut module_roots = Vec::new();
        for root in &roots {
            module_roots.push(root.join("modules"));

            if let Some(distribution_root) = find_distribution_root(root) {
                module_roots.push(distribution_root.join("modules"));
                module_roots.push(distribution_root.join("inc"));
            }
        }

        if let Some(paths) = env::var_os("ZUZULIB") {
            module_roots.extend(env::split_paths(&paths));
        }

        if runtime_module_roots.is_empty() {
            module_roots.extend(fallback_runtime_module_roots());
        } else {
            module_roots.extend(runtime_module_roots);
        }
        module_roots.retain(|path| path.is_dir());
        dedup_paths(&mut module_roots);

        let (distributions, metadata_diagnostics) = collect_distributions(&roots);
        let known_modules = scan_modules(&module_roots);

        Self {
            roots,
            module_roots,
            distributions,
            metadata_diagnostics,
            known_modules,
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn module_roots(&self) -> &[PathBuf] {
        &self.module_roots
    }

    pub fn known_modules(&self) -> impl Iterator<Item = &str> {
        self.known_modules.iter().map(String::as_str)
    }

    fn metadata_diagnostics(&self, uri: &str) -> Vec<Diagnostic> {
        self.metadata_diagnostics
            .get(uri)
            .cloned()
            .unwrap_or_default()
    }

    pub fn resolve_module(&self, module: &str) -> bool {
        self.resolve_module_path(module).is_some()
    }

    pub fn resolve_module_path(&self, module: &str) -> Option<PathBuf> {
        self.module_roots.iter().find_map(|root| {
            ["zzm", "zzs"]
                .into_iter()
                .map(|extension| root.join(format!("{}.{}", module, extension)))
                .find(|path| path.is_file())
        })
    }

    pub fn resolve_module_uri(&self, module: &str) -> Option<String> {
        path_to_uri(&self.resolve_module_path(module)?)
    }

    fn module_name_for_uri(&self, uri: &str) -> Option<String> {
        let path = uri_to_path(uri)?;
        for root in &self.module_roots {
            if let Some(module) = module_name_under_root(&path, root) {
                return Some(module);
            }
        }

        let path = fs::canonicalize(path).ok()?;
        for root in &self.module_roots {
            let root = fs::canonicalize(root).ok()?;
            if let Some(module) = module_name_under_root(&path, &root) {
                return Some(module);
            }
        }
        None
    }

    fn unresolved_import_diagnostic(&self, import: &Import) -> Diagnostic {
        Diagnostic {
            range: import.module_range,
            severity: DiagnosticSeverity::Error,
            source: "zuzu-module",
            code: "unresolved-import",
            message: format!(
                "Could not resolve module `{}`. {}",
                import.module,
                self.module_resolution_explanation()
            ),
        }
    }

    fn module_resolution_explanation(&self) -> String {
        if self.module_roots.is_empty() {
            return "No module search paths are available.".to_string();
        }
        format!(
            "Searched module roots: {}.",
            self.module_roots
                .iter()
                .map(|path| format!("`{}`", path_display(path)))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    pub fn new_workspace_module_path(&self, module: &str) -> Option<PathBuf> {
        let root = self.roots.first()?;
        let module_root = find_distribution_root(root)
            .map(|distribution_root| distribution_root.join("modules"))
            .unwrap_or_else(|| root.join("modules"));
        Some(module_root.join(format!("{module}.zzm")))
    }

    fn source_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for root in &self.roots {
            for name in ["modules", "inc", "scripts", "tests"] {
                roots.push(root.join(name));
            }
            if let Some(distribution_root) = find_distribution_root(root) {
                for name in ["modules", "inc", "scripts", "tests"] {
                    roots.push(distribution_root.join(name));
                }
            }
        }
        roots.retain(|path| path.is_dir());
        dedup_paths(&mut roots);
        roots
    }

    fn missing_dependency_diagnostic(
        &self,
        document_uri: &str,
        import: &Import,
    ) -> Option<Diagnostic> {
        let document_path = uri_to_path(document_uri)?;
        let distribution = self.distribution_for_path(&document_path)?;
        let resolved_path = self.resolve_module_path(&import.module)?;
        if self.module_path_is_local_to_distribution(distribution, &resolved_path) {
            return None;
        }
        if is_stdlib_module(&import.module) {
            return None;
        }
        if distribution
            .dependencies
            .iter()
            .any(|dependency| dependency_covers_module(dependency, &import.module))
        {
            return None;
        }

        Some(Diagnostic {
            range: import.module_range,
            severity: DiagnosticSeverity::Warning,
            source: "zuzu-package",
            code: "missing-dependency",
            message: format!(
                "Import `{}` is not listed in zuzu-distribution.json dependencies",
                import.module
            ),
        })
    }

    fn add_dependency_edit(&self, document_uri: &str, module: &str) -> Option<WorkspaceTextEdit> {
        let document_path = uri_to_path(document_uri)?;
        let distribution = self.distribution_for_path(&document_path)?;
        let metadata_path = distribution.root.join("zuzu-distribution.json");
        let metadata_uri = path_to_uri(&metadata_path)?;
        let text = fs::read_to_string(&metadata_path).ok()?;
        let mut value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
        let object = value.as_object_mut()?;
        let dependencies = object
            .entry("dependencies")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let dependencies = dependencies.as_object_mut()?;
        if dependencies
            .keys()
            .any(|dependency| dependency_covers_module(dependency, module))
        {
            return None;
        }
        dependencies.insert(
            module.to_string(),
            serde_json::Value::String("0".to_string()),
        );
        let mut new_text = serde_json::to_string_pretty(&value).ok()?;
        new_text.push('\n');

        Some(WorkspaceTextEdit {
            uri: metadata_uri,
            edit: TextEdit {
                range: full_text_range(&text),
                new_text,
            },
        })
    }

    fn distribution_for_path(&self, path: &Path) -> Option<&Distribution> {
        self.distributions
            .iter()
            .filter(|distribution| path.starts_with(&distribution.root))
            .max_by_key(|distribution| distribution.root.components().count())
    }

    fn module_path_is_local_to_distribution(
        &self,
        distribution: &Distribution,
        path: &Path,
    ) -> bool {
        path.starts_with(distribution.root.join("modules"))
            || path.starts_with(distribution.root.join("inc"))
    }
}

fn index_workspace_documents(workspace: &Workspace) -> BTreeMap<String, Document> {
    let mut documents = BTreeMap::new();
    for root in workspace.source_roots() {
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !is_zuzu_source_file(path) {
                continue;
            }
            let Some(uri) = path_to_uri(path) else {
                continue;
            };
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            documents.insert(uri.clone(), parse_workspace_document(workspace, uri, text));
        }
    }
    documents
}

fn collect_distributions(
    roots: &[PathBuf],
) -> (Vec<Distribution>, BTreeMap<String, Vec<Diagnostic>>) {
    let mut distribution_roots: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| find_distribution_root(root))
        .collect();
    distribution_roots.sort();
    distribution_roots.dedup();
    let mut distributions = Vec::new();
    let mut diagnostics = BTreeMap::new();
    for root in distribution_roots {
        let (dependencies, metadata_diagnostics) = distribution_metadata(&root);
        if !metadata_diagnostics.is_empty() {
            if let Some(uri) = path_to_uri(&root.join("zuzu-distribution.json")) {
                diagnostics.insert(uri, metadata_diagnostics);
            }
        }
        distributions.push(Distribution { root, dependencies });
    }
    (distributions, diagnostics)
}

fn distribution_metadata(root: &Path) -> (BTreeSet<String>, Vec<Diagnostic>) {
    let metadata_path = root.join("zuzu-distribution.json");
    let Ok(text) = fs::read_to_string(&metadata_path) else {
        return (
            BTreeSet::new(),
            vec![metadata_diagnostic(
                "metadata-unreadable",
                "Could not read zuzu-distribution.json",
                Range::new(Position::new(0, 0), Position::new(0, 0)),
            )],
        );
    };
    distribution_metadata_from_text(&text)
}

pub fn distribution_metadata_diagnostics(text: &str) -> Vec<Diagnostic> {
    distribution_metadata_from_text(text).1
}

fn distribution_metadata_from_text(text: &str) -> (BTreeSet<String>, Vec<Diagnostic>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return (
            BTreeSet::new(),
            vec![metadata_diagnostic(
                "metadata-invalid-json",
                "zuzu-distribution.json is not valid JSON",
                full_text_range(text),
            )],
        );
    };
    let Some(metadata) = value.as_object() else {
        return (
            BTreeSet::new(),
            vec![metadata_diagnostic(
                "metadata-root-not-object",
                "zuzu-distribution.json must contain a JSON object",
                full_text_range(text),
            )],
        );
    };
    let mut diagnostics = Vec::new();
    for required in ["name", "version", "author", "license"] {
        if metadata
            .get(required)
            .and_then(|value| value.as_str())
            .is_none_or(|value| value.trim().is_empty())
        {
            diagnostics.push(metadata_diagnostic(
                match required {
                    "name" => "metadata-missing-name",
                    "version" => "metadata-missing-version",
                    "author" => "metadata-missing-author",
                    "license" => "metadata-missing-license",
                    _ => unreachable!(),
                },
                format!("zuzu-distribution.json must include a non-empty `{required}` string"),
                full_text_range(text),
            ));
        }
    }
    if let Some(name) = metadata.get("name").and_then(|value| value.as_str()) {
        if !dist_name_ok(name) {
            diagnostics.push(metadata_diagnostic(
                "metadata-invalid-name",
                "zuzu-distribution.json name must match the ZDF-1 distribution name pattern",
                full_text_range(text),
            ));
        }
    }
    if let Some(version) = metadata.get("version").and_then(|value| value.as_str()) {
        if !version_ok(version) {
            diagnostics.push(metadata_diagnostic(
                "metadata-invalid-version",
                "zuzu-distribution.json version must match the ZDF-1 version pattern",
                full_text_range(text),
            ));
        }
    }
    if let Some(status) = metadata.get("status") {
        if status
            .as_str()
            .is_none_or(|status| status != "stable" && status != "trial")
        {
            diagnostics.push(metadata_diagnostic(
                "metadata-invalid-status",
                "zuzu-distribution.json status must be `stable` or `trial`",
                full_text_range(text),
            ));
        }
    }
    if let Some(repo) = metadata.get("repo") {
        if repo.as_str().is_none_or(|repo| !url_ok(repo)) {
            diagnostics.push(metadata_diagnostic(
                "metadata-invalid-repo",
                "zuzu-distribution.json repo must be a valid http/https URL",
                full_text_range(text),
            ));
        }
    }
    match metadata.get("abstract") {
        Some(abstract_text)
            if abstract_text
                .as_str()
                .is_some_and(|abstract_text| abstract_text.chars().count() <= 140) => {}
        Some(_) => diagnostics.push(metadata_warning(
            "metadata-invalid-abstract",
            "zuzu-distribution.json abstract should be a string of 140 characters or fewer",
            full_text_range(text),
        )),
        None => diagnostics.push(metadata_warning(
            "metadata-missing-abstract",
            "zuzu-distribution.json should include an upload-friendly abstract",
            full_text_range(text),
        )),
    }
    let Some(dependencies) = metadata.get("dependencies") else {
        return (BTreeSet::new(), diagnostics);
    };
    let Some(dependencies) = dependencies.as_object() else {
        diagnostics.push(metadata_diagnostic(
            "metadata-invalid-dependencies",
            "zuzu-distribution.json dependencies must be an object",
            full_text_range(text),
        ));
        return (BTreeSet::new(), diagnostics);
    };
    let dependency_diagnostic_count = diagnostics.len();
    for (name, version) in dependencies {
        if !module_name_ok(name) {
            diagnostics.push(metadata_diagnostic(
                "metadata-invalid-dependency-name",
                "zuzu-distribution.json dependency names must match the ZDF-1 module name pattern",
                full_text_range(text),
            ));
        }
        if version
            .as_str()
            .is_none_or(|version| version.trim().is_empty())
        {
            diagnostics.push(metadata_diagnostic(
                "metadata-invalid-dependency-version",
                "zuzu-distribution.json dependency versions must be non-empty strings",
                full_text_range(text),
            ));
        }
    }
    if diagnostics.len() > dependency_diagnostic_count {
        return (BTreeSet::new(), diagnostics);
    }
    (dependencies.keys().cloned().collect(), diagnostics)
}

fn dist_name_ok(name: &str) -> bool {
    name.len() <= 128
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn version_ok(version: &str) -> bool {
    version.len() <= 64
        && version
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

fn module_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
}

fn url_ok(url: &str) -> bool {
    url.len() <= 1024
        && !url.chars().any(char::is_whitespace)
        && Url::parse(url)
            .ok()
            .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

fn metadata_diagnostic(code: &'static str, message: impl Into<String>, range: Range) -> Diagnostic {
    metadata_diagnostic_with_severity(code, message, range, DiagnosticSeverity::Error)
}

fn metadata_warning(code: &'static str, message: impl Into<String>, range: Range) -> Diagnostic {
    metadata_diagnostic_with_severity(code, message, range, DiagnosticSeverity::Warning)
}

fn metadata_diagnostic_with_severity(
    code: &'static str,
    message: impl Into<String>,
    range: Range,
    severity: DiagnosticSeverity,
) -> Diagnostic {
    Diagnostic {
        range,
        severity,
        source: "zuzu-package",
        code,
        message: message.into(),
    }
}

fn package_diagnostic(uri: &str, diagnostic: &Diagnostic) -> PackageDiagnostic {
    PackageDiagnostic {
        uri: uri.to_string(),
        range: diagnostic.range,
        severity: diagnostic.severity.clone(),
        source: diagnostic.source.to_string(),
        code: diagnostic.code.to_string(),
        message: diagnostic.message.clone(),
    }
}

fn dependency_covers_module(dependency: &str, module: &str) -> bool {
    dependency == module
        || module
            .strip_prefix(dependency)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_stdlib_module(module: &str) -> bool {
    module == "javascript"
        || module == "perl"
        || module.strip_prefix("std/").is_some()
        || module.strip_prefix("test/").is_some()
}

fn parse_workspace_document(workspace: &Workspace, uri: String, text: String) -> Document {
    let mut document = Document::parse(uri, text);
    document.diagnostics.extend(
        document
            .imports
            .iter()
            .filter(|import| {
                import.try_range.is_none() && !workspace.resolve_module(&import.module)
            })
            .map(|import| workspace.unresolved_import_diagnostic(import)),
    );
    document.collect_unused_import_diagnostics(workspace);
    document.collect_try_import_diagnostics(workspace);
    document.collect_missing_dependency_diagnostics(workspace);
    document.collect_portability_diagnostics();
    document.collect_undefined_local_diagnostics();
    document
}

fn is_zuzu_source_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("zzs" | "zzm")
    ) {
        return true;
    }
    if path.extension().is_some() {
        return false;
    }
    has_zuzu_shebang(path)
}

fn has_zuzu_shebang(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.lines()
        .next()
        .is_some_and(|line| line.starts_with("#!") && line.contains("zuzu"))
}

impl Document {
    pub fn parse(uri: String, text: String) -> Self {
        let line_offsets = line_offsets(&text);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_zuzu::language())
            .expect("tree-sitter-zuzu language should load");
        let tree = parser.parse(&text, None).expect("tree-sitter should parse");

        let mut document = Self {
            uri,
            text,
            line_offsets,
            tree,
            diagnostics: Vec::new(),
            symbols: Vec::new(),
            imports: Vec::new(),
            signatures: Vec::new(),
            type_relations: Vec::new(),
        };
        document.collect();
        document
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn full_range(&self) -> Range {
        let line = self.line_offsets.len().saturating_sub(1) as u32;
        let start = self.line_offsets.last().copied().unwrap_or_default();
        Range::new(
            Position::new(0, 0),
            Position::new(line, self.utf16_column(start, self.text.len())),
        )
    }

    fn collect(&mut self) {
        let tree = self.tree.clone();
        let root = tree.root_node();
        let mut cursor = root.walk();
        self.collect_node(root, &mut cursor);
        self.coalesce_syntax_diagnostics();
        self.collect_top_level_duplicate_diagnostics();
    }

    fn coalesce_syntax_diagnostics(&mut self) {
        let mut syntax = Vec::new();
        let mut other = Vec::new();
        for diagnostic in std::mem::take(&mut self.diagnostics) {
            if diagnostic.source == "zuzu-syntax" {
                syntax.push(diagnostic);
            } else {
                other.push(diagnostic);
            }
        }
        syntax.sort_by(|left, right| {
            compare_positions(left.range.start, right.range.start)
                .then(compare_positions(left.range.end, right.range.end))
        });

        let mut coalesced: Vec<Diagnostic> = Vec::new();
        for diagnostic in syntax {
            if let Some(last) = coalesced.last_mut() {
                if ranges_touch_or_overlap(last.range, diagnostic.range) {
                    last.range = merge_ranges(last.range, diagnostic.range);
                    last.code = "parse-error";
                    last.message = "Could not parse this ZuzuScript syntax".to_string();
                    continue;
                }
            }
            coalesced.push(diagnostic);
        }

        self.diagnostics = coalesced;
        self.diagnostics.extend(other);
    }

    fn collect_node(&mut self, node: Node, cursor: &mut TreeCursor) {
        if node.is_missing() {
            self.diagnostics.push(Diagnostic {
                range: self.zero_width_range(node.start_byte()),
                severity: DiagnosticSeverity::Error,
                source: "zuzu-syntax",
                code: "missing-node",
                message: format!("Missing `{}`", node.kind()),
            });
            return;
        }

        if node.kind() == "ERROR" {
            self.diagnostics.push(Diagnostic {
                range: self.node_range(node),
                severity: DiagnosticSeverity::Error,
                source: "zuzu-syntax",
                code: "parse-error",
                message: "Could not parse this ZuzuScript syntax".to_string(),
            });
        }

        match node.kind() {
            "import_statement" => self.collect_import(node),
            "function_declaration" | "function_predeclaration" => {
                self.collect_named_symbol(node, SymbolKind::Function, "function");
                self.collect_callable_signature(node);
            }
            "method_declaration" | "method_predeclaration" => {
                self.collect_named_symbol(node, SymbolKind::Method, "method");
                self.collect_callable_signature(node);
            }
            "class_declaration" => {
                self.collect_named_symbol(node, SymbolKind::Class, "class");
                self.collect_type_relations(node);
            }
            "trait_declaration" => {
                self.collect_named_symbol(node, SymbolKind::Trait, "trait");
                self.collect_type_relations(node);
            }
            "field_declaration" => {
                self.collect_named_symbol(node, SymbolKind::Field, "field");
                self.collect_field_accessor_signatures(node);
            }
            "variable_declaration" => {
                self.collect_named_symbol(node, SymbolKind::Variable, "variable")
            }
            "parameter" => self.collect_named_symbol(node, SymbolKind::Parameter, "parameter"),
            _ => {}
        }

        if cursor.goto_first_child() {
            loop {
                self.collect_node(cursor.node(), cursor);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
    }

    fn collect_import(&mut self, node: Node) {
        if let Some(module) = node.child_by_field_name("module") {
            if let Ok(name) = module.utf8_text(self.text.as_bytes()) {
                let range = self.node_range(module);
                self.imports.push(Import {
                    module: name.to_string(),
                    module_range: range,
                    try_range: self.import_try_range(node),
                    statement_range: self.node_range(node),
                    delete_range: self.line_delete_range(self.node_range(node)),
                    imported_names: self.imported_names(node),
                });
                self.symbols.push(Symbol {
                    name: name.to_string(),
                    kind: SymbolKind::Import,
                    range: self.node_range(node),
                    selection_range: range,
                    detail: Some("import".to_string()),
                    uri: self.uri.clone(),
                });
            }
        }
    }

    fn import_try_range(&self, import_statement: Node) -> Option<Range> {
        let module = import_statement.child_by_field_name("module")?;
        let imports = import_statement.child_by_field_name("imports")?;
        let between = self.text.get(module.end_byte()..imports.start_byte())?;
        let offset = between.find("try")?;
        let start = module.end_byte() + offset;
        let end = start + "try".len();
        Some(Range::new(
            self.position_for_byte(start),
            self.position_for_byte(end),
        ))
    }

    fn imported_names(&self, import_statement: Node) -> Vec<ImportedName> {
        let Some(imports) = import_statement.child_by_field_name("imports") else {
            return Vec::new();
        };
        if imports.kind() != "import_list" {
            return Vec::new();
        }

        let mut names = Vec::new();
        let mut cursor = imports.walk();
        if cursor.goto_first_child() {
            loop {
                let specifier = cursor.node();
                if specifier.kind() == "import_specifier" {
                    if let Some(local) = specifier
                        .child_by_field_name("alias")
                        .or_else(|| specifier.child_by_field_name("name"))
                    {
                        if let Ok(name) = local.utf8_text(self.text.as_bytes()) {
                            names.push(ImportedName {
                                name: name.to_string(),
                            });
                        }
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        names
    }

    fn collect_named_symbol(&mut self, node: Node, kind: SymbolKind, detail: &str) {
        if let Some(name) = node.child_by_field_name("name") {
            if let Ok(text) = name.utf8_text(self.text.as_bytes()) {
                self.symbols.push(Symbol {
                    name: text.to_string(),
                    kind,
                    range: self.node_range(node),
                    selection_range: self.node_range(name),
                    detail: Some(detail.to_string()),
                    uri: self.uri.clone(),
                });
            }
        }
    }

    fn collect_type_relations(&mut self, node: Node) {
        let Some(subtype) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(subtype) = subtype.utf8_text(self.text.as_bytes()) else {
            return;
        };
        let subtype = subtype.to_string();

        if let Some(superclass) = node.child_by_field_name("superclass") {
            if let Some(supertype) = self.type_expression_text(superclass) {
                self.type_relations.push(TypeRelation {
                    subtype: subtype.clone(),
                    supertype,
                });
            }
        }

        let mut cursor = node.walk();
        if !cursor.goto_first_child() {
            return;
        }
        loop {
            let child = cursor.node();
            if child.kind() == "trait_composition" {
                self.collect_trait_composition_relations(subtype.as_str(), child);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    fn collect_trait_composition_relations(&mut self, subtype: &str, node: Node) {
        let mut cursor = node.walk();
        if !cursor.goto_first_child() {
            return;
        }
        loop {
            let child = cursor.node();
            if child.kind() == "type_expression" {
                if let Some(supertype) = self.type_expression_text(child) {
                    self.type_relations.push(TypeRelation {
                        subtype: subtype.to_string(),
                        supertype,
                    });
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    fn type_expression_text(&self, node: Node) -> Option<String> {
        let text = node.utf8_text(self.text.as_bytes()).ok()?.trim();
        if text.is_empty() {
            None
        } else {
            Some(text.to_string())
        }
    }

    fn collect_callable_signature(&mut self, node: Node) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let Some(parameters) = node.child_by_field_name("parameters") else {
            return;
        };
        let Ok(name) = name.utf8_text(self.text.as_bytes()) else {
            return;
        };

        let mut parameter_names = Vec::new();
        let mut cursor = parameters.walk();
        if cursor.goto_first_child() {
            loop {
                let parameter = cursor.node();
                if parameter.kind() == "parameter" {
                    if let Some(name) = parameter.child_by_field_name("name") {
                        if let Ok(name) = name.utf8_text(self.text.as_bytes()) {
                            parameter_names.push(name.to_string());
                        }
                    }
                    if let Some(name) = parameter.child_by_field_name("variadic_name") {
                        if let Ok(name) = name.utf8_text(self.text.as_bytes()) {
                            parameter_names.push(name.to_string());
                        }
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }

        self.signatures.push(CallableSignature {
            name: name.to_string(),
            label: format!("{}({})", name, parameter_names.join(", ")),
            parameters: parameter_names,
        });
    }

    fn collect_field_accessor_signatures(&mut self, node: Node) {
        let Some(field) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(field) = field.utf8_text(self.text.as_bytes()) else {
            return;
        };
        let field = field.to_string();

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind() == "field_accessor_list" {
                    self.collect_field_accessor_signatures_from_list(&field, child);
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn collect_field_accessor_signatures_from_list(&mut self, field: &str, list: Node) {
        let Ok(text) = list.utf8_text(self.text.as_bytes()) else {
            return;
        };
        for accessor in text.split(|character: char| !character.is_ascii_alphanumeric()) {
            let parameters = match accessor {
                "get" | "clear" | "has" => Vec::new(),
                "set" => vec!["value".to_string()],
                _ => continue,
            };
            let name = format!("{accessor}_{field}");
            self.signatures.push(CallableSignature {
                label: format!("{}({})", name, parameters.join(", ")),
                name,
                parameters,
            });
        }
    }

    fn collect_top_level_duplicate_diagnostics(&mut self) {
        let tree = self.tree.clone();
        let root = tree.root_node();
        let mut seen: BTreeMap<(String, String), Range> = BTreeMap::new();
        let mut cursor = root.walk();

        if cursor.goto_first_child() {
            loop {
                let node = cursor.node();
                if let Some((detail, name, range)) = self.top_level_declaration(node) {
                    let key = (detail.to_string(), name.to_string());
                    if seen.insert(key, range).is_some() {
                        self.diagnostics.push(Diagnostic {
                            range,
                            severity: DiagnosticSeverity::Error,
                            source: "zuzu-semantic",
                            code: "duplicate-declaration",
                            message: format!("Duplicate top-level {detail} `{name}`"),
                        });
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn top_level_declaration(&self, node: Node) -> Option<(&'static str, &str, Range)> {
        let detail = match node.kind() {
            "function_declaration" | "function_predeclaration" => "function",
            "class_declaration" => "class",
            "trait_declaration" => "trait",
            "variable_declaration" => "variable",
            _ => return None,
        };
        let name = node.child_by_field_name("name")?;
        let text = name.utf8_text(self.text.as_bytes()).ok()?;
        Some((detail, text, self.node_range(name)))
    }

    pub fn folding_ranges(&self) -> Vec<FoldingRange> {
        let mut ranges = Vec::new();
        self.collect_folding(self.tree.root_node(), &mut ranges);
        ranges
    }

    fn selection_range(&self, position: Position) -> Option<SelectionRange> {
        let byte = self.byte_for_position(position)?;
        let mut ranges = Vec::new();
        self.collect_selection_ranges(self.tree.root_node(), byte, &mut ranges);
        ranges.dedup();

        let mut current = None;
        for range in ranges {
            current = Some(Box::new(SelectionRange {
                range,
                parent: current,
            }));
        }
        current.map(|range| *range)
    }

    fn collect_unused_import_diagnostics(&mut self, workspace: &Workspace) {
        for import in &self.imports {
            if import.imported_names.is_empty() || !workspace.resolve_module(&import.module) {
                continue;
            }
            if !self.import_is_unused(import) {
                continue;
            }
            self.diagnostics.push(Diagnostic {
                range: import.statement_range,
                severity: DiagnosticSeverity::Warning,
                source: "zuzu-semantic",
                code: "unused-import",
                message: format!("Imported symbols from `{}` are unused", import.module),
            });
        }
    }

    fn collect_try_import_diagnostics(&mut self, workspace: &Workspace) {
        for import in &self.imports {
            let Some(range) = import.try_range else {
                continue;
            };
            if !workspace.resolve_module(&import.module) {
                continue;
            }
            self.diagnostics.push(Diagnostic {
                range,
                severity: DiagnosticSeverity::Warning,
                source: "zuzu-semantic",
                code: "suspicious-try-import",
                message: format!(
                    "`try import` is unnecessary for resolvable module `{}`",
                    import.module
                ),
            });
        }
    }

    fn collect_missing_dependency_diagnostics(&mut self, workspace: &Workspace) {
        for import in &self.imports {
            if let Some(diagnostic) = workspace.missing_dependency_diagnostic(&self.uri, import) {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    fn collect_portability_diagnostics(&mut self) {
        for import in &self.imports {
            let Some(runtime) = runtime_specific_module(&import.module) else {
                continue;
            };
            self.diagnostics.push(Diagnostic {
                range: import.module_range,
                severity: DiagnosticSeverity::Warning,
                source: "zuzu-semantic",
                code: "runtime-specific-module",
                message: format!(
                    "Module `{}` is specific to the {runtime} implementation",
                    import.module
                ),
            });
        }
    }

    fn collect_undefined_local_diagnostics(&mut self) {
        if self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source == "zuzu-syntax")
            || self
                .imports
                .iter()
                .any(|import| import.imported_names.is_empty())
        {
            return;
        }

        let tree = self.tree.clone();
        let root = tree.root_node();
        let globals = self.global_names();
        let mut diagnostics = Vec::new();
        self.collect_undefined_local_diagnostics_in_node(root, &globals, &mut diagnostics);
        self.diagnostics.extend(diagnostics);
    }

    fn collect_undefined_local_diagnostics_in_node(
        &self,
        node: Node,
        globals: &BTreeSet<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if matches!(node.kind(), "function_declaration" | "method_declaration") {
            self.collect_callable_undefined_local_diagnostics(node, globals, diagnostics);
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                self.collect_undefined_local_diagnostics_in_node(
                    cursor.node(),
                    globals,
                    diagnostics,
                );
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn collect_callable_undefined_local_diagnostics(
        &self,
        callable: Node,
        globals: &BTreeSet<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut names = globals.clone();
        self.collect_declared_names(callable, &mut names);
        let Some(body) = callable.child_by_field_name("body") else {
            return;
        };
        let mut reported = BTreeSet::new();
        self.collect_unknown_identifiers(body, &names, &mut reported, diagnostics);
    }

    fn collect_declared_names(&self, node: Node, names: &mut BTreeSet<String>) {
        match node.kind() {
            "parameter"
            | "lambda_parameter"
            | "variable_declaration"
            | "let_expression"
            | "catch_clause"
            | "for_statement" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.insert_identifier_name(name, names);
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                self.collect_declared_names(cursor.node(), names);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn collect_unknown_identifiers(
        &self,
        node: Node,
        names: &BTreeSet<String>,
        reported: &mut BTreeSet<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if matches!(
            node.kind(),
            "identifier" | "type_identifier" | "private_type_identifier"
        ) && self.identifier_is_value_usage(node)
        {
            if let Ok(name) = node.utf8_text(self.text.as_bytes()) {
                if !names.contains(name) && reported.insert(name.to_string()) {
                    diagnostics.push(Diagnostic {
                        range: self.node_range(node),
                        severity: DiagnosticSeverity::Error,
                        source: "zuzu-semantic",
                        code: "undefined-local",
                        message: format!("Undefined local `{name}`"),
                    });
                }
            }
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                self.collect_unknown_identifiers(cursor.node(), names, reported, diagnostics);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn global_names(&self) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = KEYWORDS.iter().map(|word| (*word).to_string()).collect();
        names.extend(BUILTIN_STATEMENTS.iter().map(|word| (*word).to_string()));
        names.extend(["self", "super"].into_iter().map(String::from));
        names.extend(self.imports.iter().flat_map(|import| {
            import
                .imported_names
                .iter()
                .map(|imported| imported.name.clone())
        }));

        let tree = self.tree.clone();
        let root = tree.root_node();
        let mut cursor = root.walk();
        if cursor.goto_first_child() {
            loop {
                let node = cursor.node();
                if matches!(
                    node.kind(),
                    "function_declaration"
                        | "function_predeclaration"
                        | "class_declaration"
                        | "trait_declaration"
                        | "variable_declaration"
                ) {
                    if let Some(name) = node.child_by_field_name("name") {
                        self.insert_identifier_name(name, &mut names);
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }

        names
    }

    fn insert_identifier_name(&self, node: Node, names: &mut BTreeSet<String>) {
        if let Ok(name) = node.utf8_text(self.text.as_bytes()) {
            names.insert(name.to_string());
        }
    }

    fn identifier_is_value_usage(&self, node: Node) -> bool {
        let Ok(name) = node.utf8_text(self.text.as_bytes()) else {
            return false;
        };
        if KEYWORDS.contains(&name) || BUILTIN_STATEMENTS.contains(&name) {
            return false;
        }

        let Some(parent) = node.parent() else {
            return true;
        };
        match parent.kind() {
            "bare_key" | "binding_identifier" | "type_expression" | "return_type"
            | "constructor_target" | "import_statement" | "import_specifier" => return false,
            "function_declaration"
            | "function_predeclaration"
            | "method_declaration"
            | "method_predeclaration"
            | "variable_declaration"
            | "let_expression"
            | "field_declaration"
            | "parameter"
            | "lambda_parameter"
            | "catch_clause"
            | "for_statement" => {
                if node_is_field(parent, node, "name") {
                    return false;
                }
            }
            "member_expression" => {
                if node_is_field(parent, node, "property") {
                    return false;
                }
            }
            "builtin_statement" => {
                if node_is_field(parent, node, "name") {
                    return false;
                }
            }
            "switch_header" => {
                if node_is_field(parent, node, "mode") {
                    return false;
                }
            }
            _ => {}
        }

        true
    }

    fn import_is_unused(&self, import: &Import) -> bool {
        import.imported_names.iter().all(|name| {
            self.word_ranges(&name.name)
                .into_iter()
                .all(|range| ranges_overlap(range, import.statement_range))
        })
    }

    fn exports_name(&self, name: &str) -> bool {
        let tree = self.tree.clone();
        let root = tree.root_node();
        let mut cursor = root.walk();
        if cursor.goto_first_child() {
            loop {
                let node = cursor.node();
                if self
                    .top_level_declaration(node)
                    .is_some_and(|(_, declared, _)| declared == name)
                {
                    return true;
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        false
    }

    fn add_import_edit(&self, module: &str, name: &str) -> Option<TextEdit> {
        if !is_identifier(name) {
            return None;
        }
        let position = self.import_insert_position();
        Some(TextEdit {
            range: Range::new(position, position),
            new_text: format!("from {module} import {name};\n"),
        })
    }

    fn import_insert_position(&self) -> Position {
        self.imports
            .iter()
            .map(|import| import.delete_range.end)
            .max_by(|left, right| compare_positions(*left, *right))
            .unwrap_or(Position::new(0, 0))
    }

    fn collect_folding(&self, node: Node, ranges: &mut Vec<FoldingRange>) {
        let kind = match node.kind() {
            "block" | "class_body" | "trait_body" | "switch_statement" => Some("region"),
            "array_literal" | "dict_literal" | "pairlist_literal" | "parameter_list" => {
                Some("region")
            }
            "comment" if node.start_position().row < node.end_position().row => Some("comment"),
            _ => None,
        };

        if let Some(kind) = kind {
            let start = node.start_position();
            let end = node.end_position();
            if end.row > start.row {
                ranges.push(FoldingRange {
                    start_line: start.row as u32,
                    start_character: start.column as u32,
                    end_line: end.row as u32,
                    end_character: end.column as u32,
                    kind: Some(kind),
                });
            }
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                self.collect_folding(cursor.node(), ranges);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn collect_selection_ranges(&self, node: Node, byte: usize, ranges: &mut Vec<Range>) {
        if !node_contains_byte(node, byte) {
            return;
        }
        if node.is_named() && node.start_byte() < node.end_byte() {
            let range = self.node_range(node);
            if ranges.last().copied() != Some(range) {
                ranges.push(range);
            }
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                self.collect_selection_ranges(cursor.node(), byte, ranges);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn visible_symbols(&self, _position: Position) -> impl Iterator<Item = &Symbol> {
        self.symbols.iter()
    }

    fn symbol_at(&self, position: Position) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| {
            position_ge(position, symbol.selection_range.start)
                && position_le(position, symbol.selection_range.end)
        })
    }

    fn word_at(&self, position: Position) -> Option<String> {
        let byte = self.byte_for_position(position)?;
        let bytes = self.text.as_bytes();
        let mut start = byte.min(bytes.len());
        let mut end = start;
        while start > 0 && is_word_byte(bytes[start - 1]) {
            start -= 1;
        }
        while end < bytes.len() && is_word_byte(bytes[end]) {
            end += 1;
        }
        (start < end).then(|| self.text[start..end].to_string())
    }

    fn word_range(&self, position: Position) -> Option<Range> {
        let byte = self.byte_for_position(position)?;
        let bytes = self.text.as_bytes();
        let mut start = byte.min(bytes.len());
        let mut end = start;
        while start > 0 && is_word_byte(bytes[start - 1]) {
            start -= 1;
        }
        while end < bytes.len() && is_word_byte(bytes[end]) {
            end += 1;
        }
        (start < end)
            .then(|| Range::new(self.position_for_byte(start), self.position_for_byte(end)))
    }

    fn word_ranges(&self, word: &str) -> Vec<Range> {
        if word.is_empty() {
            return Vec::new();
        }

        let bytes = self.text.as_bytes();
        let word_bytes = word.as_bytes();
        let mut ranges = Vec::new();
        let mut index = 0;
        while index + word_bytes.len() <= bytes.len() {
            if &bytes[index..index + word_bytes.len()] == word_bytes
                && (index == 0 || !is_word_byte(bytes[index - 1]))
                && (index + word_bytes.len() == bytes.len()
                    || !is_word_byte(bytes[index + word_bytes.len()]))
            {
                ranges.push(Range::new(
                    self.position_for_byte(index),
                    self.position_for_byte(index + word_bytes.len()),
                ));
                index += word_bytes.len();
            } else {
                index += 1;
            }
        }
        ranges
    }

    fn operator_at(&self, position: Position) -> Option<OperatorHover> {
        let byte = self.byte_for_position(position)?;
        for operator in SYMBOL_OPERATORS {
            for (start, _) in self.text.match_indices(operator.symbol) {
                let end = start + operator.symbol.len();
                if start <= byte && byte < end {
                    return Some(OperatorHover {
                        range: Range::new(
                            self.position_for_byte(start),
                            self.position_for_byte(end),
                        ),
                        description: operator.description,
                    });
                }
            }
        }
        None
    }

    fn text_for_range(&self, range: Range) -> Option<&str> {
        let start = self.byte_for_position(range.start)?;
        let end = self.byte_for_position(range.end)?;
        self.text.get(start..end)
    }

    fn line_delete_range(&self, range: Range) -> Range {
        let start_line = range.start.line as usize;
        let next_line = start_line + 1;
        let Some(&line_start) = self.line_offsets.get(start_line) else {
            return range;
        };
        let end = self
            .line_offsets
            .get(next_line)
            .copied()
            .unwrap_or(self.text.len());
        Range::new(
            self.position_for_byte(line_start),
            self.position_for_byte(end),
        )
    }

    fn call_context(&self, position: Position) -> Option<CallContext> {
        let mut best = None;
        self.collect_call_context(self.tree.root_node(), position, &mut best);
        best.map(|(_, context)| context)
    }

    fn call_argument_positions(&self) -> Vec<CallArguments> {
        let mut calls = Vec::new();
        self.collect_call_argument_positions(self.tree.root_node(), &mut calls);
        calls
    }

    fn call_sites_to(&self, name: &str) -> Vec<CallSite> {
        self.call_sites()
            .into_iter()
            .filter(|call| call.name == name)
            .collect()
    }

    fn call_sites_in_range(&self, range: Range) -> Vec<CallSite> {
        self.call_sites()
            .into_iter()
            .filter(|call| ranges_overlap(call.range, range))
            .collect()
    }

    fn call_sites(&self) -> Vec<CallSite> {
        let mut calls = Vec::new();
        self.collect_call_sites(self.tree.root_node(), &mut calls);
        calls
    }

    fn callable_by_selection_range(&self, range: Range) -> Option<&Symbol> {
        self.symbols
            .iter()
            .find(|symbol| is_callable_kind(&symbol.kind) && symbol.selection_range == range)
    }

    fn callable_containing(&self, position: Position) -> Option<&Symbol> {
        self.symbols
            .iter()
            .filter(|symbol| is_callable_kind(&symbol.kind))
            .filter(|symbol| position_in_range(position, symbol.range))
            .min_by_key(|symbol| range_size(symbol.range))
    }

    fn collect_call_argument_positions(&self, node: Node, calls: &mut Vec<CallArguments>) {
        if node.kind() == "call_expression" {
            if let Some(call) = self.call_arguments_for_node(node) {
                calls.push(call);
            }
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                self.collect_call_argument_positions(cursor.node(), calls);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn collect_call_sites(&self, node: Node, calls: &mut Vec<CallSite>) {
        if node.kind() == "call_expression" {
            if let Some(call) = self.call_site_for_node(node) {
                calls.push(call);
            }
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                self.collect_call_sites(cursor.node(), calls);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn call_site_for_node(&self, node: Node) -> Option<CallSite> {
        let function = node.child_by_field_name("function")?;
        let (name, range) = self.callable_name_for_function(function)?;
        Some(CallSite { name, range })
    }

    fn call_arguments_for_node(&self, node: Node) -> Option<CallArguments> {
        let function = node.child_by_field_name("function")?;
        let arguments = node.child_by_field_name("arguments")?;
        let (name, _) = self.callable_name_for_function(function)?;

        let mut positions = Vec::new();
        let mut cursor = arguments.walk();
        if cursor.goto_first_child() {
            loop {
                let argument = cursor.node();
                if argument.is_named()
                    && argument.kind() != "pair_entry"
                    && argument.kind() != "spread_argument"
                {
                    positions.push(self.node_range(argument).start);
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }

        Some(CallArguments {
            name,
            range: self.node_range(node),
            arguments: positions,
        })
    }

    fn collect_call_context(
        &self,
        node: Node,
        position: Position,
        best: &mut Option<(usize, CallContext)>,
    ) {
        if node.kind() == "call_expression" {
            if let Some(context) = self.call_context_for_node(node, position) {
                let width = node.end_byte().saturating_sub(node.start_byte());
                if best
                    .as_ref()
                    .is_none_or(|(best_width, _)| width < *best_width)
                {
                    *best = Some((width, context));
                }
            }
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                self.collect_call_context(cursor.node(), position, best);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn call_context_for_node(&self, node: Node, position: Position) -> Option<CallContext> {
        let function = node.child_by_field_name("function")?;
        let arguments = node.child_by_field_name("arguments")?;
        if !position_in_range(position, self.node_range(arguments)) {
            return None;
        }
        let (name, _) = self.callable_name_for_function(function)?;
        Some(CallContext {
            name,
            active_parameter: self.active_argument(arguments, position)?,
        })
    }

    fn callable_name_for_function(&self, function: Node) -> Option<(String, Range)> {
        match function.kind() {
            "identifier" => {
                let name = function.utf8_text(self.text.as_bytes()).ok()?;
                is_identifier(name).then(|| (name.to_string(), self.node_range(function)))
            }
            "member_expression" => {
                let property = function.child_by_field_name("property")?;
                let name = property.utf8_text(self.text.as_bytes()).ok()?;
                is_identifier(name).then(|| (name.to_string(), self.node_range(property)))
            }
            _ => None,
        }
    }

    fn active_argument(&self, arguments: Node, position: Position) -> Option<u32> {
        let cursor = self.byte_for_position(position)?;
        let end = cursor.min(arguments.end_byte());
        let text = self.text.get(arguments.start_byte()..end)?;
        Some(text.bytes().filter(|byte| *byte == b',').count() as u32)
    }

    fn node_range(&self, node: Node) -> Range {
        Range::new(
            self.position_for_point(node.start_byte(), node.start_position()),
            self.position_for_point(node.end_byte(), node.end_position()),
        )
    }

    fn zero_width_range(&self, byte: usize) -> Range {
        let position = self.position_for_byte(byte);
        Range::new(position, position)
    }

    fn position_for_point(&self, byte: usize, point: Point) -> Position {
        let line_start = self
            .line_offsets
            .get(point.row)
            .copied()
            .unwrap_or(byte.saturating_sub(point.column));
        Position::new(point.row as u32, self.utf16_column(line_start, byte))
    }

    fn position_for_byte(&self, byte: usize) -> Position {
        let byte = byte.min(self.text.len());
        let line = match self.line_offsets.binary_search(&byte) {
            Ok(line) => line,
            Err(line) => line.saturating_sub(1),
        };
        let line_start = self.line_offsets.get(line).copied().unwrap_or_default();
        Position::new(line as u32, self.utf16_column(line_start, byte))
    }

    fn byte_for_position(&self, position: Position) -> Option<usize> {
        let line_start = *self.line_offsets.get(position.line as usize)?;
        let line_end = self
            .line_offsets
            .get(position.line as usize + 1)
            .copied()
            .unwrap_or(self.text.len());
        let mut utf16 = 0;
        for (offset, ch) in self.text[line_start..line_end].char_indices() {
            if utf16 >= position.character {
                return Some(line_start + offset);
            }
            if ch == '\n' || ch == '\r' {
                return Some(line_start + offset);
            }
            utf16 += ch.len_utf16() as u32;
        }
        Some(line_end)
    }

    fn utf16_column(&self, line_start: usize, byte: usize) -> u32 {
        self.text[line_start..byte.min(self.text.len())]
            .chars()
            .map(|ch| ch.len_utf16() as u32)
            .sum()
    }
}

#[derive(Debug, Clone)]
struct CallContext {
    name: String,
    active_parameter: u32,
}

#[derive(Debug, Clone)]
struct CallArguments {
    name: String,
    range: Range,
    arguments: Vec<Position>,
}

#[derive(Debug, Clone)]
struct CallSite {
    name: String,
    range: Range,
}

#[derive(Debug, Clone, Copy)]
struct SymbolOperator {
    symbol: &'static str,
    description: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct OperatorHover {
    range: Range,
    description: &'static str,
}

type CallKey = (String, String, u32, u32, u32, u32);
type TypeKey = (String, String, u32, u32, u32, u32);

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "but", "case", "catch", "class", "const", "default", "do", "else",
    "extends", "fn", "for", "from", "function", "if", "import", "in", "let", "method", "new",
    "return", "self", "spawn", "static", "super", "switch", "throw", "trait", "try", "unless",
    "while", "with",
];

const BUILTIN_STATEMENTS: &[&str] = &["assert", "debug", "die", "print", "say", "warn"];

const SYMBOL_OPERATORS: &[SymbolOperator] = &[
    SymbolOperator {
        symbol: ">>>",
        description: "Bag literal delimiter.",
    },
    SymbolOperator {
        symbol: "<<<",
        description: "Bag literal delimiter.",
    },
    SymbolOperator {
        symbol: "**",
        description: "Exponentiation operator.",
    },
    SymbolOperator {
        symbol: "==",
        description: "Type-aware equality comparison.",
    },
    SymbolOperator {
        symbol: "!=",
        description: "Type-aware inequality comparison.",
    },
    SymbolOperator {
        symbol: "<=",
        description: "Numeric less-than-or-equal comparison.",
    },
    SymbolOperator {
        symbol: ">=",
        description: "Numeric greater-than-or-equal comparison.",
    },
    SymbolOperator {
        symbol: "<<",
        description: "Set literal delimiter.",
    },
    SymbolOperator {
        symbol: ">>",
        description: "Set literal delimiter or right-shift operator, depending on context.",
    },
    SymbolOperator {
        symbol: "∈",
        description: "Collection membership operator.",
    },
    SymbolOperator {
        symbol: "∉",
        description: "Collection non-membership operator.",
    },
    SymbolOperator {
        symbol: "⋃",
        description: "Collection union operator.",
    },
    SymbolOperator {
        symbol: "⋂",
        description: "Collection intersection operator.",
    },
    SymbolOperator {
        symbol: "≡",
        description: "Type-aware equality comparison.",
    },
    SymbolOperator {
        symbol: "≢",
        description: "Type-aware inequality comparison.",
    },
    SymbolOperator {
        symbol: "≤",
        description: "Numeric less-than-or-equal comparison.",
    },
    SymbolOperator {
        symbol: "≥",
        description: "Numeric greater-than-or-equal comparison.",
    },
    SymbolOperator {
        symbol: "+",
        description: "Numeric addition operator.",
    },
    SymbolOperator {
        symbol: "-",
        description: "Numeric subtraction or negation operator.",
    },
    SymbolOperator {
        symbol: "*",
        description: "Numeric multiplication operator.",
    },
    SymbolOperator {
        symbol: "×",
        description: "Numeric multiplication operator.",
    },
    SymbolOperator {
        symbol: "/",
        description: "Numeric division operator.",
    },
    SymbolOperator {
        symbol: "÷",
        description: "Numeric division operator.",
    },
    SymbolOperator {
        symbol: "<",
        description: "Numeric less-than comparison or set literal delimiter.",
    },
    SymbolOperator {
        symbol: ">",
        description: "Numeric greater-than comparison or set literal delimiter.",
    },
];

fn runtime_specific_module(module: &str) -> Option<&'static str> {
    match module {
        "perl" => Some("Perl"),
        _ => None,
    }
}

fn describe_keyword_or_builtin(word: &str) -> Option<&'static str> {
    match word {
        "as" => Some("Introduces an import alias."),
        "async" => Some("Marks work that may run asynchronously."),
        "await" => Some("Waits for an asynchronous result."),
        "but" => Some("Introduces traits composed into a class or trait declaration."),
        "case" => Some("Introduces a switch case branch."),
        "catch" => Some("Handles exceptions thrown by a try block."),
        "class" => Some("Declares a ZuzuScript class."),
        "const" => Some("Declares an immutable binding."),
        "default" => Some("Introduces the fallback switch branch."),
        "do" => Some("Introduces a do block or loop body."),
        "else" => Some("Introduces the alternative branch for if or for."),
        "extends" => Some("Introduces the superclass in a class declaration."),
        "fn" => Some("Introduces a short anonymous function literal."),
        "for" => Some("Iterates over values and may have an else branch."),
        "from" => Some("Starts a module import statement."),
        "function" => Some("Declares a named function or function predeclaration."),
        "if" => Some("Introduces a conditional branch."),
        "import" => Some("Imports symbols from a ZuzuScript module."),
        "in" => Some("Separates a loop variable from the iterated expression."),
        "let" => Some("Declares a mutable local binding."),
        "method" => Some("Declares a method inside a class or trait."),
        "new" => Some("Constructs a new object."),
        "return" => Some("Returns a value from the current function or method."),
        "self" => Some("Refers to the current object inside a method."),
        "spawn" => Some("Starts work in a separate asynchronous task."),
        "static" => Some("Marks a method as belonging to the class or trait itself."),
        "super" => Some("Refers to superclass behaviour from a method."),
        "switch" => Some("Selects one branch from a group of cases."),
        "throw" => Some("Throws an exception value."),
        "trait" => Some("Declares a ZuzuScript trait."),
        "try" => Some("Starts exception handling or an optional import."),
        "unless" => Some("Introduces a negated conditional branch."),
        "while" => Some("Repeats a block while a condition remains true."),
        "with" => Some("Introduces traits in compact class or trait declarations."),
        "assert" => Some("Checks a condition and fails when it is false."),
        "debug" => Some("Emits debugging output."),
        "die" => Some("Terminates by throwing an error."),
        "print" => Some("Writes output without automatically adding a newline."),
        "say" => Some("Writes output and adds a newline."),
        "warn" => Some("Emits a warning message."),
        word if KEYWORDS.contains(&word) => Some("ZuzuScript keyword."),
        _ => None,
    }
}

fn describe_word_operator(word: &str) -> Option<&'static str> {
    match word {
        "mod" => Some("Numeric modulo operator."),
        "and" => Some("Logical and operator."),
        "or" => Some("Logical or operator."),
        "not" => Some("Logical negation operator."),
        "typeof" => Some("Returns the type associated with a value."),
        "floor" => Some("Rounds a numeric value towards zero."),
        "ceil" => Some("Rounds a numeric value upwards."),
        "int" => Some("Converts a numeric value to an integer."),
        "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "cmp" => Some("String comparison operator."),
        "in" => Some("Collection membership operator."),
        _ => None,
    }
}

fn format_symbol(symbol: &Symbol) -> String {
    let detail = symbol.detail.as_deref().unwrap_or("symbol");
    format!("`{}`\n\nZuzuScript {}.", symbol.name, detail)
}

fn is_callable_kind(kind: &SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Method)
}

fn is_type_kind(kind: &SymbolKind) -> bool {
    matches!(kind, SymbolKind::Class | SymbolKind::Trait)
}

fn call_hierarchy_item(symbol: &Symbol) -> CallHierarchyItem {
    CallHierarchyItem {
        name: symbol.name.clone(),
        kind: symbol.kind.clone(),
        uri: symbol.uri.clone(),
        range: symbol.range,
        selection_range: symbol.selection_range,
        detail: symbol.detail.clone(),
    }
}

fn call_key(item: &CallHierarchyItem) -> CallKey {
    (
        item.uri.clone(),
        item.name.clone(),
        item.selection_range.start.line,
        item.selection_range.start.character,
        item.selection_range.end.line,
        item.selection_range.end.character,
    )
}

fn type_hierarchy_item(symbol: &Symbol) -> TypeHierarchyItem {
    TypeHierarchyItem {
        name: symbol.name.clone(),
        kind: symbol.kind.clone(),
        uri: symbol.uri.clone(),
        range: symbol.range,
        selection_range: symbol.selection_range,
        detail: symbol.detail.clone(),
    }
}

fn type_key(item: &TypeHierarchyItem) -> TypeKey {
    (
        item.uri.clone(),
        item.name.clone(),
        item.selection_range.start.line,
        item.selection_range.start.character,
        item.selection_range.end.line,
        item.selection_range.end.character,
    )
}

fn range_size(range: Range) -> (u32, u32) {
    (
        range.end.line.saturating_sub(range.start.line),
        range.end.character.saturating_sub(range.start.character),
    )
}

fn line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(index + 1);
        }
    }
    offsets
}

fn full_text_range(text: &str) -> Range {
    let offsets = line_offsets(text);
    let line = offsets.len().saturating_sub(1);
    let line_start = offsets.get(line).copied().unwrap_or_default();
    Range::new(
        Position::new(0, 0),
        Position::new(line as u32, utf16_width(&text[line_start..])),
    )
}

fn utf16_width(text: &str) -> u32 {
    text.chars().map(|ch| ch.len_utf16() as u32).sum()
}

fn is_word_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn is_identifier(word: &str) -> bool {
    let mut bytes = word.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic()) && bytes.all(is_word_byte)
}

fn position_ge(left: Position, right: Position) -> bool {
    (left.line, left.character) >= (right.line, right.character)
}

fn position_le(left: Position, right: Position) -> bool {
    (left.line, left.character) <= (right.line, right.character)
}

fn position_lt(left: Position, right: Position) -> bool {
    (left.line, left.character) < (right.line, right.character)
}

fn compare_positions(left: Position, right: Position) -> std::cmp::Ordering {
    (left.line, left.character).cmp(&(right.line, right.character))
}

fn position_in_range(position: Position, range: Range) -> bool {
    position_ge(position, range.start) && position_le(position, range.end)
}

fn ranges_overlap(left: Range, right: Range) -> bool {
    if left.start == left.end {
        return position_ge(left.start, right.start) && position_le(left.start, right.end);
    }
    if right.start == right.end {
        return position_ge(right.start, left.start) && position_le(right.start, left.end);
    }
    position_lt(left.start, right.end) && position_lt(right.start, left.end)
}

fn ranges_touch_or_overlap(left: Range, right: Range) -> bool {
    ranges_overlap(left, right)
        || position_le(left.end, right.end) && position_ge(left.end, right.start)
        || position_le(right.end, left.end) && position_ge(right.end, left.start)
}

fn merge_ranges(left: Range, right: Range) -> Range {
    let start = if position_le(left.start, right.start) {
        left.start
    } else {
        right.start
    };
    let end = if position_ge(left.end, right.end) {
        left.end
    } else {
        right.end
    };
    Range::new(start, end)
}

fn node_contains_byte(node: Node, byte: usize) -> bool {
    node.start_byte() <= byte && byte <= node.end_byte()
}

fn node_is_field(parent: Node, node: Node, field: &str) -> bool {
    parent.child_by_field_name(field).is_some_and(|child| {
        child.kind() == node.kind()
            && child.start_byte() == node.start_byte()
            && child.end_byte() == node.end_byte()
    })
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn find_distribution_root(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.join("zuzu-distribution.json").is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn fallback_runtime_module_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        if let Some(userprofile) = env::var_os("USERPROFILE") {
            roots.push(PathBuf::from(userprofile).join(".zuzu").join("modules"));
        }
    } else {
        if let Some(home) = env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".zuzu").join("modules"));
        }
        roots.push(PathBuf::from("/var/lib/zuzu/modules"));
    }
    if let Some(stdlib) = env::var_os("ZUZU_STDLIB") {
        roots.push(PathBuf::from(stdlib));
    }
    roots
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
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

fn scan_modules(module_roots: &[PathBuf]) -> BTreeSet<String> {
    let mut modules = BTreeSet::new();
    for root in module_roots {
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("zzm") {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(root) {
                let module = relative.with_extension("");
                let name = module
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                modules.insert(name);
            }
        }
    }
    modules
}

fn module_name_under_root(path: &Path, root: &Path) -> Option<String> {
    if !matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("zzm" | "zzs")
    ) {
        return None;
    }
    let relative = path.strip_prefix(root).ok()?;
    let module = relative.with_extension("");
    Some(
        module
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    Url::parse(uri).ok()?.to_file_path().ok()
}

fn canonical_uri_path(uri: &str) -> Option<PathBuf> {
    fs::canonicalize(uri_to_path(uri)?).ok()
}

pub fn path_to_uri(path: &Path) -> Option<String> {
    Url::from_file_path(fs::canonicalize(path).ok()?)
        .ok()
        .map(|url| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_syntax_errors() {
        let document = Document::parse("file:///bad.zzs".to_string(), "let x := ;\n".to_string());
        assert!(document
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source == "zuzu-syntax"));
    }

    #[test]
    fn coalesces_touching_syntax_diagnostics() {
        let mut document = Document::parse("file:///bad.zzs".to_string(), "say 1;\n".to_string());
        document.diagnostics = vec![
            Diagnostic {
                range: Range::new(Position::new(0, 4), Position::new(0, 6)),
                severity: DiagnosticSeverity::Error,
                source: "zuzu-syntax",
                code: "parse-error",
                message: "Could not parse this ZuzuScript syntax".to_string(),
            },
            Diagnostic {
                range: Range::new(Position::new(0, 6), Position::new(0, 6)),
                severity: DiagnosticSeverity::Error,
                source: "zuzu-syntax",
                code: "missing-node",
                message: "Missing `expression`".to_string(),
            },
            Diagnostic {
                range: Range::new(Position::new(1, 0), Position::new(1, 1)),
                severity: DiagnosticSeverity::Warning,
                source: "zuzu-semantic",
                code: "example",
                message: "Keep non-syntax diagnostics separate".to_string(),
            },
        ];

        document.coalesce_syntax_diagnostics();

        let syntax: Vec<_> = document
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.source == "zuzu-syntax")
            .collect();
        assert_eq!(syntax.len(), 1);
        assert_eq!(
            syntax[0].range,
            Range::new(Position::new(0, 4), Position::new(0, 6))
        );
        assert_eq!(syntax[0].code, "parse-error");
        assert!(document
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source == "zuzu-semantic"));
    }

    #[test]
    fn indexes_common_symbols() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "class Foo;\nfunction bar(x) {\n\tlet y := x;\n}\n",
        );
        let symbols = analyzer.document_symbols("file:///example.zzs");
        assert!(symbols.iter().any(|symbol| symbol.name == "Foo"));
        assert!(symbols.iter().any(|symbol| symbol.name == "bar"));
        assert!(symbols.iter().any(|symbol| symbol.name == "x"));
        assert!(symbols.iter().any(|symbol| symbol.name == "y"));
    }

    #[test]
    fn indexes_extensionless_zuzu_shebang_scripts() {
        let root = unique_temp_dir("zuzu-analysis-extensionless-script");
        let scripts = root.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        let script = scripts.join("tool");
        let readme = scripts.join("README");
        fs::write(
            &script,
            "#!/usr/bin/env zuzu\nfunction __main__() {\n\tsay \"ok\";\n}\n",
        )
        .unwrap();
        fs::write(&readme, "This is documentation, not a script.\n").unwrap();

        let analyzer = Analyzer::new(vec![root.clone()]);
        let script_uri = path_to_uri(&script).unwrap();
        assert!(analyzer
            .document_symbols(&script_uri)
            .iter()
            .any(|symbol| symbol.name == "__main__"));
        let readme_uri = path_to_uri(&readme).unwrap();
        assert!(analyzer.document_symbols(&readme_uri).is_empty());

        let _ = fs::remove_file(script);
        let _ = fs::remove_file(readme);
        let _ = fs::remove_dir(scripts);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn detects_unresolved_imports() {
        let mut analyzer = Analyzer::new(Vec::new());
        let diagnostics =
            analyzer.upsert_document("file:///example.zzs", "from missing/module import Thing;\n");
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.source == "zuzu-module")
            .expect("unresolved import diagnostic");
        assert!(diagnostic.message.contains("Could not resolve module"));

        let root = unique_temp_dir("zuzu-analysis-unresolved-import");
        fs::create_dir_all(root.join("modules")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        let script = root.join("scripts").join("example.zzs");
        fs::write(&script, "").unwrap();
        let uri = path_to_uri(&script).unwrap();
        let mut analyzer = Analyzer::new(vec![root.clone()]);
        let diagnostics = analyzer.upsert_document(&uri, "from missing/module import Thing;\n");
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.source == "zuzu-module")
            .expect("unresolved import diagnostic with module roots");
        assert!(diagnostic.message.contains("Searched module roots"));
        assert!(diagnostic
            .message
            .contains(&path_display(&root.join("modules"))));

        let _ = fs::remove_file(script);
        let _ = fs::remove_dir(root.join("scripts"));
        let _ = fs::remove_dir(root.join("modules"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn resolves_script_modules_with_zzs_extension() {
        let root = unique_temp_dir("zuzu-analysis-zzs-module");
        let module_dir = root.join("modules").join("app");
        fs::create_dir_all(&module_dir).unwrap();
        fs::write(module_dir.join("script.zzs"), "function run() {}\n").unwrap();

        let analyzer = Analyzer::new(vec![root.clone()]);
        assert!(analyzer.workspace().resolve_module("app/script"));

        let _ = fs::remove_file(module_dir.join("script.zzs"));
        let _ = fs::remove_dir(module_dir);
        let _ = fs::remove_dir(root.join("modules"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn does_not_use_umbrella_checkout_stdlib_layout_as_module_root() {
        let root = unique_temp_dir("zuzu-analysis-no-checkout-layout");
        let module_dir = root
            .join("zuzu-perl")
            .join("stdlib")
            .join("modules")
            .join("std");
        fs::create_dir_all(&module_dir).unwrap();
        fs::write(module_dir.join("fake.zzm"), "function fake() {}\n").unwrap();

        let analyzer = Analyzer::new(vec![root.clone()]);
        assert!(!analyzer.workspace().resolve_module("std/fake"));

        let _ = fs::remove_file(module_dir.join("fake.zzm"));
        let _ = fs::remove_dir(module_dir);
        let _ = fs::remove_dir(root.join("zuzu-perl").join("stdlib").join("modules"));
        let _ = fs::remove_dir(root.join("zuzu-perl").join("stdlib"));
        let _ = fs::remove_dir(root.join("zuzu-perl"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn detects_duplicate_top_level_declarations() {
        let mut analyzer = Analyzer::new(Vec::new());
        let diagnostics = analyzer.upsert_document(
            "file:///example.zzs",
            "function same() {}\nfunction same() {}\nfunction local() {\n\tlet x := 1;\n\tlet x := 2;\n}\n",
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source == "zuzu-semantic" && diagnostic.code == "duplicate-declaration"
        }));
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "duplicate-declaration")
                .count(),
            1
        );
    }

    #[test]
    fn diagnoses_undefined_locals_conservatively() {
        let mut analyzer = Analyzer::new(Vec::new());
        let diagnostics = analyzer.upsert_document(
            "file:///example.zzs",
            "function main() {\n\tlet total := missing;\n\tsay total;\n}\n",
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source == "zuzu-semantic"
                && diagnostic.code == "undefined-local"
                && diagnostic.message.contains("missing")
        }));

        let diagnostics = analyzer.upsert_document(
            "file:///example.zzs",
            concat!(
                "function add(value) {\n\treturn value;\n}\n",
                "function main(obj) {\n",
                "\tlet value := 1;\n",
                "\tadd(right: value);\n",
                "\tsay {missing: value};\n",
                "\tobj.method(value);\n",
                "}\n",
            ),
        );
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "undefined-local"));
    }

    #[test]
    fn completes_fn_keyword() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document("file:///example.zzs", "let cb := f\n");
        let labels: Vec<_> = analyzer
            .completions("file:///example.zzs", Position::new(0, 11))
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert!(labels.iter().any(|label| label == "fn"));
    }

    #[test]
    fn provides_hover_for_keywords_and_builtin_statements() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "if (ready) {\n\tlet cb := fn (x) { x };\n\tsay cb(1);\n}\n",
        );

        let if_hover = analyzer
            .hover("file:///example.zzs", Position::new(0, 1))
            .expect("if keyword hover");
        assert!(if_hover.markdown.contains("conditional"));
        assert_eq!(
            if_hover.range,
            Range::new(Position::new(0, 0), Position::new(0, 2))
        );

        let fn_hover = analyzer
            .hover("file:///example.zzs", Position::new(1, 12))
            .expect("fn keyword hover");
        assert!(fn_hover.markdown.contains("anonymous function"));

        let say_hover = analyzer
            .hover("file:///example.zzs", Position::new(2, 2))
            .expect("say builtin hover");
        assert!(say_hover.markdown.contains("adds a newline"));
    }

    #[test]
    fn provides_hover_for_word_and_symbol_operators() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "let total := 1 + 2;\nlet remainder := total mod 2;\nlet same := total == 3;\n",
        );
        let plus = analyzer
            .hover("file:///example.zzs", Position::new(0, 15))
            .expect("plus operator hover");
        assert!(plus.markdown.contains("addition"));
        assert_eq!(
            plus.range,
            Range::new(Position::new(0, 15), Position::new(0, 16))
        );

        let modulo = analyzer
            .hover("file:///example.zzs", Position::new(1, 24))
            .expect("mod operator hover");
        assert!(modulo.markdown.contains("modulo"));

        let equality = analyzer
            .hover("file:///example.zzs", Position::new(2, 19))
            .expect("equality operator hover");
        assert!(equality.markdown.contains("Type-aware equality"));
    }

    #[test]
    fn provides_hover_for_unicode_operators() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document("file:///example.zzs", "let scaled := 2 × 3;\n");
        let hover = analyzer
            .hover("file:///example.zzs", Position::new(0, 16))
            .expect("unicode multiplication hover");
        assert!(hover.markdown.contains("multiplication"));
        assert_eq!(
            hover.range,
            Range::new(Position::new(0, 16), Position::new(0, 17))
        );
    }

    #[test]
    fn provides_signature_help_for_open_document_functions() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "function add(a, b) {\n\treturn a + b;\n}\nfunction main() {\n\tadd(1, 2);\n}\n",
        );
        let help = analyzer
            .signature_help("file:///example.zzs", Position::new(4, 9))
            .expect("signature help");
        assert_eq!(help.label, "add(a, b)");
        assert_eq!(help.parameters, vec!["a", "b"]);
        assert_eq!(help.active_parameter, 1);
    }

    #[test]
    fn includes_variadic_collectors_in_signature_help() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "function collect(head ... tail) {\n\treturn tail;\n}\nfunction main() {\n\tcollect(1, 2, 3);\n}\n",
        );
        let help = analyzer
            .signature_help("file:///example.zzs", Position::new(4, 14))
            .expect("signature help");
        assert_eq!(help.label, "collect(head, tail)");
        assert_eq!(help.parameters, vec!["head", "tail"]);
        assert_eq!(help.active_parameter, 1);
    }

    #[test]
    fn provides_signature_help_for_method_calls() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "class Box {\n\tmethod scale(value, factor) {\n\t\treturn value;\n\t}\n}\nfunction main() {\n\tlet box := new Box();\n\tbox.scale(2, 3);\n}\n",
        );
        let help = analyzer
            .signature_help("file:///example.zzs", Position::new(7, 15))
            .expect("method signature help");
        assert_eq!(help.label, "scale(value, factor)");
        assert_eq!(help.parameters, vec!["value", "factor"]);
        assert_eq!(help.active_parameter, 1);

        let hints = analyzer.inlay_hints(
            "file:///example.zzs",
            Range::new(Position::new(0, 0), Position::new(8, 0)),
        );
        assert!(hints
            .iter()
            .any(|hint| hint.label == "value:" && hint.position == Position::new(7, 11)));
        assert!(hints
            .iter()
            .any(|hint| hint.label == "factor:" && hint.position == Position::new(7, 14)));
    }

    #[test]
    fn provides_signature_help_for_field_accessors() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "class Person {\n\tlet String name with get, set, clear, has := \"Anon\";\n}\nfunction main() {\n\tlet person := new Person();\n\tperson.set_name(\"Zia\");\n\tperson.clear_name();\n}\n",
        );
        let setter = analyzer
            .signature_help("file:///example.zzs", Position::new(5, 20))
            .expect("set accessor signature help");
        assert_eq!(setter.label, "set_name(value)");
        assert_eq!(setter.parameters, vec!["value"]);

        let clearer = analyzer
            .signature_help("file:///example.zzs", Position::new(6, 19))
            .expect("clear accessor signature help");
        assert_eq!(clearer.label, "clear_name()");
        assert!(clearer.parameters.is_empty());

        let hints = analyzer.inlay_hints(
            "file:///example.zzs",
            Range::new(Position::new(0, 0), Position::new(8, 0)),
        );
        assert!(hints
            .iter()
            .any(|hint| hint.label == "value:" && hint.position == Position::new(5, 17)));
    }

    #[test]
    fn provides_parameter_inlay_hints_for_plain_call_arguments() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "function add(left, right) {\n\treturn left + right;\n}\nfunction main() {\n\tadd(1, right: 2, ...rest);\n}\n",
        );
        let hints = analyzer.inlay_hints(
            "file:///example.zzs",
            Range::new(Position::new(0, 0), Position::new(5, 0)),
        );
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].label, "left:");
        assert_eq!(hints[0].position, Position::new(4, 5));
    }

    #[test]
    fn builds_tree_sitter_selection_range_chain() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "function main() {\n\tlet total := 1;\n\tsay total;\n}\n",
        );

        let ranges = analyzer.selection_ranges("file:///example.zzs", &[Position::new(2, 7)]);
        assert_eq!(ranges.len(), 1);
        assert_eq!(
            ranges[0].range,
            Range::new(Position::new(2, 5), Position::new(2, 10))
        );
        assert!(ranges[0].parent.is_some());
    }

    #[test]
    fn exposes_semantic_tokens_for_declarations() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "class Thing;\nfunction main(arg) {\n\tlet total := arg;\n}\n",
        );

        let tokens = analyzer.semantic_tokens("file:///example.zzs");
        assert!(tokens.iter().any(|token| {
            token.kind == SymbolKind::Class
                && token.range == Range::new(Position::new(0, 6), Position::new(0, 11))
        }));
        assert!(tokens
            .iter()
            .any(|token| token.kind == SymbolKind::Function));
        assert!(tokens
            .iter()
            .any(|token| token.kind == SymbolKind::Parameter));
        assert!(tokens
            .iter()
            .any(|token| token.kind == SymbolKind::Variable));
    }

    #[test]
    fn builds_simple_call_hierarchy() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "function helper() {\n\treturn 1;\n}\nfunction main() {\n\thelper();\n}\n",
        );

        let prepared = analyzer.prepare_call_hierarchy("file:///example.zzs", Position::new(3, 10));
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0].name, "main");

        let outgoing = analyzer.outgoing_calls(&prepared[0]);
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].to.name, "helper");
        assert_eq!(
            outgoing[0].from_ranges,
            vec![Range::new(Position::new(4, 1), Position::new(4, 7))]
        );

        let helper = analyzer
            .prepare_call_hierarchy("file:///example.zzs", Position::new(0, 11))
            .pop()
            .expect("helper hierarchy item");
        let incoming = analyzer.incoming_calls(&helper);
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "main");
    }

    #[test]
    fn builds_type_hierarchy_for_classes_and_traits() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///types.zzm",
            concat!(
                "trait Printable {}\n",
                "class Base;\n",
                "class Derived extends Base but Printable;\n",
            ),
        );

        let derived = analyzer.prepare_type_hierarchy("file:///types.zzm", Position::new(2, 8));
        assert_eq!(derived.len(), 1);
        assert_eq!(derived[0].name, "Derived");

        let supertypes = analyzer.supertypes(&derived[0]);
        let supertype_names: Vec<_> = supertypes.iter().map(|item| item.name.as_str()).collect();
        assert!(supertype_names.contains(&"Base"));
        assert!(supertype_names.contains(&"Printable"));

        let base = analyzer.prepare_type_hierarchy("file:///types.zzm", Position::new(1, 8));
        let subtypes = analyzer.subtypes(&base[0]);
        assert_eq!(subtypes[0].name, "Derived");
    }

    #[test]
    fn finds_references_across_open_documents() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///one.zzs",
            "function main() {\n\tlet total := 1;\n\tsay total;\n}\n",
        );
        analyzer.upsert_document("file:///two.zzs", "say total;\n");

        let references = analyzer.references("file:///one.zzs", Position::new(1, 7), true);
        assert_eq!(references.len(), 3);
        assert!(references
            .iter()
            .any(|location| location.uri == "file:///two.zzs"));

        let references = analyzer.references("file:///one.zzs", Position::new(1, 7), false);
        assert_eq!(references.len(), 2);
    }

    #[test]
    fn prepares_and_builds_rename_edits() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "function main() {\n\tlet total := 1;\n\tsay total;\n}\n",
        );

        assert_eq!(
            analyzer.prepare_rename("file:///example.zzs", Position::new(1, 7)),
            Some(Range::new(Position::new(1, 5), Position::new(1, 10)))
        );

        let edits = analyzer
            .rename("file:///example.zzs", Position::new(1, 7), "sum")
            .unwrap();
        assert_eq!(edits.len(), 2);
        assert!(edits.iter().all(|edit| edit.edit.new_text == "sum"));

        assert!(matches!(
            analyzer.rename("file:///example.zzs", Position::new(1, 7), "2bad"),
            Err(RenameError::InvalidIdentifier(_))
        ));
    }

    #[test]
    fn builds_unresolved_import_removal_fix() {
        let mut analyzer = Analyzer::new(Vec::new());
        analyzer.upsert_document(
            "file:///example.zzs",
            "from missing/module import Thing;\nsay 1;\n",
        );

        let fixes = analyzer.import_fixes(
            "file:///example.zzs",
            Range::new(Position::new(0, 5), Position::new(0, 5)),
        );
        assert_eq!(fixes.len(), 1);
        let ImportFixAction::Edit(edit) = &fixes[0].action else {
            panic!("expected edit fix");
        };
        assert_eq!(edit.edit.new_text, "");
        assert_eq!(
            edit.edit.range,
            Range::new(Position::new(0, 0), Position::new(1, 0))
        );
    }

    #[test]
    fn diagnoses_and_fixes_wholly_unused_imports() {
        let root = unique_temp_dir("zuzu-analysis-unused-import");
        let module_dir = root.join("modules").join("lib");
        fs::create_dir_all(&module_dir).unwrap();
        fs::write(
            module_dir.join("stuff.zzm"),
            "class Thing;\nclass Used;\nclass Unused;\n",
        )
        .unwrap();
        let mut analyzer = Analyzer::new(vec![root.clone()]);

        let diagnostics = analyzer.upsert_document(
            "file:///example.zzs",
            "from lib/stuff import Thing;\nsay 1;\n",
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source == "zuzu-semantic" && diagnostic.code == "unused-import"
        }));

        let fixes = analyzer.import_fixes(
            "file:///example.zzs",
            Range::new(Position::new(0, 5), Position::new(0, 5)),
        );
        assert!(fixes
            .iter()
            .any(|fix| fix.title == "Remove unused import `lib/stuff`"));

        let diagnostics = analyzer.upsert_document(
            "file:///mixed.zzs",
            "from lib/stuff import Used, Unused;\nsay Used;\n",
        );
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "unused-import"));

        let _ = fs::remove_file(module_dir.join("stuff.zzm"));
        let _ = fs::remove_dir(module_dir);
        let _ = fs::remove_dir(root.join("modules"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn handles_try_import_diagnostics_conservatively() {
        let root = unique_temp_dir("zuzu-analysis-try-import");
        let module_dir = root.join("modules").join("lib");
        fs::create_dir_all(&module_dir).unwrap();
        fs::write(module_dir.join("stuff.zzm"), "class Thing;\n").unwrap();
        let mut analyzer = Analyzer::new(vec![root.clone()]);

        let diagnostics = analyzer.upsert_document(
            "file:///missing.zzs",
            "from missing/module try import Thing;\nsay 1;\n",
        );
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "unresolved-import"));
        let fixes = analyzer.import_fixes(
            "file:///missing.zzs",
            Range::new(Position::new(0, 5), Position::new(0, 5)),
        );
        assert!(fixes.is_empty());

        let diagnostics = analyzer.upsert_document(
            "file:///resolved.zzs",
            "from lib/stuff try import Thing;\nsay Thing;\n",
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source == "zuzu-semantic" && diagnostic.code == "suspicious-try-import"
        }));

        let _ = fs::remove_file(module_dir.join("stuff.zzm"));
        let _ = fs::remove_dir(module_dir);
        let _ = fs::remove_dir(root.join("modules"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn warns_about_runtime_specific_module_imports() {
        let mut analyzer = Analyzer::new(Vec::new());
        let diagnostics =
            analyzer.upsert_document("file:///example.zzs", "from perl import eval;\nsay 1;\n");

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source == "zuzu-semantic" && diagnostic.code == "runtime-specific-module"
        }));
    }

    #[test]
    fn diagnoses_missing_distribution_dependencies_for_external_modules() {
        let root = unique_temp_dir("zuzu-analysis-missing-dependency");
        let external = unique_temp_dir("zuzu-analysis-external-modules");
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(external.join("dep")).unwrap();
        fs::create_dir_all(external.join("std")).unwrap();
        fs::write(
            root.join("zuzu-distribution.json"),
            "{\n\t\"name\": \"missing-dependency-fixture\",\n\t\"version\": \"0.0.1\",\n\t\"author\": \"Example Author\",\n\t\"license\": \"MIT\",\n\t\"abstract\": \"Missing dependency fixture.\"\n}\n",
        )
        .unwrap();
        fs::write(external.join("dep").join("stuff.zzm"), "class Thing;\n").unwrap();
        fs::write(external.join("std").join("demo.zzm"), "class StdThing;\n").unwrap();
        let script = root.join("scripts").join("app.zzs");
        fs::write(
            &script,
            "from dep/stuff import Thing;\nfrom std/demo import StdThing;\nsay Thing;\nsay StdThing;\n",
        )
        .unwrap();
        let uri = path_to_uri(&script).unwrap();

        let analyzer = Analyzer::with_module_roots(vec![root.clone()], vec![external.clone()]);
        let diagnostics = analyzer.diagnostics(&uri);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source == "zuzu-package" && diagnostic.code == "missing-dependency"
        }));
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing-dependency" && diagnostic.message.contains("std/demo")
        }));
        let fixes =
            analyzer.import_fixes(&uri, Range::new(Position::new(0, 5), Position::new(0, 5)));
        let dependency_fix = fixes
            .iter()
            .find(|fix| fix.title == "Add `dep/stuff` to zuzu-distribution.json dependencies")
            .expect("dependency fix");
        let ImportFixAction::Edit(edit) = &dependency_fix.action else {
            panic!("expected metadata edit");
        };
        assert!(edit.uri.ends_with("/zuzu-distribution.json"));
        let fixed_metadata = serde_json::from_str::<serde_json::Value>(&edit.edit.new_text)
            .expect("valid fixed metadata");
        assert_eq!(fixed_metadata["dependencies"]["dep/stuff"], "0");

        fs::write(
            root.join("zuzu-distribution.json"),
            "{\n\t\"name\": \"missing-dependency-fixture\",\n\t\"version\": \"0.0.1\",\n\t\"author\": \"Example Author\",\n\t\"license\": \"MIT\",\n\t\"abstract\": \"Missing dependency fixture.\",\n\t\"dependencies\": {\n\t\t\"dep\": \"0\"\n\t}\n}\n",
        )
        .unwrap();
        let analyzer = Analyzer::with_module_roots(vec![root.clone()], vec![external.clone()]);
        let diagnostics = analyzer.diagnostics(&uri);
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "missing-dependency"));

        let _ = fs::remove_file(&script);
        let _ = fs::remove_file(root.join("zuzu-distribution.json"));
        let _ = fs::remove_file(external.join("dep").join("stuff.zzm"));
        let _ = fs::remove_file(external.join("std").join("demo.zzm"));
        let _ = fs::remove_dir(root.join("scripts"));
        let _ = fs::remove_dir(root);
        let _ = fs::remove_dir(external.join("dep"));
        let _ = fs::remove_dir(external.join("std"));
        let _ = fs::remove_dir(external);
    }

    #[test]
    fn reports_package_health_without_running_tools() {
        let root = unique_temp_dir("zuzu-analysis-package-report");
        fs::create_dir_all(root.join("modules").join("local")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("zuzu-distribution.json"),
            "{\n\t\"name\": \"package-report-fixture\",\n\t\"version\": \"0.0.1\",\n\t\"author\": \"Example Author\",\n\t\"license\": \"MIT\",\n\t\"abstract\": \"Package report fixture.\",\n\t\"dependencies\": {\n\t\t\"dep\": \"0\"\n\t}\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("modules").join("local").join("thing.zzm"),
            "class Thing;\n",
        )
        .unwrap();
        let script = root.join("scripts").join("app.zzs");
        fs::write(&script, "from missing/module import Thing;\nsay Thing;\n").unwrap();

        let analyzer = Analyzer::new(vec![root.clone()]);
        let report = analyzer.package_report(Some(&script));

        assert_eq!(report.root, Some(path_display(&root)));
        assert_eq!(report.dependencies, vec!["dep"]);
        assert!(report
            .module_roots
            .iter()
            .any(|module_root| module_root.ends_with("/modules")));
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "unresolved-import"));

        let _ = fs::remove_file(&script);
        let _ = fs::remove_file(root.join("modules").join("local").join("thing.zzm"));
        let _ = fs::remove_file(root.join("zuzu-distribution.json"));
        let _ = fs::remove_dir(root.join("scripts"));
        let _ = fs::remove_dir(root.join("modules").join("local"));
        let _ = fs::remove_dir(root.join("modules"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn diagnoses_malformed_distribution_metadata() {
        let root = unique_temp_dir("zuzu-analysis-bad-metadata");
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("zuzu-distribution.json"), "{ not json\n").unwrap();
        let metadata_uri = path_to_uri(&root.join("zuzu-distribution.json")).unwrap();

        let analyzer = Analyzer::new(vec![root.clone()]);
        let diagnostics = analyzer.diagnostics(&metadata_uri);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source == "zuzu-package" && diagnostic.code == "metadata-invalid-json"
        }));

        let report = analyzer.package_report(Some(&root.join("zuzu-distribution.json")));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.uri == metadata_uri && diagnostic.code == "metadata-invalid-json"
        }));

        fs::write(
            root.join("zuzu-distribution.json"),
            "{\n\t\"name\": \"bad-metadata\",\n\t\"dependencies\": []\n}\n",
        )
        .unwrap();
        let analyzer = Analyzer::new(vec![root.clone()]);
        let diagnostics = analyzer.diagnostics(&metadata_uri);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "metadata-invalid-dependencies"));

        fs::write(root.join("zuzu-distribution.json"), "[]\n").unwrap();
        let analyzer = Analyzer::new(vec![root.clone()]);
        let diagnostics = analyzer.diagnostics(&metadata_uri);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "metadata-root-not-object"));

        fs::write(
            root.join("zuzu-distribution.json"),
            "{\n\t\"dependencies\": {\n\t\t\"dep\": \"0\"\n\t}\n}\n",
        )
        .unwrap();
        let analyzer = Analyzer::new(vec![root.clone()]);
        let diagnostics = analyzer.diagnostics(&metadata_uri);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "metadata-missing-name"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "metadata-missing-version"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "metadata-missing-author"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "metadata-missing-license"));
        let report = analyzer.package_report(Some(&root.join("zuzu-distribution.json")));
        assert_eq!(report.dependencies, vec!["dep"]);

        fs::write(
            root.join("zuzu-distribution.json"),
            "{\n\t\"name\": \"bad-metadata\",\n\t\"version\": \"0.0.1\",\n\t\"author\": \"Example Author\",\n\t\"license\": \"MIT\",\n\t\"abstract\": \"Bad dependency fixture.\",\n\t\"dependencies\": {\n\t\t\"\": \"0\",\n\t\t\"dep\": 1\n\t}\n}\n",
        )
        .unwrap();
        let analyzer = Analyzer::new(vec![root.clone()]);
        let diagnostics = analyzer.diagnostics(&metadata_uri);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "metadata-invalid-dependency-name"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "metadata-invalid-dependency-version"));

        fs::write(
            root.join("zuzu-distribution.json"),
            "{\n\t\"name\": \"-bad-metadata\",\n\t\"version\": \"-0.0.1\",\n\t\"author\": \"Example Author\",\n\t\"license\": \"MIT\",\n\t\"status\": \"ancient\",\n\t\"repo\": \"ftp://example.test/repo\",\n\t\"abstract\": 42,\n\t\"dependencies\": {\n\t\t\"bad//dep\": \"0\"\n\t}\n}\n",
        )
        .unwrap();
        let analyzer = Analyzer::new(vec![root.clone()]);
        let diagnostics = analyzer.diagnostics(&metadata_uri);
        for code in [
            "metadata-invalid-name",
            "metadata-invalid-version",
            "metadata-invalid-status",
            "metadata-invalid-repo",
            "metadata-invalid-abstract",
            "metadata-invalid-dependency-name",
        ] {
            assert!(
                diagnostics.iter().any(|diagnostic| diagnostic.code == code),
                "expected diagnostic code {code}, got {diagnostics:?}"
            );
        }
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "metadata-invalid-abstract"
                && diagnostic.severity == DiagnosticSeverity::Warning
        }));

        let _ = fs::remove_file(root.join("zuzu-distribution.json"));
        let _ = fs::remove_dir(root.join("scripts"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn builds_missing_module_creation_fix_under_workspace_modules() {
        let root = unique_temp_dir("zuzu-analysis-create-module-fix");
        fs::create_dir_all(root.join("modules")).unwrap();
        let mut analyzer = Analyzer::new(vec![root.clone()]);
        analyzer.upsert_document(
            "file:///example.zzs",
            "from missing/module import Thing;\nsay 1;\n",
        );

        let fixes = analyzer.import_fixes(
            "file:///example.zzs",
            Range::new(Position::new(0, 5), Position::new(0, 5)),
        );
        assert!(fixes.iter().any(|fix| {
            matches!(
                &fix.action,
                ImportFixAction::CreateModule { path }
                    if path == &root.join("modules").join("missing/module.zzm")
            )
        }));

        let _ = fs::remove_dir(root.join("modules"));
        let _ = fs::remove_dir(root);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
    }
}
