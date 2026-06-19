use std::process::Command;

#[test]
fn reports_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_zuzu-lsp"))
        .arg("--version")
        .output()
        .expect("run zuzu-lsp --version");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("zuzu-lsp "));
}

#[test]
fn suggests_stdio_or_doctor_without_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_zuzu-lsp"))
        .output()
        .expect("run zuzu-lsp");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--stdio"));
    assert!(stderr.contains("zuzu-lsp doctor"));
}

#[test]
fn reports_doctor_lines() {
    let output = Command::new(env!("CARGO_BIN_EXE_zuzu-lsp"))
        .arg("doctor")
        .output()
        .expect("run zuzu-lsp doctor");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("zuzu-tidy.pl:"));
    assert!(stdout.contains("module search paths:"));
}
