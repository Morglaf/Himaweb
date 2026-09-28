use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("timeout après {0:?}")]
    Timeout(Duration),
    #[error("échec lancement: {0}")]
    Spawn(String),
    #[error("code de sortie {code}: {stderr}")]
    Exit { code: i32, stderr: String },
    #[error("JSON invalide: {0}")]
    Json(String),
    #[error("{0}")]
    Message(String),
}

pub type CliResult<T> = Result<T, CliError>;

#[derive(Clone)]
pub struct CliRunner {
    timeout: Duration,
}

impl CliRunner {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    pub async fn run_json(&self, bin: &str, args: &[&str]) -> CliResult<Value> {
        let mut full_args: Vec<&str> = Vec::with_capacity(args.len() + 1);
        // Prefer global --json early in argv
        if !args.iter().any(|a| *a == "--json") {
            full_args.push("--json");
        }
        full_args.extend_from_slice(args);

        let output = timeout(self.timeout, async {
            Command::new(bin)
                .args(&full_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .output()
                .await
        })
        .await
        .map_err(|_| CliError::Timeout(self.timeout))?
        .map_err(|e| CliError::Spawn(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            // Some CLIs still emit JSON on stderr/stdout when failing
            if let Ok(v) = serde_json::from_str::<Value>(&stdout) {
                if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                    let detail = v
                        .get("sources")
                        .and_then(|s| s.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str())
                                .collect::<Vec<_>>()
                                .join(" | ")
                        })
                        .filter(|s| !s.is_empty());
                    return Err(CliError::Message(match detail {
                        Some(d) => format!("{err}: {d}"),
                        None => err.to_string(),
                    }));
                }
            }
            let msg = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("commande échouée (code {code})")
            };
            return Err(CliError::Exit { code, stderr: msg });
        }

        if stdout.is_empty() {
            return Ok(Value::Null);
        }

        serde_json::from_str(&stdout).map_err(|e| CliError::Json(format!("{e}: {stdout}")))
    }

    pub async fn run_raw(&self, bin: &str, args: &[&str]) -> CliResult<Vec<u8>> {
        let output = timeout(self.timeout, async {
            Command::new(bin)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .output()
                .await
        })
        .await
        .map_err(|_| CliError::Timeout(self.timeout))?
        .map_err(|e| CliError::Spawn(e.to_string()))?;

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(CliError::Exit { code, stderr });
        }
        Ok(output.stdout)
    }

    pub async fn run_with_stdin(
        &self,
        bin: &str,
        args: &[&str],
        stdin_data: &[u8],
    ) -> CliResult<Value> {
        use tokio::io::AsyncWriteExt;

        let mut full_args: Vec<&str> = Vec::with_capacity(args.len() + 1);
        if !args.iter().any(|a| *a == "--json") {
            full_args.push("--json");
        }
        full_args.extend_from_slice(args);

        let result = timeout(self.timeout, async {
            let mut child = Command::new(bin)
                .args(&full_args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| CliError::Spawn(e.to_string()))?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(stdin_data)
                    .await
                    .map_err(|e| CliError::Spawn(e.to_string()))?;
            }

            child
                .wait_with_output()
                .await
                .map_err(|e| CliError::Spawn(e.to_string()))
        })
        .await
        .map_err(|_| CliError::Timeout(self.timeout))??;

        let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).trim().to_string();

        if !result.status.success() {
            let code = result.status.code().unwrap_or(-1);
            let msg = if !stderr.is_empty() { stderr } else { stdout };
            return Err(CliError::Exit { code, stderr: msg });
        }

        if stdout.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&stdout).map_err(|e| CliError::Json(e.to_string()))
    }

    pub async fn which_exists(bin: &str) -> bool {
        which::which(bin).is_ok() || PathBuf::from(bin).exists()
    }
}
