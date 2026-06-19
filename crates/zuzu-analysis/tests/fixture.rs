use std::path::PathBuf;

use zuzu_analysis::{Analyzer, ImportFixAction, Position, Range};

#[test]
fn resolves_fixture_modules_and_provides_symbols() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("workspaces")
        .join("basic");
    let source = std::fs::read_to_string(root.join("scripts/demo.zzs")).unwrap();
    let mut analyzer = Analyzer::new(vec![root.clone()]);
    let indexed_symbols = analyzer.workspace_symbols("Calculator");
    assert!(indexed_symbols
        .iter()
        .any(|symbol| symbol.name == "Calculator"
            && symbol.uri.ends_with("/modules/example/math.zzm")));

    let diagnostics = analyzer.upsert_document("file:///demo.zzs", source);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.source != "zuzu-module"),
        "unexpected module diagnostics: {diagnostics:#?}"
    );

    let symbols = analyzer.document_symbols("file:///demo.zzs");
    assert!(symbols.iter().any(|symbol| symbol.name == "__main__"));

    let completions = analyzer.completions("file:///demo.zzs", Position::new(3, 5));
    assert!(completions.iter().any(|item| item.label == "fn"));
    assert!(completions.iter().any(|item| item.label == "example/math"));

    let links = analyzer.document_links("file:///demo.zzs");
    assert_eq!(links.len(), 1);
    assert!(links[0].target.ends_with("/modules/example/math.zzm"));

    let definition = analyzer
        .definition("file:///demo.zzs", Position::new(3, 15))
        .expect("definition from indexed module");
    assert!(definition.uri.ends_with("/modules/example/math.zzm"));

    let help = analyzer
        .signature_help("file:///demo.zzs", Position::new(3, 20))
        .expect("signature help from indexed module");
    assert_eq!(help.label, "add(a, b)");

    let hints = analyzer.inlay_hints(
        "file:///demo.zzs",
        Range::new(Position::new(0, 0), Position::new(6, 0)),
    );
    assert!(hints.iter().any(|hint| hint.label == "a:"));

    let graph = analyzer.dependency_graph();
    assert!(graph
        .nodes
        .iter()
        .any(|node| node.id == "example/math" && node.kind == "module"));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == "file:///demo.zzs" && edge.to == "example/math" && edge.resolved
    }));

    let diagnostics = analyzer.upsert_document(
        "file:///needs-import.zzs",
        "function __main__() {\n\tlet calculator := Calculator;\n}\n".to_string(),
    );
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "undefined-local")
        .expect("undefined local diagnostic");
    let fixes = analyzer.import_fixes("file:///needs-import.zzs", diagnostic.range);
    let fix = fixes
        .iter()
        .find(|fix| fix.title == "Import `Calculator` from `example/math`")
        .expect("missing import fix");
    let ImportFixAction::Edit(edit) = &fix.action else {
        panic!("expected text edit fix");
    };
    assert_eq!(edit.edit.range.start, Position::new(0, 0));
    assert_eq!(edit.edit.new_text, "from example/math import Calculator;\n");
}

#[test]
fn resolves_runtime_module_roots_and_optional_imports() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("workspaces")
        .join("basic");
    let runtime_modules = root.join("runtime-modules");
    let mut analyzer = Analyzer::with_module_roots(vec![root], vec![runtime_modules]);
    assert!(analyzer
        .workspace()
        .known_modules()
        .any(|module| module == "std/demo"));

    let diagnostics = analyzer.upsert_document(
        "file:///stdlib-and-optional.zzs",
        "from std/demo import StdThing;\nfrom missing/optional try import OptionalThing;\n\nfunction __main__() {\n\tlet thing := StdThing;\n\tlet optional := OptionalThing;\n}\n".to_string(),
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "unresolved-import"
                && diagnostic.code != "suspicious-try-import"),
        "unexpected import diagnostics: {diagnostics:#?}"
    );

    let links = analyzer.document_links("file:///stdlib-and-optional.zzs");
    assert_eq!(links.len(), 1);
    assert!(links[0].target.ends_with("/runtime-modules/std/demo.zzm"));
}
