use std::path::PathBuf;

use zuzu_analysis::{Analyzer, Position};

#[test]
fn resolves_fixture_modules_and_provides_symbols() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("workspaces")
        .join("basic");
    let source = std::fs::read_to_string(root.join("scripts/demo.zzs")).unwrap();
    let mut analyzer = Analyzer::new(vec![root]);
    let diagnostics = analyzer.upsert_document("file:///demo.zzs", source);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.source != "zuzu-module"),
        "unexpected module diagnostics: {diagnostics:#?}"
    );

    let symbols = analyzer.document_symbols("file:///demo.zzs");
    assert!(symbols.iter().any(|symbol| symbol.name == "main"));

    let completions = analyzer.completions("file:///demo.zzs", Position::new(3, 5));
    assert!(completions.iter().any(|item| item.label == "fn"));
    assert!(completions.iter().any(|item| item.label == "example/math"));
}
