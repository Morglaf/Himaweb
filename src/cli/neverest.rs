use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::runner::{CliError, CliResult, CliRunner, hide_console_for};
use crate::data_backup::BackupStatusHandle;

#[derive(Clone)]
pub struct NeverestClient {
    bin: String,
}

impl NeverestClient {
    pub fn new(bin: String, _runner: CliRunner) -> Self {
        Self { bin }
    }

    /// Lance une synchronisation (tente `sync` puis `synchronize`).
    #[allow(dead_code)]
    pub async fn sync(&self, account: Option<&str>) -> CliResult<String> {
        let attempts: &[&[&str]] = &[&["sync"], &["synchronize"], &["backup"]];
        let mut last_err = None;
        for base in attempts {
            let mut args: Vec<String> = Vec::new();
            if let Some(a) = account.filter(|s| !s.is_empty()) {
                args.push("--account".into());
                args.push(a.to_string());
            }
            args.extend(base.iter().map(|s| (*s).to_string()));
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let runner = CliRunner::new(Duration::from_secs(120));
            match runner.run_json(&self.bin, &refs).await {
                Ok(v) => {
                    return Ok(v.to_string());
                }
                Err(e) => match runner.run_raw(&self.bin, &refs).await {
                    Ok(bytes) => {
                        return Ok(String::from_utf8_lossy(&bytes).trim().to_string());
                    }
                    Err(_) => last_err = Some(e),
                },
            }
        }
        Err(last_err.unwrap_or_else(|| {
            CliError::Message("Neverest: aucune sous-commande sync connue".into())
        }))
    }

    /// Exécute une commande avec `-c <config>` et un timeout dédié (backup long).
    ///
    /// Sous Windows, Neverest découpe les chemins `-c` sur `:` (séparateur multi-config),
    /// ce qui casse `C:\…`. On lance donc depuis le dossier parent avec un `-c` relatif.
    #[allow(dead_code)]
    pub async fn run_config_cmd(
        &self,
        config: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> CliResult<String> {
        let parent = config.parent().unwrap_or_else(|| Path::new("."));
        let name = config
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("neverest-backup.toml")
            .to_string();
        let mut full: Vec<String> = vec!["-c".into(), name];
        full.extend(args.iter().map(|s| (*s).to_string()));
        let refs: Vec<&str> = full.iter().map(|s| s.as_str()).collect();
        let long = CliRunner::new(timeout);
        match long.run_json_cwd(&self.bin, &refs, Some(parent)).await {
            Ok(v) => Ok(v.to_string()),
            Err(e_json) => match long.run_raw_cwd(&self.bin, &refs, Some(parent)).await {
                Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().to_string()),
                Err(_) => Err(e_json),
            },
        }
    }

    /// Comme `run_config_cmd`, mais streame stderr (`--log-level info`) vers le statut backup.
    pub async fn run_config_cmd_logged(
        &self,
        config: &Path,
        args: &[&str],
        timeout_dur: Duration,
        status: &BackupStatusHandle,
    ) -> CliResult<String> {
        let parent = config.parent().unwrap_or_else(|| Path::new("."));
        let name = config
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("neverest-backup.toml")
            .to_string();

        let mut full: Vec<String> = vec![
            "-c".into(),
            name,
            "--log-level".into(),
            "info".into(),
        ];
        full.extend(args.iter().map(|s| (*s).to_string()));
        if !full.iter().any(|a| a == "--json") {
            // Pas de --json ici : on veut des logs humains sur stderr.
        }

        {
            let mut s = status.lock().await;
            s.push_log(format!("$ neverest {}", full.join(" ")));
        }

        let mut cmd = Command::new(&self.bin);
        hide_console_for(&mut cmd);
        cmd.current_dir(parent)
            .args(&full)
            .env_remove("NEVEREST_CONFIG")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| CliError::Spawn(e.to_string()))?;

        let stderr = child.stderr.take();
        let stdout = child.stdout.take();
        let status_err = status.clone();
        let stderr_task = tokio::spawn(async move {
            let Some(stderr) = stderr else { return };
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let mut s = status_err.lock().await;
                s.push_log(trimmed);
                // Garde un message lisible sans noyer avec du bruit debug.
                if trimmed.len() < 180
                    && !trimmed.starts_with("TRACE")
                    && !trimmed.starts_with("DEBUG")
                {
                    s.message = trimmed.to_string();
                }
            }
        });

        let stdout_task = tokio::spawn(async move {
            let Some(stdout) = stdout else {
                return String::new();
            };
            let mut lines = BufReader::new(stdout).lines();
            let mut buf = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&line);
            }
            buf
        });

        let wait_result = tokio::time::timeout(timeout_dur, child.wait()).await;
        match wait_result {
            Err(_) => {
                let _ = child.kill().await;
                let _ = stderr_task.await;
                let _ = stdout_task.await;
                {
                    let mut s = status.lock().await;
                    s.push_log(format!("Timeout après {timeout_dur:?} — processus Neverest tué."));
                }
                return Err(CliError::Timeout(timeout_dur));
            }
            Ok(Err(e)) => {
                let _ = stderr_task.await;
                let _ = stdout_task.await;
                return Err(CliError::Spawn(e.to_string()));
            }
            Ok(Ok(status_code)) => {
                let _ = stderr_task.await;
                let out = stdout_task.await.unwrap_or_default();
                if !status_code.success() {
                    let code = status_code.code().unwrap_or(-1);
                    let msg = if out.trim().is_empty() {
                        format!("neverest exit {code}")
                    } else {
                        out.chars().take(800).collect()
                    };
                    // Exit 2 = conflits / items en attente : pas un échec total pour un backup.
                    if code == 2 {
                        let mut s = status.lock().await;
                        s.push_log(format!("neverest exit 2 (items en attente): {msg}"));
                        return Ok(msg);
                    }
                    return Err(CliError::Exit {
                        code,
                        stderr: msg,
                    });
                }
                Ok(out.trim().to_string())
            }
        }
    }
}
