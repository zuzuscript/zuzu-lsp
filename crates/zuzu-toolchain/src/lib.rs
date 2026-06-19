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
    pub tidy: Option<PathBuf>,
    pub zuzudoc: Option<PathBuf>,
    pub zuzuprove: Option<PathBuf>,
    pub zuzubox: Option<PathBuf>,
    pub stdlib: Option<PathBuf>,
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
}

impl Toolchain {
    pub fn discover(roots: &[PathBuf]) -> Self {
        let mut candidates = CandidatePaths::new(roots);

        Self {
            zuzu: candidates.find(
                "zuzu",
                &["zuzu-perl/bin/zuzu", "zuzu-rust/target/debug/zuzu-rust"],
            ),
            tidy: candidates.find("zuzu-tidy.pl", &["zuzu-perl/bin/zuzu-tidy.pl"]),
            zuzudoc: candidates.find("zuzudoc.pl", &["zuzu-perl/bin/zuzudoc.pl"]),
            zuzuprove: candidates.find(
                "zuzuprove",
                &[
                    "stdlib/scripts/zuzuprove",
                    "zuzu-perl/stdlib/scripts/zuzuprove",
                    "zuzu-rust/stdlib/scripts/zuzuprove",
                    "zuzu-js/stdlib/scripts/zuzuprove",
                ],
            ),
            zuzubox: candidates.find("zuzubox", &["tobyink-dists/zuzubox/scripts/zuzubox"]),
            stdlib: candidates.find_dir(&[
                "stdlib",
                "zuzu-perl/stdlib",
                "zuzu-rust/stdlib",
                "zuzu-js/stdlib",
            ]),
        }
    }

    pub fn format_text(&self, text: &str) -> Result<String, ToolchainError> {
        let Some(formatter) = &self.tidy else {
            return Err(ToolchainError::MissingFormatter);
        };

        let line_ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let mut child = Command::new(formatter)
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

        let mut formatted = String::from_utf8_lossy(&output.stdout).to_string();
        if line_ending == "\r\n" {
            formatted = formatted.replace('\n', "\r\n");
        }
        Ok(formatted)
    }

    pub fn doctor_lines(&self) -> Vec<String> {
        vec![
            format_tool("zuzu", self.zuzu.as_deref()),
            format_tool("zuzu-tidy.pl", self.tidy.as_deref()),
            format_tool("zuzudoc.pl", self.zuzudoc.as_deref()),
            format_tool("zuzuprove", self.zuzuprove.as_deref()),
            format_tool("zuzubox", self.zuzubox.as_deref()),
            format_tool("stdlib", self.stdlib.as_deref()),
        ]
    }
}

struct CandidatePaths {
    roots: Vec<PathBuf>,
    path_dirs: Vec<PathBuf>,
}

impl CandidatePaths {
    fn new(roots: &[PathBuf]) -> Self {
        let path_dirs = env::var_os("PATH")
            .map(|path| env::split_paths(&path).collect())
            .unwrap_or_default();
        let mut roots = roots.to_vec();
        if let Ok(cwd) = env::current_dir() {
            roots.push(cwd);
        }
        roots.sort();
        roots.dedup();
        Self { roots, path_dirs }
    }

    fn find(&mut self, executable: &str, relatives: &[&str]) -> Option<PathBuf> {
        for directory in &self.path_dirs {
            let candidate = directory.join(executable);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }

        for root in &self.roots {
            for relative in relatives {
                let candidate = root.join(relative);
                if is_executable_file(&candidate) {
                    return Some(candidate);
                }
            }
        }

        None
    }

    fn find_dir(&self, relatives: &[&str]) -> Option<PathBuf> {
        for root in &self.roots {
            for relative in relatives {
                let candidate = root.join(relative);
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn format_tool(name: &str, path: Option<&Path>) -> String {
    match path {
        Some(path) => format!("{name}: {}", path.display()),
        None => format!("{name}: not found"),
    }
}

#[allow(dead_code)]
fn _path_os_string(path: &Path) -> OsString {
    path.as_os_str().to_os_string()
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
}
