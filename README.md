# zuzu-lsp

`zuzu-lsp` is an editor-independent language server for ZuzuScript.

The server is intentionally layered:

- `zuzu-analysis` owns parsing, diagnostics, symbols, folding, hover,
  completion, and simple module resolution.
- `zuzu-toolchain` discovers existing command-line tools and wraps them.
- `zuzu-lsp` translates those reusable pieces into LSP requests and
  notifications.

The current implementation covers the useful MVP and selected stronger editor
features from the planning document. It uses `tree-sitter-zuzu` for fast
syntax-aware features, reports syntax diagnostics, semantic diagnostics, package
metadata diagnostics, runtime parser diagnostics, toolchain hints, and
unresolved imports, provides
document/workspace symbols, folding ranges, selection ranges, hover,
completion, definition, references, rename, semantic tokens, call/type
hierarchy, document links, code actions, code lenses, inlay hints, and signature
help. Formatting delegates to `zuzu-tidy.pl` when it can be found.

Toolchain commands are explicit user actions. The server wraps `zuzuprove`,
`pod_parse` with `zuzudoc.pl` fallback, `zuzubox verify`, package reporting,
dependency graph reporting, and REPL launch instructions without running project
code during normal editing.
When the workspace is trusted and a `zuzu` binary is available, normal editing
may run parse-only `zuzu --lint -e` checks to surface exact runtime parser
errors and semantic warnings; these diagnostics supplement the Tree-sitter
error recovery path.

Workspace indexing follows LSP workspace folders, refreshes after configuration
or watched-file changes, and combines workspace modules with the module search
paths discovered from the active Zuzu toolchain.
Workspace diagnostic requests honour client-supplied work-done progress tokens
so clients can surface broad diagnostic collection as an explicit operation.
Push diagnostics and workspace diagnostic reports include open-document versions
when the client supplied them.

Open documents are stored as rope-backed snapshots so incremental edits can be
applied efficiently while still preserving LSP UTF-16 position semantics. The
snapshots also maintain incrementally edited Tree-sitter parse trees for fast
syntax-aware editor state.
Tree-sitter syntax diagnostics are coalesced when their ranges touch or overlap
so incomplete edits produce useful errors instead of diagnostic floods.

## Usage

Install from Git:

```sh
cargo install --git https://github.com/zuzuscript/zuzu-lsp.git zuzu-lsp
```

Tagged releases also publish standalone archives for Linux, macOS, and Windows.
Those archives contain the `zuzu-lsp` binary and this README.

Run the installed server over stdio:

```sh
zuzu-lsp --stdio
```

For a quick local health check:

```sh
zuzu-lsp doctor
```

During local development, run the same commands through Cargo:

```sh
cargo run -p zuzu-lsp -- --stdio
cargo run -p zuzu-lsp -- doctor
```

## Configuration

Clients may pass a `zuzu` settings object through initialization options or
`workspace/didChangeConfiguration`.

```json
{
  "zuzu": {
    "moduleRoots": ["vendor/modules"],
    "runtimeParserDiagnostics": true
  }
}
```

`moduleRoots` adds extra module search roots ahead of toolchain-discovered
paths. Relative paths are resolved against the first workspace root.
`runtimeParserDiagnostics` controls trusted-workspace `zuzu --lint -e`
diagnostics; it defaults to `true`.
