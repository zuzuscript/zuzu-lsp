use std::fs;
use std::time::{Duration, Instant};

use zuzu_analysis::Analyzer;

#[test]
fn indexes_representative_module_workspace_promptly() {
    let root = unique_temp_dir("zuzu-analysis-performance");
    let _ = fs::remove_dir_all(&root);
    let modules = root.join("modules").join("perf");
    let scripts = root.join("scripts");
    let tests = root.join("tests");
    let inc = root.join("inc").join("perf");
    let runtime_modules = root.join("runtime-modules");
    let stdlib = runtime_modules.join("std");
    fs::create_dir_all(&modules).unwrap();
    fs::create_dir_all(&scripts).unwrap();
    fs::create_dir_all(&tests).unwrap();
    fs::create_dir_all(&inc).unwrap();
    fs::create_dir_all(&stdlib).unwrap();
    fs::write(
        root.join("zuzu-distribution.json"),
        "{\n\t\"name\": \"performance-fixture\",\n\t\"version\": \"0.0.1\",\n\t\"author\": \"Example Author\",\n\t\"license\": \"MIT\",\n\t\"abstract\": \"Performance fixture.\",\n\t\"dependencies\": {}\n}\n",
    )
    .unwrap();

    for index in 0..120 {
        fs::write(
            modules.join(format!("module_{index}.zzm")),
            format!(
                "from perf/include_{index} import include_{index};\n\
                 function value_{index}(input) {{\n\
                 \treturn include_{index}() + input + {index};\n\
                 }}\n\
                 class PerfClass{index};\n"
            ),
        )
        .unwrap();
        fs::write(
            inc.join(format!("include_{index}.zzm")),
            format!(
                "function include_{index}() {{\n\
                 \treturn {index};\n\
                 }}\n"
            ),
        )
        .unwrap();
        fs::write(
            stdlib.join(format!("perf_{index}.zzm")),
            format!(
                "function std_value_{index}() {{\n\
                 \treturn {index};\n\
                 }}\n"
            ),
        )
        .unwrap();
        fs::write(
            scripts.join(format!("script_{index}.zzs")),
            format!(
                "from perf/module_{index} import value_{index};\n\
                 from std/perf_{index} import std_value_{index};\n\
                 function __main__() {{\n\
                 \tsay value_{index}(std_value_{index}());\n\
                 }}\n"
            ),
        )
        .unwrap();
        fs::write(
            tests.join(format!("case_{index}.zzs")),
            "say \"1..1\";\nsay \"ok 1 - generated performance fixture\";\n",
        )
        .unwrap();
    }

    let started = Instant::now();
    let analyzer = Analyzer::with_module_roots(vec![root.clone()], vec![runtime_modules]);
    let symbols = analyzer.workspace_symbols("value_119");
    let std_symbols = analyzer.workspace_symbols("std_value_119");
    let graph = analyzer.dependency_graph();
    let report = analyzer.package_report(Some(&root));
    let elapsed = started.elapsed();

    assert!(
        symbols.iter().any(|symbol| symbol.name == "value_119"),
        "generated distribution module symbol should be indexed"
    );
    assert!(
        std_symbols
            .iter()
            .any(|symbol| symbol.name == "std_value_119"),
        "generated stdlib module symbol should be indexed"
    );
    assert!(graph.edges.len() >= 240, "expected imports to be indexed");
    let expected_root = root.to_string_lossy().to_string();
    assert_eq!(report.root.as_deref(), Some(expected_root.as_str()));
    assert!(
        elapsed < Duration::from_secs(8),
        "indexing generated workspace took {elapsed:?}"
    );

    let _ = fs::remove_dir_all(root);
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}
