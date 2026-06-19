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
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
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

#[derive(Debug, Clone)]
pub struct Document {
    uri: String,
    text: String,
    line_offsets: Vec<usize>,
    tree: Tree,
    diagnostics: Vec<Diagnostic>,
    symbols: Vec<Symbol>,
    imports: Vec<Import>,
}

#[derive(Debug, Clone)]
struct Import {
    module: String,
    range: Range,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    roots: Vec<PathBuf>,
    module_roots: Vec<PathBuf>,
    known_modules: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct Analyzer {
    workspace: Workspace,
    documents: BTreeMap<String, Document>,
}

impl Analyzer {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            workspace: Workspace::new(roots),
            documents: BTreeMap::new(),
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
        let mut document = Document::parse(uri.clone(), text.into());
        document.diagnostics.extend(
            document
                .imports
                .iter()
                .filter(|import| !self.workspace.resolve_module(&import.module))
                .map(|import| Diagnostic {
                    range: import.range,
                    severity: DiagnosticSeverity::Error,
                    source: "zuzu-module",
                    code: "unresolved-import",
                    message: format!(
                        "Could not resolve module `{}` from workspace module paths",
                        import.module
                    ),
                }),
        );
        let diagnostics = document.diagnostics.clone();
        self.documents.insert(uri, document);
        diagnostics
    }

    pub fn remove_document(&mut self, uri: &str) {
        self.documents.remove(uri);
    }

    pub fn document(&self, uri: &str) -> Option<&Document> {
        self.documents.get(uri)
    }

    pub fn diagnostics(&self, uri: &str) -> Vec<Diagnostic> {
        self.document(uri)
            .map(|document| document.diagnostics.clone())
            .unwrap_or_default()
    }

    pub fn document_symbols(&self, uri: &str) -> Vec<Symbol> {
        self.document(uri)
            .map(|document| document.symbols.clone())
            .unwrap_or_default()
    }

    pub fn workspace_symbols(&self, query: &str) -> Vec<Symbol> {
        let query = query.to_lowercase();
        self.documents
            .values()
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

    pub fn hover(&self, uri: &str, position: Position) -> Option<Hover> {
        let document = self.document(uri)?;
        let word = document.word_at(position)?;

        if let Some(description) = describe_keyword_or_builtin(&word) {
            return Some(Hover {
                range: document.word_range(position)?,
                markdown: description.to_string(),
            });
        }

        document
            .symbol_at(position)
            .or_else(|| document.symbols.iter().find(|symbol| symbol.name == word))
            .map(|symbol| Hover {
                range: symbol.selection_range,
                markdown: format_symbol(symbol),
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

    pub fn definition(&self, uri: &str, position: Position) -> Option<Location> {
        let document = self.document(uri)?;
        let word = document.word_at(position)?;
        document
            .symbols
            .iter()
            .find(|symbol| symbol.name == word)
            .or_else(|| {
                self.documents
                    .values()
                    .flat_map(|candidate| candidate.symbols.iter())
                    .find(|symbol| symbol.name == word)
            })
            .map(|symbol| Location {
                uri: symbol.uri.clone(),
                range: symbol.selection_range,
            })
    }
}

impl Workspace {
    pub fn new(mut roots: Vec<PathBuf>) -> Self {
        roots.sort();
        roots.dedup();

        let mut module_roots = Vec::new();
        for root in &roots {
            module_roots.push(root.join("modules"));
            module_roots.push(root.join("stdlib").join("modules"));
            module_roots.push(root.join("zuzu-perl").join("stdlib").join("modules"));
            module_roots.push(root.join("zuzu-rust").join("stdlib").join("modules"));
            module_roots.push(root.join("zuzu-js").join("stdlib").join("modules"));

            if let Some(distribution_root) = find_distribution_root(root) {
                module_roots.push(distribution_root.join("modules"));
                module_roots.push(distribution_root.join("inc"));
            }
        }

        if let Some(paths) = env::var_os("ZUZULIB") {
            module_roots.extend(env::split_paths(&paths));
        }

        module_roots.retain(|path| path.is_dir());
        module_roots.sort();
        module_roots.dedup();

        let known_modules = scan_modules(&module_roots);

        Self {
            roots,
            module_roots,
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

    pub fn resolve_module(&self, module: &str) -> bool {
        let relative = format!("{}.zzm", module);
        self.module_roots
            .iter()
            .any(|root| root.join(&relative).is_file())
    }
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
                self.collect_named_symbol(node, SymbolKind::Function, "function")
            }
            "method_declaration" | "method_predeclaration" => {
                self.collect_named_symbol(node, SymbolKind::Method, "method")
            }
            "class_declaration" => self.collect_named_symbol(node, SymbolKind::Class, "class"),
            "trait_declaration" => self.collect_named_symbol(node, SymbolKind::Trait, "trait"),
            "field_declaration" => self.collect_named_symbol(node, SymbolKind::Field, "field"),
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
                    range,
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

    pub fn folding_ranges(&self) -> Vec<FoldingRange> {
        let mut ranges = Vec::new();
        self.collect_folding(self.tree.root_node(), &mut ranges);
        ranges
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

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "but", "case", "catch", "class", "const", "default", "do", "else",
    "extends", "fn", "for", "from", "function", "if", "import", "in", "let", "method", "new",
    "return", "self", "spawn", "static", "super", "switch", "throw", "trait", "try", "unless",
    "while", "with",
];

const BUILTIN_STATEMENTS: &[&str] = &["assert", "debug", "die", "print", "say", "warn"];

fn describe_keyword_or_builtin(word: &str) -> Option<&'static str> {
    match word {
        "fn" => Some("Anonymous function literal introducer."),
        "function" => Some("Declares a named ZuzuScript function."),
        "method" => Some("Declares a method inside a class or trait."),
        "class" => Some("Declares a ZuzuScript class."),
        "trait" => Some("Declares a ZuzuScript trait."),
        "from" | "import" => Some("Imports symbols from a ZuzuScript module."),
        "say" | "print" | "warn" | "assert" | "debug" | "die" => {
            Some("ZuzuScript builtin statement.")
        }
        word if KEYWORDS.contains(&word) => Some("ZuzuScript keyword."),
        _ => None,
    }
}

fn format_symbol(symbol: &Symbol) -> String {
    let detail = symbol.detail.as_deref().unwrap_or("symbol");
    format!("`{}`\n\nZuzuScript {}.", symbol.name, detail)
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

fn is_word_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn position_ge(left: Position, right: Position) -> bool {
    (left.line, left.character) >= (right.line, right.character)
}

fn position_le(left: Position, right: Position) -> bool {
    (left.line, left.character) <= (right.line, right.character)
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

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    Url::parse(uri).ok()?.to_file_path().ok()
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
    fn detects_unresolved_imports() {
        let mut analyzer = Analyzer::new(Vec::new());
        let diagnostics =
            analyzer.upsert_document("file:///example.zzs", "from missing/module import Thing;\n");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source == "zuzu-module"));
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
}
