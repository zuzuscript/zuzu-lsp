use std::fs;
use std::time::{Duration, Instant};

use zuzu_analysis::Analyzer;

#[test]
fn indexes_representative_module_workspace_promptly() {
    let root = unique_temp_dir("zuzu-analysis-performance");
    let modules = root.join("modules").join("perf");
    fs::create_dir_all(&modules).unwrap();

    for index in 0..250 {
        fs::write(
            modules.join(format!("module_{index}.zzm")),
            format!(
                "module perf/module_{index};\n\
                 import perf/module_0;\n\
                 function value_{index}() {{\n\
                 \treturn {index};\n\
                 }}\n"
            ),
        )
        .unwrap();
    }

    let started = Instant::now();
    let analyzer = Analyzer::new(vec![root.clone()]);
    let symbols = analyzer.workspace_symbols("value_249");
    let elapsed = started.elapsed();

    assert!(
        symbols.iter().any(|symbol| symbol.name == "value_249"),
        "generated module symbol should be indexed"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "indexing generated workspace took {elapsed:?}"
    );

    let _ = fs::remove_dir_all(root);
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}
