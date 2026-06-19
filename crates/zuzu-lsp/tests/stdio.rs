use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use url::Url;

#[test]
fn serves_basic_stdio_requests() {
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
    assert!(names.contains(&"main"));

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
