use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct Toolchain {
    pub zuzu: Option<PathBuf>,
    pub zuzu_version: Option<String>,
    pub tidy: Option<PathBuf>,
    pub pod_parse: Option<PathBuf>,
    pub zuzudoc: Option<PathBuf>,
    pub zuzuprove: Option<PathBuf>,
    pub zuzubox: Option<PathBuf>,
    pub module_search_paths: Vec<PathBuf>,
    pub stdlib: Option<PathBuf>,
    pub installed_modules: Vec<PathBuf>,
}

#[derive(Debug, Error)]
pub enum ToolchainError {
    #[error("zuzu-tidy.pl was not found")]
    MissingFormatter,
    #[error("could not open formatter stdin for `{command}`")]
    FormatterStdinUnavailable { command: PathBuf },
    #[error("could not write source to formatter `{command}`: {source}")]
    WriteFormatterStdin {
        command: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not run formatter `{command}`: {source}")]
    RunFormatter {
        command: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("formatter failed with exit status {status}: {stderr}")]
    FormatterFailed { status: String, stderr: String },
    #[error("neither pod_parse nor zuzudoc.pl was found")]
    MissingDocumentationRenderer,
    #[error("could not run documentation renderer `{command}`: {source}")]
    RunDocumentationRenderer {
        command: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("documentation renderer failed with exit status {status}: {stderr}")]
    DocumentationRendererFailed { status: String, stderr: String },
    #[error("{tool} was not found")]
    MissingTool { tool: &'static str },
    #[error("could not run `{command}`: {source}")]
    RunTool {
        command: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub command: Vec<String>,
    pub status: String,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParserDiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserDiagnostic {
    pub severity: ParserDiagnosticSeverity,
    pub kind: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl Toolchain {
    pub fn discover(_roots: &[PathBuf]) -> Self {
        let candidates = CandidatePaths::new();
        let zuzu = candidates.find("zuzu");
        let runtime_info = zuzu.as_deref().and_then(runtime_info);
        let module_search_paths = runtime_info
            .as_ref()
            .map(|info| info.module_search_paths.clone())
            .filter(|paths| !paths.is_empty())
            .unwrap_or_else(fallback_module_search_paths);

        Self {
            zuzu,
            zuzu_version: runtime_info.and_then(|info| info.version),
            tidy: candidates.find("zuzu-tidy.pl"),
            pod_parse: candidates.find("pod_parse"),
            zuzudoc: candidates.find("zuzudoc.pl"),
            zuzuprove: candidates.find("zuzuprove"),
            zuzubox: candidates.find("zuzubox"),
            module_search_paths,
            stdlib: configured_stdlib_dir(),
            installed_modules: candidates.find_installed_module_dirs(),
        }
    }

    pub fn format_text(&self, text: &str) -> Result<String, ToolchainError> {
        let Some(formatter) = &self.tidy else {
            return Err(ToolchainError::MissingFormatter);
        };

        let line_ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let mut process = Command::new(formatter);
        apply_minimal_environment(&mut process, None);
        let mut child = process
            .arg("--stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| ToolchainError::RunFormatter {
                command: formatter.clone(),
                source,
            })?;

        let mut stdin =
            child
                .stdin
                .take()
                .ok_or_else(|| ToolchainError::FormatterStdinUnavailable {
                    command: formatter.clone(),
                })?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|source| ToolchainError::WriteFormatterStdin {
                command: formatter.clone(),
                source,
            })?;
        drop(stdin);

        let output = child
            .wait_with_output()
            .map_err(|source| ToolchainError::RunFormatter {
                command: formatter.clone(),
                source,
            })?;

        if !output.status.success() {
            return Err(ToolchainError::FormatterFailed {
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        let formatted = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(normalize_line_endings(&formatted, line_ending))
    }

    pub fn render_pod_markdown(&self, path: &Path) -> Result<Option<String>, ToolchainError> {
        if !has_pod(path) {
            return Ok(None);
        }

        let rendered = if let Some(pod_parse) = &self.pod_parse {
            run_documentation_command(pod_parse, &["-f", "markdown"], path, None)?
        } else if let Some(zuzudoc) = &self.zuzudoc {
            run_documentation_command(zuzudoc, &[], path, Some(("PAGER", "cat")))?
        } else {
            return Err(ToolchainError::MissingDocumentationRenderer);
        };

        let rendered = rendered.trim().to_string();
        Ok((!rendered.is_empty()).then_some(rendered))
    }

    pub fn render_docs(&self, path: &Path) -> Result<ToolOutput, ToolchainError> {
        if let Some(pod_parse) = &self.pod_parse {
            self.run_tool_with_env(
                pod_parse,
                &["-f".into(), "markdown".into(), path.into()],
                None,
            )
        } else if let Some(zuzudoc) = &self.zuzudoc {
            self.run_tool_with_env(zuzudoc, &[path.into()], Some(("PAGER", "cat")))
        } else {
            Err(ToolchainError::MissingDocumentationRenderer)
        }
    }

    pub fn run_test_file(&self, path: &Path) -> Result<ToolOutput, ToolchainError> {
        let Some(zuzuprove) = &self.zuzuprove else {
            return Err(ToolchainError::MissingTool { tool: "zuzuprove" });
        };
        self.run_tool(zuzuprove, &[path.into()])
    }

    pub fn run_workspace_tests(&self, path: &Path) -> Result<ToolOutput, ToolchainError> {
        let Some(zuzuprove) = &self.zuzuprove else {
            return Err(ToolchainError::MissingTool { tool: "zuzuprove" });
        };
        self.run_tool(zuzuprove, &[path.into()])
    }

    pub fn verify_distribution(&self, path: &Path) -> Result<ToolOutput, ToolchainError> {
        let Some(zuzubox) = &self.zuzubox else {
            return Err(ToolchainError::MissingTool { tool: "zuzubox" });
        };
        self.run_tool(zuzubox, &["verify".into(), path.into()])
    }

    pub fn lint_text(&self, text: &str) -> Result<Vec<ParserDiagnostic>, ToolchainError> {
        let Some(zuzu) = &self.zuzu else {
            return Err(ToolchainError::MissingTool { tool: "zuzu" });
        };
        let output = self.run_tool(zuzu, &["--lint".into(), "-e".into(), text.into()])?;
        Ok(parse_lint_diagnostics(&output.stderr))
    }

    pub fn doctor_lines(&self) -> Vec<String> {
        vec![
            format_tool("zuzu", self.zuzu.as_deref()),
            format_optional("zuzu version", self.zuzu_version.as_deref()),
            format_tool("zuzu-tidy.pl", self.tidy.as_deref()),
            format_tool("pod_parse", self.pod_parse.as_deref()),
            format_tool("zuzudoc.pl", self.zuzudoc.as_deref()),
            format_tool("zuzuprove", self.zuzuprove.as_deref()),
            format_tool("zuzubox", self.zuzubox.as_deref()),
            format_paths("module search paths", &self.module_search_paths),
            format_tool("ZUZU_STDLIB", self.stdlib.as_deref()),
            format_paths("installed modules", &self.installed_modules),
        ]
    }

    fn run_tool(&self, command: &Path, args: &[OsString]) -> Result<ToolOutput, ToolchainError> {
        self.run_tool_with_env(command, args, None)
    }

    fn run_tool_with_env(
        &self,
        command: &Path,
        args: &[OsString],
        env_pair: Option<(&str, &str)>,
    ) -> Result<ToolOutput, ToolchainError> {
        let mut process = Command::new(command);
        process.args(args);
        apply_minimal_environment(&mut process, env_pair);
        let output = process
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|source| ToolchainError::RunTool {
                command: command.to_path_buf(),
                source,
            })?;

        let mut rendered_command = vec![command.display().to_string()];
        rendered_command.extend(args.iter().map(|arg| arg.to_string_lossy().to_string()));

        Ok(ToolOutput {
            command: rendered_command,
            status: output.status.to_string(),
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

struct CandidatePaths {
    bin_dirs: Vec<PathBuf>,
    installed_module_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeInfo {
    version: Option<String>,
    module_search_paths: Vec<PathBuf>,
}

impl CandidatePaths {
    fn new() -> Self {
        let mut bin_dirs: Vec<PathBuf> = env::var_os("PATH")
            .map(|path| env::split_paths(&path).collect())
            .unwrap_or_default();
        bin_dirs.extend(default_bin_dirs());
        dedup_paths(&mut bin_dirs);

        let mut installed_module_dirs = default_installed_module_dirs();
        dedup_paths(&mut installed_module_dirs);

        Self {
            bin_dirs,
            installed_module_dirs,
        }
    }

    fn find(&self, executable: &str) -> Option<PathBuf> {
        for directory in &self.bin_dirs {
            let candidate = directory.join(executable);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }

        None
    }

    fn find_installed_module_dirs(&self) -> Vec<PathBuf> {
        self.installed_module_dirs
            .iter()
            .filter(|path| path.is_dir())
            .cloned()
            .collect()
    }

    #[cfg(test)]
    fn from_parts(bin_dirs: Vec<PathBuf>, module_dirs: Vec<PathBuf>) -> Self {
        Self {
            bin_dirs,
            installed_module_dirs: module_dirs,
        }
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn format_tool(name: &str, path: Option<&Path>) -> String {
    match path {
        Some(path) => format!("{name}: {}", path.display()),
        None => format!("{name}: not found"),
    }
}

fn format_optional(name: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => format!("{name}: {value}"),
        None => format!("{name}: not found"),
    }
}

fn format_paths(name: &str, paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return format!("{name}: not found");
    }
    format!(
        "{name}: {}",
        paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn default_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        if let Some(userprofile) = env::var_os("USERPROFILE") {
            dirs.push(PathBuf::from(userprofile).join(".zuzu").join("bin"));
        }
    } else {
        if let Some(home) = env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".zuzu").join("bin"));
        }
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    dirs
}

fn configured_stdlib_dir() -> Option<PathBuf> {
    if let Some(stdlib) = env::var_os("ZUZU_STDLIB") {
        let path = PathBuf::from(stdlib);
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}

fn fallback_module_search_paths() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(paths) = env::var_os("ZUZULIB") {
        roots.extend(env::split_paths(&paths));
    }
    roots.extend(default_installed_module_dirs());
    if let Some(stdlib) = configured_stdlib_dir() {
        roots.push(stdlib);
    }
    dedup_paths(&mut roots);
    roots.retain(|path| path.is_dir());
    roots
}

fn runtime_info(zuzu: &Path) -> Option<RuntimeInfo> {
    let mut process = Command::new(zuzu);
    apply_minimal_environment(&mut process, None);
    let output = process
        .arg("-V")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut info = parse_verbose_runtime_info(&stdout);
    info.module_search_paths.retain(|path| path.is_dir());
    dedup_paths(&mut info.module_search_paths);
    Some(info)
}

fn parse_verbose_runtime_info(output: &str) -> RuntimeInfo {
    RuntimeInfo {
        version: parse_verbose_runtime_version(output),
        module_search_paths: parse_verbose_module_search_paths(output),
    }
}

fn parse_verbose_runtime_version(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn parse_verbose_module_search_paths(output: &str) -> Vec<PathBuf> {
    let mut in_search_paths = false;
    let mut paths = Vec::new();

    for line in output.lines() {
        if is_verbose_module_search_heading(line.trim()) {
            in_search_paths = true;
            continue;
        }
        if !in_search_paths {
            continue;
        }
        if line.trim().is_empty() {
            if !paths.is_empty() {
                break;
            }
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            paths.push(PathBuf::from(line.trim()));
            continue;
        }
        break;
    }

    paths
}

fn is_verbose_module_search_heading(line: &str) -> bool {
    matches!(line, "lib search paths:" | "module search paths:")
}

fn default_installed_module_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        if let Some(userprofile) = env::var_os("USERPROFILE") {
            dirs.push(PathBuf::from(userprofile).join(".zuzu").join("modules"));
        }
    } else {
        if let Some(home) = env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".zuzu").join("modules"));
        }
        dirs.push(PathBuf::from("/var/lib/zuzu/modules"));
    }
    dirs
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = Vec::new();
    paths.retain(|path| {
        if seen.iter().any(|seen_path| seen_path == path) {
            false
        } else {
            seen.push(path.clone());
            true
        }
    });
}

fn has_pod(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.lines()
        .any(|line| line.starts_with("=pod") || line.starts_with("=head"))
}

fn run_documentation_command(
    command: &Path,
    args: &[&str],
    path: &Path,
    env_pair: Option<(&str, &str)>,
) -> Result<String, ToolchainError> {
    let mut process = Command::new(command);
    process.args(args).arg(path);
    apply_minimal_environment(&mut process, env_pair);
    let output = process
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| ToolchainError::RunDocumentationRenderer {
            command: command.to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        return Err(ToolchainError::DocumentationRendererFailed {
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn apply_minimal_environment(process: &mut Command, env_pair: Option<(&str, &str)>) {
    process.env_clear();
    for key in MINIMAL_ENV_KEYS {
        if let Some(value) = env::var_os(key) {
            process.env(key, value);
        }
    }
    if let Some((key, value)) = env_pair {
        process.env(key, value);
    }
}

const MINIMAL_ENV_KEYS: &[&str] = &[
    "APPDATA",
    "COMSPEC",
    "HOME",
    "LANG",
    "LC_ALL",
    "LOCALAPPDATA",
    "PATH",
    "PATHEXT",
    "PERL5LIB",
    "SystemRoot",
    "TEMP",
    "TMP",
    "TMPDIR",
    "USERPROFILE",
    "ZUZU_STDLIB",
    "ZUZULIB",
];

fn normalize_line_endings(text: &str, line_ending: &str) -> String {
    let normalised = text.replace("\r\n", "\n");
    if line_ending == "\r\n" {
        normalised.replace('\n', "\r\n")
    } else {
        normalised
    }
}

fn parse_lint_diagnostics(stderr: &str) -> Vec<ParserDiagnostic> {
    stderr.lines().filter_map(parse_lint_diagnostic).collect()
}

fn parse_lint_diagnostic(line: &str) -> Option<ParserDiagnostic> {
    parse_located_lint_error(line).or_else(|| parse_semantic_warning(line))
}

fn parse_located_lint_error(line: &str) -> Option<ParserDiagnostic> {
    for kind in [
        "lex error",
        "parse error",
        "incomplete parse error",
        "semantic error",
    ] {
        let Some(rest) = line
            .strip_prefix(kind)
            .and_then(|rest| rest.strip_prefix(" at "))
        else {
            continue;
        };
        let Some((location, message)) = rest.rsplit_once(": ") else {
            continue;
        };
        let Some((line_number, column)) = parse_lint_location(location) else {
            continue;
        };
        return Some(ParserDiagnostic {
            severity: ParserDiagnosticSeverity::Error,
            kind: kind.to_string(),
            line: line_number,
            column,
            message: message.to_string(),
        });
    }
    None
}

fn parse_lint_location(location: &str) -> Option<(usize, usize)> {
    let (line, column) = location.rsplit_once(':')?;
    let line = line.rsplit_once(':').map(|(_, line)| line).unwrap_or(line);
    Some((line.parse().ok()?, column.parse().ok()?))
}

fn parse_semantic_warning(line: &str) -> Option<ParserDiagnostic> {
    let rest = line.strip_prefix("SemanticWarning at line ")?;
    let (line, message) = rest.split_once(": ")?;
    Some(ParserDiagnostic {
        severity: ParserDiagnosticSeverity::Warning,
        kind: "semantic warning".to_string(),
        line: line.parse().ok()?,
        column: 1,
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_reports_missing_tools() {
        let toolchain = Toolchain::default();
        let lines = toolchain.doctor_lines();
        assert!(lines.iter().any(|line| line == "zuzu-tidy.pl: not found"));
    }

    #[test]
    fn finds_binaries_from_configured_bin_dirs_only() {
        let root = unique_temp_dir("zuzu-toolchain-bin-test");
        fs::create_dir_all(&root).unwrap();
        let binary = root.join("zuzu-tidy.pl");
        fs::write(&binary, "#!/bin/sh\n").unwrap();
        make_executable(&binary);

        let candidates = CandidatePaths::from_parts(vec![root.clone()], Vec::new());
        assert_eq!(candidates.find("zuzu-tidy.pl"), Some(binary));

        let _ = fs::remove_file(root.join("zuzu-tidy.pl"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn reports_stdlib_separately_from_installed_modules() {
        let toolchain = Toolchain {
            stdlib: None,
            installed_modules: vec![PathBuf::from("/example/.zuzu/modules")],
            zuzu_version: Some("zuzu-rust version 0.6.0".to_string()),
            ..Default::default()
        };
        let lines = toolchain.doctor_lines();
        assert!(lines
            .iter()
            .any(|line| { line == "zuzu version: zuzu-rust version 0.6.0" }));
        assert!(lines.iter().any(|line| line == "ZUZU_STDLIB: not found"));
        assert!(lines
            .iter()
            .any(|line| { line == "installed modules: /example/.zuzu/modules" }));
    }

    #[test]
    fn finds_installed_modules_without_treating_them_as_stdlib() {
        let root = unique_temp_dir("zuzu-toolchain-modules-test");
        fs::create_dir_all(&root).unwrap();

        let candidates = CandidatePaths::from_parts(Vec::new(), vec![root.clone()]);
        assert_eq!(candidates.find_installed_module_dirs(), vec![root.clone()]);

        let _ = fs::remove_dir(root);
    }

    #[test]
    fn parses_lint_error_diagnostics() {
        let diagnostics = parse_lint_diagnostics(
            "parse error at main.zzs:1:12: Expected expression\n\
             incomplete parse error at 3:4: Expected expression\n",
        );
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].severity, ParserDiagnosticSeverity::Error);
        assert_eq!(diagnostics[0].kind, "parse error");
        assert_eq!(diagnostics[0].line, 1);
        assert_eq!(diagnostics[0].column, 12);
        assert_eq!(diagnostics[0].message, "Expected expression");
        assert_eq!(diagnostics[1].kind, "incomplete parse error");
        assert_eq!(diagnostics[1].line, 3);
        assert_eq!(diagnostics[1].column, 4);
    }

    #[test]
    fn parses_lint_warning_diagnostics() {
        let diagnostics = parse_lint_diagnostics(
            "SemanticWarning at line 2: prefer 'instanceof' for runtime type checks\n",
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, ParserDiagnosticSeverity::Warning);
        assert_eq!(diagnostics[0].kind, "semantic warning");
        assert_eq!(diagnostics[0].line, 2);
        assert_eq!(diagnostics[0].column, 1);
        assert_eq!(
            diagnostics[0].message,
            "prefer 'instanceof' for runtime type checks"
        );
    }

    #[test]
    fn format_text_preserves_crlf_when_formatter_outputs_lf() {
        let root = unique_temp_dir("zuzu-toolchain-format-crlf-lf");
        fs::create_dir_all(&root).unwrap();
        let script = root.join("zuzu-tidy.pl");
        write_fake_command(&script, "cat >/dev/null\nprintf 'say 1;\\nsay 2;\\n'\n");

        let toolchain = Toolchain {
            tidy: Some(script),
            ..Default::default()
        };
        let formatted = toolchain.format_text("say 1;\r\nsay 2;\r\n").unwrap();
        assert_eq!(formatted, "say 1;\r\nsay 2;\r\n");

        let _ = fs::remove_file(root.join("zuzu-tidy.pl"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn format_text_does_not_double_crlf_from_formatter() {
        let root = unique_temp_dir("zuzu-toolchain-format-crlf-crlf");
        fs::create_dir_all(&root).unwrap();
        let script = root.join("zuzu-tidy.pl");
        write_fake_command(
            &script,
            "cat >/dev/null\nprintf 'say 1;\\r\\nsay 2;\\r\\n'\n",
        );

        let toolchain = Toolchain {
            tidy: Some(script),
            ..Default::default()
        };
        let formatted = toolchain.format_text("say 1;\r\nsay 2;\r\n").unwrap();
        assert_eq!(formatted, "say 1;\r\nsay 2;\r\n");
        assert!(!formatted.contains("\r\r\n"));

        let _ = fs::remove_file(root.join("zuzu-tidy.pl"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn format_text_reports_formatter_stderr_without_output() {
        let root = unique_temp_dir("zuzu-toolchain-format-failure");
        fs::create_dir_all(&root).unwrap();
        let script = root.join("zuzu-tidy.pl");
        write_fake_command(&script, "printf 'bad input\\n' >&2\nexit 7\n");

        let toolchain = Toolchain {
            tidy: Some(script),
            ..Default::default()
        };
        let error = toolchain.format_text("say 1;\n").unwrap_err();
        let ToolchainError::FormatterFailed { status, stderr } = error else {
            panic!("expected formatter failure");
        };
        assert!(status.contains('7'));
        assert_eq!(stderr, "bad input");

        let _ = fs::remove_file(root.join("zuzu-tidy.pl"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn tool_commands_use_minimal_environment() {
        std::env::set_var("ZUZU_LSP_SECRET", "leak");
        let root = unique_temp_dir("zuzu-toolchain-minimal-env");
        fs::create_dir_all(&root).unwrap();
        let script = root.join("zuzuprove");
        write_fake_command(
            &script,
            "if [ \"${ZUZU_LSP_SECRET-unset}\" = leak ]; then exit 9; fi\nprintf 'ok\\n'\n",
        );

        let toolchain = Toolchain {
            zuzuprove: Some(script),
            ..Default::default()
        };
        let output = toolchain
            .run_test_file(&root.join("tests").join("example.zzs"))
            .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout, "ok\n");

        std::env::remove_var("ZUZU_LSP_SECRET");
        let _ = fs::remove_file(root.join("zuzuprove"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn documentation_commands_keep_explicit_environment_overrides() {
        std::env::set_var("ZUZU_LSP_SECRET", "leak");
        let root = unique_temp_dir("zuzu-toolchain-doc-env");
        fs::create_dir_all(&root).unwrap();
        let script = root.join("zuzudoc.pl");
        write_fake_command(
            &script,
            "if [ \"${ZUZU_LSP_SECRET-unset}\" = leak ]; then exit 9; fi\nprintf 'pager=%s\\n' \"$PAGER\"\n",
        );
        let module = root.join("mod.zzm");
        fs::write(&module, "=pod\n\n=head1 NAME\n\nmod\n").unwrap();

        let rendered = run_documentation_command(&script, &[], &module, Some(("PAGER", "cat")))
            .expect("documentation output");
        assert_eq!(rendered, "pager=cat\n");

        std::env::remove_var("ZUZU_LSP_SECRET");
        let _ = fs::remove_file(root.join("zuzudoc.pl"));
        let _ = fs::remove_file(module);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn parses_runtime_verbose_module_search_paths() {
        let output = "\
zuzu-rust version 0.6.0

lib search paths:
  /home/example/.zuzu/modules
  /var/lib/zuzu/modules
  /usr/share/zuzu-rust/modules
";
        let info = parse_verbose_runtime_info(output);
        assert_eq!(info.version, Some("zuzu-rust version 0.6.0".to_string()));
        assert_eq!(
            info.module_search_paths,
            vec![
                PathBuf::from("/home/example/.zuzu/modules"),
                PathBuf::from("/var/lib/zuzu/modules"),
                PathBuf::from("/usr/share/zuzu-rust/modules"),
            ]
        );
    }

    #[test]
    fn parses_runtime_verbose_module_search_paths_heading_variant() {
        let output = "\
zuzu-js version 0.6.0

module search paths:
  /home/example/.zuzu/modules
  /var/lib/zuzu/modules
";
        let info = parse_verbose_runtime_info(output);
        assert_eq!(info.version, Some("zuzu-js version 0.6.0".to_string()));
        assert_eq!(
            info.module_search_paths,
            vec![
                PathBuf::from("/home/example/.zuzu/modules"),
                PathBuf::from("/var/lib/zuzu/modules"),
            ]
        );
    }

    #[test]
    fn wraps_test_file_command() {
        let root = unique_temp_dir("zuzu-toolchain-test-file");
        fs::create_dir_all(&root).unwrap();
        let script = root.join("zuzuprove");
        write_fake_command(&script, "printf 'tested %s\\n' \"$1\"\n");

        let toolchain = Toolchain {
            zuzuprove: Some(script),
            ..Default::default()
        };
        let target = root.join("tests").join("example.zzs");
        let output = toolchain.run_test_file(&target).unwrap();
        assert!(output.success);
        assert!(output.stdout.contains("tested "));
        assert!(output
            .command
            .iter()
            .any(|part| part.ends_with("example.zzs")));

        let _ = fs::remove_file(root.join("zuzuprove"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn wraps_verify_distribution_command() {
        let root = unique_temp_dir("zuzu-toolchain-verify");
        fs::create_dir_all(&root).unwrap();
        let script = root.join("zuzubox");
        write_fake_command(&script, "printf '%s %s\\n' \"$1\" \"$2\"\n");

        let toolchain = Toolchain {
            zuzubox: Some(script),
            ..Default::default()
        };
        let output = toolchain.verify_distribution(&root).unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.trim(), format!("verify {}", root.display()));

        let _ = fs::remove_file(root.join("zuzubox"));
        let _ = fs::remove_dir(root);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
    }

    fn write_fake_command(path: &Path, body: &str) {
        let mut file = fs::File::create(path).unwrap();
        write!(file, "#!/bin/sh\n{body}").unwrap();
        file.sync_all().unwrap();
        drop(file);
        make_executable(path);
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &Path) {}
}
