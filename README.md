# zuzu-lsp

`zuzu-lsp` is an editor-independent language server for ZuzuScript.

The server is intentionally layered:

- `zuzu-analysis` owns parsing, diagnostics, symbols, folding, hover,
  completion, and simple module resolution.
- `zuzu-toolchain` discovers existing command-line tools and wraps them.
- `zuzu-lsp` translates those reusable pieces into LSP requests and
  notifications.

The first implementation is a Phase 1 language server. It uses
`tree-sitter-zuzu` for fast syntax-aware features, reports syntax diagnostics
and unresolved imports, provides document/workspace symbols, folding ranges,
hover, completion, local definition lookup, and delegates formatting to
`zuzu-tidy.pl` when it can be found.

## Usage

```sh
cargo run -p zuzu-lsp -- --stdio
```

For a quick local health check:

```sh
cargo run -p zuzu-lsp -- doctor
```
