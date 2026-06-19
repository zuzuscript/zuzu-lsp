use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use url::Url;

#[test]
fn serves_basic_stdio_requests() {
    let tool_root = std::env::temp_dir().join(format!("zuzu-lsp-tool-root-{}", std::process::id()));
    std::fs::create_dir_all(&tool_root).unwrap();
    write_fake_command(
        &tool_root.join("zuzu-tidy.pl"),
        "printf 'function formatted() {\\n\\tsay 42;\\n}\\n'\n",
    );
    write_fake_command(
        &tool_root.join("zuzuprove"),
        "printf 'tested %s\\n' \"$1\"\n",
    );
    write_fake_command(
        &tool_root.join("pod_parse"),
        "printf 'Fixture arithmetic helpers\\nrendered %s %s %s\\n' \"$1\" \"$2\" \"$3\"\n",
    );
    write_fake_command(
        &tool_root.join("zuzubox"),
        "printf 'boxed %s %s\\n' \"$1\" \"$2\"\n",
    );
    write_fake_command(
        &tool_root.join("zuzu"),
        r#"if [ "$1" = "-V" ]; then
	printf 'zuzu-rust version test\nmodule search paths:\n'
	exit 0
fi
if [ "$1" = "--lint" ]; then
	shift
	if [ "$1" = "-e" ]; then
		case "$2" in
			*"let x := ;"*) printf 'parse error at 1:10: Expected expression\n' >&2; exit 1 ;;
			*) exit 0 ;;
		esac
	fi
fi
exit 0
"#,
    );
    let mut path_entries = vec![tool_root.clone()];
    if let Some(path) = std::env::var_os("PATH") {
        path_entries.extend(std::env::split_paths(&path));
    }
    let test_path = std::env::join_paths(path_entries).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_zuzu-lsp"))
        .arg("--stdio")
        .env("PATH", test_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zuzu-lsp");
    let mut stdin = child.stdin.take().expect("server stdin");
    let stdout = child.stdout.take().expect("server stdout");
    let mut reader = BufReader::new(stdout);

    let root = fixture_root();
    let canonical_root = root.canonicalize().unwrap();
    let extra_root =
        std::env::temp_dir().join(format!("zuzu-lsp-extra-root-{}", std::process::id()));
    let extra_module_dir = extra_root.join("modules").join("extra");
    std::fs::create_dir_all(&extra_module_dir).unwrap();
    std::fs::write(extra_module_dir.join("thing.zzm"), "class Thing;\n").unwrap();
    let configured_root =
        std::env::temp_dir().join(format!("zuzu-lsp-configured-root-{}", std::process::id()));
    let configured_module_dir = configured_root.join("configured");
    std::fs::create_dir_all(&configured_module_dir).unwrap();
    std::fs::write(
        configured_module_dir.join("module.zzm"),
        "class Configured;\n",
    )
    .unwrap();

    let script_path = root.join("scripts").join("demo.zzs");
    let uri = Url::from_file_path(&script_path).unwrap().to_string();
    let source = std::fs::read_to_string(&script_path).unwrap();

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": Url::from_file_path(&root).unwrap().to_string(),
                "capabilities": {},
                "initializationOptions": {
                    "zuzu": {
                        "moduleRoots": [configured_root.display().to_string()]
                    }
                }
            }
        }),
    );
    let initialize = read_response(&mut reader, 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "zuzu-lsp");
    assert_eq!(
        initialize["result"]["capabilities"]["textDocumentSync"]["change"],
        2
    );
    assert_eq!(
        initialize["result"]["capabilities"]["textDocumentSync"]["openClose"],
        true
    );
    assert!(initialize["result"]["capabilities"]["inlayHintProvider"].is_object());
    assert!(initialize["result"]["capabilities"]["codeLensProvider"].is_object());
    assert_eq!(
        initialize["result"]["capabilities"]["selectionRangeProvider"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["callHierarchyProvider"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["typeHierarchyProvider"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["workspace"]["workspaceFolders"]["supported"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["diagnosticProvider"]["identifier"],
        "zuzu"
    );
    assert_eq!(
        initialize["result"]["capabilities"]["diagnosticProvider"]["workspaceDiagnostics"],
        true
    );
    let semantic_token_types = initialize["result"]["capabilities"]["semanticTokensProvider"]
        ["legend"]["tokenTypes"]
        .as_array()
        .unwrap();
    assert!(semantic_token_types.iter().any(|token| token == "function"));
    assert!(semantic_token_types.iter().any(|token| token == "class"));
    let commands: Vec<_> = initialize["result"]["capabilities"]["executeCommandProvider"]
        ["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|command| command.as_str())
        .collect();
    assert!(commands.contains(&"zuzu.formatDocument"));
    assert!(commands.contains(&"zuzu.testFile"));
    assert!(commands.contains(&"zuzu.testWorkspace"));
    assert!(commands.contains(&"zuzu.renderDocs"));
    assert!(commands.contains(&"zuzu.verifyPackage"));
    assert!(commands.contains(&"zuzu.packageReport"));
    assert!(commands.contains(&"zuzu.dependencyGraph"));
    assert!(commands.contains(&"zuzu.replInstructions"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );

    let diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["params"]["version"], 1);
    assert_eq!(diagnostics["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeConfiguration",
            "params": {
                "settings": {}
            }
        }),
    );
    let refreshed_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(refreshed_diagnostics["params"]["uri"], uri);
    assert_eq!(refreshed_diagnostics["params"]["version"], 1);
    assert_eq!(refreshed_diagnostics["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 5 }
            }
        }),
    );
    let completion = read_response(&mut reader, 2);
    let labels: Vec<_> = completion["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"fn"));
    assert!(labels.contains(&"example/math"));
    assert!(labels.contains(&"configured/module"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWorkspaceFolders",
            "params": {
                "event": {
                    "added": [
                        {
                            "uri": Url::from_file_path(&extra_root).unwrap().to_string(),
                            "name": "extra"
                        }
                    ],
                    "removed": []
                }
            }
        }),
    );
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 5 }
            }
        }),
    );
    let completion = read_response(&mut reader, 21);
    let labels: Vec<_> = completion["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"extra/thing"));

    let later_module = extra_module_dir.join("later.zzm");
    std::fs::write(&later_module, "class Later;\n").unwrap();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [
                    {
                        "uri": Url::from_file_path(&later_module).unwrap().to_string(),
                        "type": 1
                    }
                ]
            }
        }),
    );
    let watched_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(watched_diagnostics["params"]["uri"], uri);
    assert_eq!(watched_diagnostics["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 5 }
            }
        }),
    );
    let completion = read_response(&mut reader, 42);
    let labels: Vec<_> = completion["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"extra/later"));

    std::fs::remove_file(&later_module).unwrap();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [
                    {
                        "uri": Url::from_file_path(&later_module).unwrap().to_string(),
                        "type": 3
                    }
                ]
            }
        }),
    );
    let watched_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(watched_diagnostics["params"]["uri"], uri);
    assert_eq!(watched_diagnostics["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 43,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 5 }
            }
        }),
    );
    let completion = read_response(&mut reader, 43);
    let labels: Vec<_> = completion["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(!labels.contains(&"extra/later"));
    assert!(labels.contains(&"extra/thing"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 8 }
            }
        }),
    );
    let hover = read_response(&mut reader, 11);
    assert_eq!(hover["result"]["contents"]["kind"], "markdown");
    assert!(hover["result"]["contents"]["value"]
        .as_str()
        .unwrap()
        .contains("Fixture arithmetic helpers"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 34,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 15 }
            }
        }),
    );
    let symbol_hover = read_response(&mut reader, 34);
    assert_eq!(symbol_hover["result"]["contents"]["kind"], "markdown");
    assert!(symbol_hover["result"]["contents"]["value"]
        .as_str()
        .unwrap()
        .contains("Fixture arithmetic helpers"));

    let incremental_uri = Url::from_file_path(root.join("scripts").join("incremental.zzs"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": incremental_uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": "function before() {\n\tsay 1;\n}\n"
                }
            }
        }),
    );
    let incremental_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(incremental_diagnostics["params"]["uri"], incremental_uri);
    assert_eq!(incremental_diagnostics["params"]["version"], 1);
    assert_eq!(incremental_diagnostics["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {
                    "uri": incremental_uri,
                    "version": 2
                },
                "contentChanges": [
                    {
                        "range": {
                            "start": { "line": 0, "character": 9 },
                            "end": { "line": 0, "character": 15 }
                        },
                        "text": "after"
                    }
                ]
            }
        }),
    );
    let incremental_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(incremental_diagnostics["params"]["uri"], incremental_uri);
    assert_eq!(incremental_diagnostics["params"]["version"], 2);
    assert_eq!(incremental_diagnostics["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 35,
            "method": "textDocument/documentSymbol",
            "params": {
                "textDocument": { "uri": incremental_uri }
            }
        }),
    );
    let incremental_symbols = read_response(&mut reader, 35);
    assert_eq!(incremental_symbols["result"][0]["name"], "after");

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 36,
            "method": "textDocument/formatting",
            "params": {
                "textDocument": { "uri": incremental_uri },
                "options": {
                    "tabSize": 4,
                    "insertSpaces": false
                }
            }
        }),
    );
    let formatting = read_response(&mut reader, 36);
    assert_eq!(
        formatting["result"][0]["newText"],
        "function formatted() {\n\tsay 42;\n}\n"
    );

    let sig_uri = Url::from_file_path(root.join("scripts").join("signature.zzs"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": sig_uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": "function add(a, b) {\n\treturn a + b;\n}\nfunction main() {\n\tadd(1, 2);\n}\n"
                }
            }
        }),
    );
    let sig_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(sig_diagnostics["params"]["uri"], sig_uri);
    assert_eq!(sig_diagnostics["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 41,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": sig_uri },
                "position": { "line": 1, "character": 10 }
            }
        }),
    );
    let operator_hover = read_response(&mut reader, 41);
    assert!(operator_hover["result"]["contents"]
        .as_str()
        .unwrap()
        .contains("addition"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "textDocument/signatureHelp",
            "params": {
                "textDocument": { "uri": sig_uri },
                "position": { "line": 4, "character": 9 }
            }
        }),
    );
    let signature = read_response(&mut reader, 12);
    assert_eq!(signature["result"]["signatures"][0]["label"], "add(a, b)");
    assert_eq!(signature["result"]["activeParameter"], 1);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "textDocument/inlayHint",
            "params": {
                "textDocument": { "uri": sig_uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 6, "character": 0 }
                }
            }
        }),
    );
    let inlay_hints = read_response(&mut reader, 13);
    assert_eq!(inlay_hints["result"][0]["label"], "a:");
    assert_eq!(inlay_hints["result"][1]["label"], "b:");
    assert_eq!(inlay_hints["result"][0]["kind"], 2);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "textDocument/selectionRange",
            "params": {
                "textDocument": { "uri": sig_uri },
                "positions": [
                    { "line": 4, "character": 2 }
                ]
            }
        }),
    );
    let selection_ranges = read_response(&mut reader, 20);
    assert_eq!(selection_ranges["result"][0]["range"]["start"]["line"], 4);
    assert_eq!(
        selection_ranges["result"][0]["range"]["start"]["character"],
        1
    );
    assert_eq!(
        selection_ranges["result"][0]["range"]["end"]["character"],
        4
    );
    assert!(selection_ranges["result"][0]["parent"].is_object());

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 24,
            "method": "textDocument/prepareCallHierarchy",
            "params": {
                "textDocument": { "uri": sig_uri },
                "position": { "line": 3, "character": 10 }
            }
        }),
    );
    let prepared_main = read_response(&mut reader, 24);
    assert_eq!(prepared_main["result"][0]["name"], "main");
    let main_item = prepared_main["result"][0].clone();

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 25,
            "method": "callHierarchy/outgoingCalls",
            "params": {
                "item": main_item
            }
        }),
    );
    let outgoing = read_response(&mut reader, 25);
    assert_eq!(outgoing["result"][0]["to"]["name"], "add");
    assert_eq!(outgoing["result"][0]["fromRanges"][0]["start"]["line"], 4);
    assert_eq!(
        outgoing["result"][0]["fromRanges"][0]["start"]["character"],
        1
    );
    assert_eq!(
        outgoing["result"][0]["fromRanges"][0]["end"]["character"],
        4
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 26,
            "method": "textDocument/prepareCallHierarchy",
            "params": {
                "textDocument": { "uri": sig_uri },
                "position": { "line": 0, "character": 10 }
            }
        }),
    );
    let prepared_add = read_response(&mut reader, 26);
    assert_eq!(prepared_add["result"][0]["name"], "add");
    let add_item = prepared_add["result"][0].clone();

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 27,
            "method": "callHierarchy/incomingCalls",
            "params": {
                "item": add_item
            }
        }),
    );
    let incoming = read_response(&mut reader, 27);
    let incoming_names: Vec<_> = incoming["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|call| call["from"]["name"].as_str())
        .collect();
    assert!(incoming_names.contains(&"__main__"));
    assert!(incoming_names.contains(&"main"));

    let types_uri = Url::from_file_path(root.join("modules").join("example").join("types.zzm"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": types_uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": "trait Printable {}\nclass Base;\nclass Derived extends Base but Printable;\n"
                }
            }
        }),
    );
    let types_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(types_diagnostics["params"]["uri"], types_uri);
    assert_eq!(types_diagnostics["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 28,
            "method": "textDocument/prepareTypeHierarchy",
            "params": {
                "textDocument": { "uri": types_uri },
                "position": { "line": 2, "character": 8 }
            }
        }),
    );
    let prepared_derived = read_response(&mut reader, 28);
    assert_eq!(prepared_derived["result"][0]["name"], "Derived");
    let derived_item = prepared_derived["result"][0].clone();

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 29,
            "method": "typeHierarchy/supertypes",
            "params": {
                "item": derived_item
            }
        }),
    );
    let supertypes = read_response(&mut reader, 29);
    let supertype_names: Vec<_> = supertypes["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    assert!(supertype_names.contains(&"Base"));
    assert!(supertype_names.contains(&"Printable"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "textDocument/prepareTypeHierarchy",
            "params": {
                "textDocument": { "uri": types_uri },
                "position": { "line": 1, "character": 8 }
            }
        }),
    );
    let prepared_base = read_response(&mut reader, 30);
    assert_eq!(prepared_base["result"][0]["name"], "Base");
    let base_item = prepared_base["result"][0].clone();

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 31,
            "method": "typeHierarchy/subtypes",
            "params": {
                "item": base_item
            }
        }),
    );
    let subtypes = read_response(&mut reader, 31);
    assert_eq!(subtypes["result"][0]["name"], "Derived");

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/documentSymbol",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let symbols = read_response(&mut reader, 3);
    let names: Vec<_> = symbols["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    assert!(names.contains(&"__main__"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 23,
            "method": "textDocument/semanticTokens/full",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let semantic_tokens = read_response(&mut reader, 23);
    let token_data = semantic_tokens["result"]["data"].as_array().unwrap();
    assert!(!token_data.is_empty());
    assert_eq!(token_data.len() % 5, 0);
    assert!(token_data.chunks(5).any(|token| token[3] == 3));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "textDocument/documentLink",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let links = read_response(&mut reader, 4);
    assert_eq!(links["result"].as_array().unwrap().len(), 1);
    assert!(links["result"][0]["target"]
        .as_str()
        .unwrap()
        .ends_with("/modules/example/math.zzm"));

    let metadata_uri = Url::from_file_path(root.join("zuzu-distribution.json"))
        .unwrap()
        .to_string();
    let metadata_source = std::fs::read_to_string(root.join("zuzu-distribution.json"))
        .unwrap()
        .replace(
            "\"version\": \"0.0.1\",",
            "\"version\": \"0.0.1\",\n\t\"repo\": \"https://github.com/zuzuscript/zuzu-lsp-fixture\",",
        );
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": metadata_uri,
                    "languageId": "json",
                    "version": 1,
                    "text": metadata_source
                }
            }
        }),
    );
    let metadata_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(metadata_diagnostics["params"]["uri"], metadata_uri);
    assert_eq!(metadata_diagnostics["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [
                    {
                        "uri": metadata_uri,
                        "type": 2
                    }
                ]
            }
        }),
    );
    let mut refreshed_metadata_diagnostics = None;
    let mut saw_zuzu_diagnostics = false;
    for _ in 0..8 {
        let message = read_method(&mut reader, "textDocument/publishDiagnostics");
        if message["params"]["uri"] == uri {
            saw_zuzu_diagnostics = true;
        }
        if message["params"]["uri"] == metadata_uri {
            refreshed_metadata_diagnostics = Some(message);
        }
        if refreshed_metadata_diagnostics.is_some() && saw_zuzu_diagnostics {
            break;
        }
    }
    assert!(
        saw_zuzu_diagnostics,
        "zuzu diagnostics after workspace refresh"
    );
    assert_eq!(
        refreshed_metadata_diagnostics.expect("metadata diagnostics after workspace refresh")
            ["params"]["diagnostics"],
        json!([])
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "textDocument/documentLink",
            "params": {
                "textDocument": { "uri": metadata_uri }
            }
        }),
    );
    let metadata_links = read_response(&mut reader, 15);
    assert_eq!(
        metadata_links["result"][0]["tooltip"],
        "Open dependency module `example/math`"
    );
    assert!(metadata_links["result"][0]["target"]
        .as_str()
        .unwrap()
        .ends_with("/modules/example/math.zzm"));
    let repo_link = metadata_links["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["tooltip"] == "Open package metadata `repo` URL")
        .expect("metadata repo document link");
    assert_eq!(
        repo_link["target"],
        "https://github.com/zuzuscript/zuzu-lsp-fixture"
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 16,
            "method": "textDocument/codeLens",
            "params": {
                "textDocument": { "uri": metadata_uri }
            }
        }),
    );
    let metadata_lenses = read_response(&mut reader, 16);
    assert_eq!(
        metadata_lenses["result"][0]["command"]["command"],
        "zuzu.testWorkspace"
    );
    assert_eq!(
        metadata_lenses["result"][1]["command"]["command"],
        "zuzu.verifyPackage"
    );
    assert_eq!(
        metadata_lenses["result"][2]["command"]["command"],
        "zuzu.packageReport"
    );

    let test_uri = Url::from_file_path(root.join("tests").join("example.zzs"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 17,
            "method": "textDocument/codeLens",
            "params": {
                "textDocument": { "uri": test_uri }
            }
        }),
    );
    let test_lenses = read_response(&mut reader, 17);
    assert_eq!(
        test_lenses["result"][0]["command"]["command"],
        "zuzu.testFile"
    );
    assert_eq!(
        test_lenses["result"][0]["command"]["arguments"][0],
        test_uri
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 37,
            "method": "workspace/executeCommand",
            "params": {
                "command": "zuzu.testFile",
                "arguments": [test_uri]
            }
        }),
    );
    let command_log = read_method(&mut reader, "window/logMessage");
    assert_eq!(command_log["params"]["type"], 4);
    assert!(command_log["params"]["message"]
        .as_str()
        .unwrap()
        .contains("zuzuprove"));
    assert!(command_log["params"]["message"]
        .as_str()
        .unwrap()
        .contains("example.zzs"));
    let test_output = read_response(&mut reader, 37);
    assert!(test_output["result"]["success"].as_bool().unwrap());
    assert!(test_output["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains("tested "));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 38,
            "method": "workspace/executeCommand",
            "params": {
                "command": "zuzu.testWorkspace",
                "arguments": [metadata_uri]
            }
        }),
    );
    let workspace_test_log = read_method(&mut reader, "window/logMessage");
    assert!(workspace_test_log["params"]["message"]
        .as_str()
        .unwrap()
        .contains("zuzuprove"));
    let workspace_test_output = read_response(&mut reader, 38);
    assert!(workspace_test_output["result"]["success"]
        .as_bool()
        .unwrap());
    assert!(workspace_test_output["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains("tested "));
    assert!(workspace_test_output["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains(canonical_root.to_string_lossy().as_ref()));
    assert!(!workspace_test_output["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains("zuzu-distribution.json"));

    let module_doc_uri = Url::from_file_path(root.join("modules").join("example").join("math.zzm"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 45,
            "method": "textDocument/documentLink",
            "params": {
                "textDocument": { "uri": module_doc_uri }
            }
        }),
    );
    let pod_links = read_response(&mut reader, 45);
    assert_eq!(
        pod_links["result"][0]["tooltip"],
        "Open POD module link `example/math`"
    );
    assert!(pod_links["result"][0]["target"]
        .as_str()
        .unwrap()
        .ends_with("/modules/example/math.zzm"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 39,
            "method": "workspace/executeCommand",
            "params": {
                "command": "zuzu.renderDocs",
                "arguments": [module_doc_uri]
            }
        }),
    );
    let docs_log = read_method(&mut reader, "window/logMessage");
    assert!(docs_log["params"]["message"]
        .as_str()
        .unwrap()
        .contains("pod_parse"));
    let docs_output = read_response(&mut reader, 39);
    assert!(docs_output["result"]["success"].as_bool().unwrap());
    assert!(docs_output["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains("rendered -f markdown"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 40,
            "method": "workspace/executeCommand",
            "params": {
                "command": "zuzu.verifyPackage",
                "arguments": [metadata_uri]
            }
        }),
    );
    let verify_log = read_method(&mut reader, "window/logMessage");
    assert!(verify_log["params"]["message"]
        .as_str()
        .unwrap()
        .contains("zuzubox"));
    let verify_output = read_response(&mut reader, 40);
    assert!(verify_output["result"]["success"].as_bool().unwrap());
    assert!(verify_output["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains("boxed verify "));
    assert!(verify_output["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains(canonical_root.to_string_lossy().as_ref()));
    assert!(!verify_output["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains("zuzu-distribution.json"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 18,
            "method": "workspace/executeCommand",
            "params": {
                "command": "zuzu.packageReport",
                "arguments": [metadata_uri]
            }
        }),
    );
    let package_report = read_response(&mut reader, 18);
    assert!(package_report["result"]["root"]
        .as_str()
        .unwrap()
        .ends_with("/fixtures/workspaces/basic"));
    assert!(package_report["result"]["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .any(|dependency| dependency == "example/math"));
    assert!(package_report["result"]["moduleRoots"]
        .as_array()
        .unwrap()
        .iter()
        .any(|root| root.as_str().unwrap().ends_with("/modules")));
    assert_eq!(package_report["result"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 33,
            "method": "workspace/executeCommand",
            "params": {
                "command": "zuzu.dependencyGraph",
                "arguments": []
            }
        }),
    );
    let dependency_graph = read_response(&mut reader, 33);
    assert!(dependency_graph["result"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["id"] == "example/math" && node["kind"] == "module"));
    assert!(dependency_graph["result"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["to"] == "example/math"
            && edge["resolved"] == true
            && edge["from"]
                .as_str()
                .unwrap()
                .ends_with("/scripts/demo.zzs")));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 19,
            "method": "workspace/executeCommand",
            "params": {
                "command": "zuzu.replInstructions",
                "arguments": []
            }
        }),
    );
    let repl = read_response(&mut reader, 19);
    assert_eq!(repl["result"]["command"][1], "-R");
    assert!(repl["result"]["message"]
        .as_str()
        .unwrap()
        .contains("terminal"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 7 },
                "context": { "includeDeclaration": true }
            }
        }),
    );
    let references = read_response(&mut reader, 5);
    assert_eq!(references["result"].as_array().unwrap().len(), 2);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "textDocument/prepareRename",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 7 }
            }
        }),
    );
    let prepare_rename = read_response(&mut reader, 6);
    assert_eq!(prepare_rename["result"]["start"]["line"], 3);
    assert_eq!(prepare_rename["result"]["start"]["character"], 5);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "textDocument/rename",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 7 },
                "newName": "sum"
            }
        }),
    );
    let rename = read_response(&mut reader, 7);
    assert_eq!(
        rename["result"]["documentChanges"][0]["textDocument"]["uri"],
        uri
    );
    let edits = rename["result"]["documentChanges"][0]["edits"]
        .as_array()
        .unwrap();
    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit["newText"] == "sum"));

    let bad_uri = Url::from_file_path(root.join("scripts").join("bad.zzs"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": bad_uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": "from missing/module import Thing;\nsay 1;\n"
                }
            }
        }),
    );
    let bad_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(bad_diagnostics["params"]["uri"], bad_uri);
    assert_eq!(bad_diagnostics["params"]["version"], 1);
    assert_eq!(
        bad_diagnostics["params"]["diagnostics"][0]["code"],
        "unresolved-import"
    );

    let parser_bad_uri = Url::from_file_path(root.join("scripts").join("parser-bad.zzs"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": parser_bad_uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": "let x := ;\n"
                }
            }
        }),
    );
    let parser_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(parser_diagnostics["params"]["uri"], parser_bad_uri);
    let parser_diagnostic = parser_diagnostics["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["source"].as_str() == Some("zuzu-parser"))
        .expect("parser diagnostic");
    assert_eq!(parser_diagnostic["code"], "parse-error");
    assert_eq!(parser_diagnostic["range"]["start"]["line"], 0);
    assert_eq!(parser_diagnostic["range"]["start"]["character"], 9);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "textDocument/diagnostic",
            "params": {
                "textDocument": { "uri": bad_uri },
                "identifier": "zuzu",
                "previousResultId": null
            }
        }),
    );
    let pulled_diagnostics = read_response(&mut reader, 22);
    assert_eq!(pulled_diagnostics["result"]["kind"], "full");
    assert_eq!(
        pulled_diagnostics["result"]["items"][0]["code"],
        "unresolved-import"
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 44,
            "method": "workspace/diagnostic",
            "params": {
                "identifier": "zuzu",
                "previousResultIds": [],
                "workDoneToken": "workspace-diagnostics"
            }
        }),
    );
    let progress_begin = read_method(&mut reader, "$/progress");
    assert_eq!(progress_begin["params"]["token"], "workspace-diagnostics");
    assert_eq!(progress_begin["params"]["value"]["kind"], "begin");
    assert_eq!(
        progress_begin["params"]["value"]["title"],
        "ZuzuScript workspace diagnostics"
    );
    assert_eq!(progress_begin["params"]["value"]["percentage"], 0);
    let progress_report = read_method(&mut reader, "$/progress");
    assert_eq!(progress_report["params"]["token"], "workspace-diagnostics");
    assert_eq!(progress_report["params"]["value"]["kind"], "report");
    assert_eq!(progress_report["params"]["value"]["percentage"], 50);
    let progress_ready = read_method(&mut reader, "$/progress");
    assert_eq!(progress_ready["params"]["token"], "workspace-diagnostics");
    assert_eq!(progress_ready["params"]["value"]["kind"], "report");
    assert_eq!(progress_ready["params"]["value"]["percentage"], 100);
    let progress_end = read_method(&mut reader, "$/progress");
    assert_eq!(progress_end["params"]["token"], "workspace-diagnostics");
    assert_eq!(progress_end["params"]["value"]["kind"], "end");
    let workspace_diagnostics = read_response(&mut reader, 44);
    let workspace_items = workspace_diagnostics["result"]["items"].as_array().unwrap();
    let bad_report = workspace_items
        .iter()
        .find(|item| item["uri"].as_str() == Some(bad_uri.as_str()))
        .expect("bad document diagnostics in workspace report");
    assert_eq!(bad_report["kind"], "full");
    assert_eq!(bad_report["version"], 1);
    assert_eq!(bad_report["items"][0]["code"], "unresolved-import");

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": bad_uri },
                "range": {
                    "start": { "line": 0, "character": 5 },
                    "end": { "line": 0, "character": 5 }
                },
                "context": { "diagnostics": [] }
            }
        }),
    );
    let code_actions = read_response(&mut reader, 8);
    assert_eq!(
        code_actions["result"][0]["title"],
        "Remove unresolved import `missing/module`"
    );
    assert_eq!(
        code_actions["result"][1]["title"],
        "Create module `missing/module`"
    );
    assert_eq!(
        code_actions["result"][1]["edit"]["documentChanges"][0]["kind"],
        "create"
    );
    assert!(
        code_actions["result"][1]["edit"]["documentChanges"][0]["uri"]
            .as_str()
            .unwrap()
            .ends_with("/modules/missing/module.zzm")
    );

    let unused_uri = Url::from_file_path(root.join("scripts").join("unused.zzs"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": unused_uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": "from example/math import Calculator;\nsay 1;\n"
                }
            }
        }),
    );
    let unused_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(unused_diagnostics["params"]["uri"], unused_uri);
    assert_eq!(
        unused_diagnostics["params"]["diagnostics"][0]["code"],
        "unused-import"
    );
    assert_eq!(
        unused_diagnostics["params"]["diagnostics"][0]["severity"],
        2
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": unused_uri },
                "range": {
                    "start": { "line": 0, "character": 5 },
                    "end": { "line": 0, "character": 5 }
                },
                "context": { "diagnostics": [] }
            }
        }),
    );
    let unused_actions = read_response(&mut reader, 14);
    assert_eq!(
        unused_actions["result"][0]["title"],
        "Remove unused import `example/math`"
    );

    let try_import_uri = Url::from_file_path(root.join("scripts").join("try-import.zzs"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": try_import_uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": "from example/math try import Calculator;\nsay Calculator;\n"
                }
            }
        }),
    );
    let try_import_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(try_import_diagnostics["params"]["uri"], try_import_uri);
    assert_eq!(
        try_import_diagnostics["params"]["diagnostics"][0]["code"],
        "suspicious-try-import"
    );

    let undefined_uri = Url::from_file_path(root.join("scripts").join("undefined.zzs"))
        .unwrap()
        .to_string();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": undefined_uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": "function main() {\n\tlet total := Calculator;\n\tsay total;\n}\n"
                }
            }
        }),
    );
    let undefined_diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(undefined_diagnostics["params"]["uri"], undefined_uri);
    assert_eq!(
        undefined_diagnostics["params"]["diagnostics"][0]["code"],
        "undefined-local"
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 32,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": undefined_uri },
                "range": {
                    "start": { "line": 1, "character": 15 },
                    "end": { "line": 1, "character": 15 }
                },
                "context": { "diagnostics": [] }
            }
        }),
    );
    let missing_import_actions = read_response(&mut reader, 32);
    assert_eq!(
        missing_import_actions["result"][0]["title"],
        "Import `Calculator` from `example/math`"
    );
    assert_eq!(
        missing_import_actions["result"][0]["edit"]["documentChanges"][0]["textDocument"]["uri"],
        undefined_uri
    );
    assert_eq!(
        missing_import_actions["result"][0]["edit"]["documentChanges"][0]["edits"][0]["newText"],
        "from example/math import Calculator;\n"
    );
    assert!(missing_import_actions["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["title"] == "Run Zuzu formatter"
            && action["kind"] == "source"
            && action["command"]["command"] == "zuzu.formatDocument"
            && action["command"]["arguments"][0] == undefined_uri));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "textDocument/documentHighlight",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 7 }
            }
        }),
    );
    let highlights = read_response(&mut reader, 9);
    assert_eq!(highlights["result"].as_array().unwrap().len(), 2);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "workspace/executeCommand",
            "params": {
                "command": "zuzu.doctor",
                "arguments": []
            }
        }),
    );
    let doctor = read_response(&mut reader, 10);
    assert!(doctor["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line.as_str().unwrap().starts_with("zuzu-tidy.pl: ")));
    assert!(doctor["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line.as_str().unwrap().starts_with("zuzu version: ")));

    let _ = std::fs::remove_file(extra_module_dir.join("later.zzm"));
    let _ = std::fs::remove_file(extra_module_dir.join("thing.zzm"));
    let _ = std::fs::remove_dir(extra_module_dir);
    let _ = std::fs::remove_dir(extra_root.join("modules"));
    let _ = std::fs::remove_dir(extra_root);
    let _ = std::fs::remove_file(configured_module_dir.join("module.zzm"));
    let _ = std::fs::remove_dir(configured_module_dir);
    let _ = std::fs::remove_dir(configured_root);
    let _ = std::fs::remove_file(tool_root.join("zuzu-tidy.pl"));
    let _ = std::fs::remove_file(tool_root.join("zuzuprove"));
    let _ = std::fs::remove_file(tool_root.join("pod_parse"));
    let _ = std::fs::remove_file(tool_root.join("zuzubox"));
    let _ = std::fs::remove_file(tool_root.join("zuzu"));
    let _ = std::fs::remove_dir(tool_root);

    shutdown(&mut child, stdin, &mut reader);
}

#[test]
fn refuses_tool_execution_in_untrusted_workspace() {
    let tool_root = std::env::temp_dir().join(format!(
        "zuzu-lsp-untrusted-tool-root-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tool_root).unwrap();
    let pod_parse_marker = tool_root.join("pod-parse-ran");
    let tidy_marker = tool_root.join("tidy-ran");
    let prove_marker = tool_root.join("prove-ran");
    let box_marker = tool_root.join("box-ran");
    write_fake_command(
        &tool_root.join("pod_parse"),
        &format!(
            "printf ran > '{}'\nprintf 'untrusted docs\\n'\n",
            pod_parse_marker.display()
        ),
    );
    write_fake_command(
        &tool_root.join("zuzu-tidy.pl"),
        &format!("printf ran > '{}'\ncat\n", tidy_marker.display()),
    );
    write_fake_command(
        &tool_root.join("zuzuprove"),
        &format!(
            "printf ran > '{}'\nprintf 'tested %s\\n' \"$1\"\n",
            prove_marker.display()
        ),
    );
    write_fake_command(
        &tool_root.join("zuzubox"),
        &format!(
            "printf ran > '{}'\nprintf 'boxed %s %s\\n' \"$1\" \"$2\"\n",
            box_marker.display()
        ),
    );
    let test_path = std::env::join_paths([tool_root.clone()]).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_zuzu-lsp"))
        .arg("--stdio")
        .env("PATH", test_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zuzu-lsp");
    let mut stdin = child.stdin.take().expect("server stdin");
    let stdout = child.stdout.take().expect("server stdout");
    let mut reader = BufReader::new(stdout);

    let root = fixture_root();
    let script_path = root.join("scripts").join("demo.zzs");
    let uri = Url::from_file_path(&script_path).unwrap().to_string();
    let source = std::fs::read_to_string(&script_path).unwrap();

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": Url::from_file_path(&root).unwrap().to_string(),
                "capabilities": {
                    "workspace": {
                        "workspaceTrust": {
                            "trusted": false
                        }
                    }
                }
            }
        }),
    );
    let initialize = read_response(&mut reader, 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "zuzu-lsp");

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["params"]["diagnostics"], json!([]));
    assert!(!pod_parse_marker.exists());

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/documentSymbol",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let symbols = read_response(&mut reader, 2);
    assert!(symbols["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["name"] == "__main__"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/formatting",
            "params": {
                "textDocument": { "uri": uri },
                "options": {
                    "tabSize": 4,
                    "insertSpaces": false
                }
            }
        }),
    );
    let formatting = read_response(&mut reader, 3);
    assert!(formatting["error"]["message"]
        .as_str()
        .unwrap()
        .contains("untrusted"));
    assert!(!pod_parse_marker.exists());
    assert!(!tidy_marker.exists());
    assert!(!prove_marker.exists());
    assert!(!box_marker.exists());

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 8 }
            }
        }),
    );
    let hover = read_response(&mut reader, 5);
    assert!(hover["result"]["contents"]
        .as_str()
        .unwrap()
        .contains("ZuzuScript import"));
    assert!(!pod_parse_marker.exists());
    assert!(!tidy_marker.exists());
    assert!(!prove_marker.exists());
    assert!(!box_marker.exists());

    for (id, command, arguments) in [
        (4, "zuzu.formatDocument", json!([uri])),
        (6, "zuzu.testFile", json!([uri])),
        (
            7,
            "zuzu.testWorkspace",
            json!([Url::from_file_path(root.join("zuzu-distribution.json"))
                .unwrap()
                .to_string()]),
        ),
        (
            8,
            "zuzu.renderDocs",
            json!([
                Url::from_file_path(root.join("modules").join("example").join("math.zzm"))
                    .unwrap()
                    .to_string()
            ]),
        ),
        (
            9,
            "zuzu.verifyPackage",
            json!([Url::from_file_path(root.join("zuzu-distribution.json"))
                .unwrap()
                .to_string()]),
        ),
    ] {
        send(
            &mut stdin,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "workspace/executeCommand",
                "params": {
                    "command": command,
                    "arguments": arguments
                }
            }),
        );
        let command_response = read_response(&mut reader, id);
        assert!(
            command_response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("untrusted"),
            "{command} should be refused in untrusted workspaces: {command_response}"
        );
        assert!(!pod_parse_marker.exists());
        assert!(!tidy_marker.exists());
        assert!(!prove_marker.exists());
        assert!(!box_marker.exists());
    }

    shutdown(&mut child, stdin, &mut reader);
    let _ = std::fs::remove_dir_all(tool_root);
}

#[test]
fn can_disable_runtime_parser_diagnostics_with_settings() {
    let tool_root = std::env::temp_dir().join(format!(
        "zuzu-lsp-parser-settings-tool-root-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tool_root).unwrap();
    write_fake_command(
        &tool_root.join("zuzu"),
        r#"if [ "$1" = "-V" ]; then
	printf 'zuzu-rust version test\nmodule search paths:\n'
	exit 0
fi
if [ "$1" = "--lint" ]; then
	printf 'parse error at 1:10: Expected expression\n' >&2
	exit 1
fi
exit 0
"#,
    );
    let mut path_entries = vec![tool_root.clone()];
    if let Some(path) = std::env::var_os("PATH") {
        path_entries.extend(std::env::split_paths(&path));
    }
    let test_path = std::env::join_paths(path_entries).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_zuzu-lsp"))
        .arg("--stdio")
        .env("PATH", test_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zuzu-lsp");
    let mut stdin = child.stdin.take().expect("server stdin");
    let stdout = child.stdout.take().expect("server stdout");
    let mut reader = BufReader::new(stdout);

    let root = fixture_root();
    let parser_bad_uri = Url::from_file_path(root.join("scripts").join("parser-disabled.zzs"))
        .unwrap()
        .to_string();

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": Url::from_file_path(&root).unwrap().to_string(),
                "capabilities": {},
                "initializationOptions": {
                    "zuzu": {
                        "runtimeParserDiagnostics": false
                    }
                }
            }
        }),
    );
    let initialize = read_response(&mut reader, 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "zuzu-lsp");

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": parser_bad_uri,
                    "languageId": "zuzu",
                    "version": 1,
                    "text": "let x := ;\n"
                }
            }
        }),
    );
    let diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["params"]["uri"], parser_bad_uri);
    assert!(!diagnostics["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["source"].as_str() == Some("zuzu-parser")));

    let _ = std::fs::remove_file(tool_root.join("zuzu"));
    let _ = std::fs::remove_dir(tool_root);

    shutdown(&mut child, stdin, &mut reader);
}

#[test]
fn publishes_distribution_metadata_diagnostics() {
    let root =
        std::env::temp_dir().join(format!("zuzu-lsp-bad-metadata-root-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let metadata_path = root.join("zuzu-distribution.json");
    let valid_metadata = "{\n\t\"name\": \"live-metadata\",\n\t\"version\": \"0.0.1\",\n\t\"author\": \"Example Author\",\n\t\"license\": \"MIT\",\n\t\"abstract\": \"Live metadata fixture.\",\n\t\"dependencies\": {}\n}\n";
    std::fs::write(&metadata_path, valid_metadata).unwrap();
    let metadata_uri = Url::from_file_path(&metadata_path).unwrap().to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_zuzu-lsp"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zuzu-lsp");
    let mut stdin = child.stdin.take().expect("server stdin");
    let stdout = child.stdout.take().expect("server stdout");
    let mut reader = BufReader::new(stdout);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": Url::from_file_path(&root).unwrap().to_string(),
                "capabilities": {}
            }
        }),
    );
    let initialize = read_response(&mut reader, 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "zuzu-lsp");

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": metadata_uri,
                    "languageId": "json",
                    "version": 1,
                    "text": "{ not json\n"
                }
            }
        }),
    );
    let diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["params"]["uri"], metadata_uri);
    assert_eq!(
        diagnostics["params"]["diagnostics"][0]["code"],
        "metadata-invalid-json"
    );
    assert_eq!(
        diagnostics["params"]["diagnostics"][0]["source"],
        "zuzu-package"
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {
                    "uri": metadata_uri,
                    "version": 2
                },
                "contentChanges": [
                    {
                        "text": valid_metadata
                    }
                ]
            }
        }),
    );
    let diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["params"]["uri"], metadata_uri);
    assert_eq!(diagnostics["params"]["diagnostics"], json!([]));

    let _ = std::fs::remove_file(metadata_path);
    let _ = std::fs::remove_dir(root);

    shutdown(&mut child, stdin, &mut reader);
}

#[test]
fn publishes_distribution_metadata_toolchain_diagnostics() {
    let root = std::env::temp_dir().join(format!(
        "zuzu-lsp-metadata-toolchain-root-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let home = std::env::temp_dir().join(format!(
        "zuzu-lsp-metadata-toolchain-home-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let metadata_path = root.join("zuzu-distribution.json");
    let metadata_text = "{\n\t\"name\": \"missing-tools\",\n\t\"version\": \"0.0.1\",\n\t\"author\": \"Example Author\",\n\t\"license\": \"MIT\",\n\t\"abstract\": \"Missing package tool fixture.\",\n\t\"dependencies\": {}\n}\n";
    std::fs::write(&metadata_path, metadata_text).unwrap();
    let metadata_uri = Url::from_file_path(&metadata_path).unwrap().to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_zuzu-lsp"))
        .arg("--stdio")
        .env("PATH", "")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zuzu-lsp");
    let mut stdin = child.stdin.take().expect("server stdin");
    let stdout = child.stdout.take().expect("server stdout");
    let mut reader = BufReader::new(stdout);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": Url::from_file_path(&root).unwrap().to_string(),
                "capabilities": {}
            }
        }),
    );
    let initialize = read_response(&mut reader, 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "zuzu-lsp");

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": metadata_uri,
                    "languageId": "json",
                    "version": 1,
                    "text": metadata_text
                }
            }
        }),
    );
    let diagnostics = read_method(&mut reader, "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["params"]["uri"], metadata_uri);
    assert!(diagnostics["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |diagnostic| diagnostic["code"] == "missing-package-verifier"
                && diagnostic["source"] == "zuzu-toolchain"
        ));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/diagnostic",
            "params": {
                "textDocument": { "uri": metadata_uri },
                "identifier": "zuzu",
                "previousResultId": null
            }
        }),
    );
    let pulled_diagnostics = read_response(&mut reader, 2);
    assert!(pulled_diagnostics["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |diagnostic| diagnostic["code"] == "missing-package-verifier"
                && diagnostic["source"] == "zuzu-toolchain"
        ));

    let _ = std::fs::remove_file(metadata_path);
    let _ = std::fs::remove_dir(root);
    let _ = std::fs::remove_dir(home);

    shutdown(&mut child, stdin, &mut reader);
}

#[test]
fn cancels_workspace_diagnostics() {
    let root = fixture_root();
    let mut child = Command::new(env!("CARGO_BIN_EXE_zuzu-lsp"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zuzu-lsp");
    let mut stdin = child.stdin.take().expect("server stdin");
    let stdout = child.stdout.take().expect("server stdout");
    let mut reader = BufReader::new(stdout);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": Url::from_file_path(root).unwrap().to_string(),
                "capabilities": {}
            }
        }),
    );
    let initialize = read_response(&mut reader, 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "zuzu-lsp");

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "$/cancelRequest",
            "params": {
                "id": 2
            }
        }),
    );
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "workspace/diagnostic",
            "params": {
                "identifier": "zuzu",
                "previousResultIds": [],
                "workDoneToken": "cancelled-workspace-diagnostics"
            }
        }),
    );

    let progress_begin = read_method(&mut reader, "$/progress");
    assert_eq!(
        progress_begin["params"]["token"],
        "cancelled-workspace-diagnostics"
    );
    assert_eq!(progress_begin["params"]["value"]["kind"], "begin");
    assert_eq!(progress_begin["params"]["value"]["cancellable"], true);
    assert_eq!(progress_begin["params"]["value"]["percentage"], 0);

    let progress_end = read_method(&mut reader, "$/progress");
    assert_eq!(
        progress_end["params"]["token"],
        "cancelled-workspace-diagnostics"
    );
    assert_eq!(progress_end["params"]["value"]["kind"], "end");
    assert_eq!(
        progress_end["params"]["value"]["message"],
        "Workspace diagnostics cancelled"
    );

    let response = read_response(&mut reader, 2);
    assert_eq!(response["error"]["code"], -32800);
    assert_eq!(
        response["error"]["message"],
        "Workspace diagnostics cancelled"
    );

    shutdown(&mut child, stdin, &mut reader);
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("workspaces")
        .join("basic")
}

fn send(stdin: &mut ChildStdin, message: Value) {
    let body = serde_json::to_string(&message).unwrap();
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
    stdin.flush().unwrap();
}

fn read_response(reader: &mut BufReader<ChildStdout>, id: i64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for response id {id}"
        );
        let message = read_message(reader);
        if message["id"].as_i64() == Some(id) {
            return message;
        }
    }
}

fn read_method(reader: &mut BufReader<ChildStdout>, method: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for method {method}"
        );
        let message = read_message(reader);
        if message["method"].as_str() == Some(method) {
            return message;
        }
    }
}

fn read_message(reader: &mut BufReader<ChildStdout>) -> Value {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).unwrap();
        assert!(bytes != 0, "server stdout closed before a full LSP message");
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length: ") {
            content_length = Some(value.parse::<usize>().unwrap());
        }
    }

    let length = content_length.expect("content length");
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn write_fake_command(path: &std::path::Path, body: &str) {
    let mut file = std::fs::File::create(path).unwrap();
    write!(file, "#!/bin/sh\n{body}").unwrap();
    file.sync_all().unwrap();
    drop(file);
    make_executable(path);
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) {}

fn shutdown(child: &mut Child, mut stdin: ChildStdin, reader: &mut BufReader<ChildStdout>) {
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "shutdown",
            "params": null
        }),
    );
    let _ = read_response(reader, 99);
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}
