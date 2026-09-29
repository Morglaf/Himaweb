use super::runner::{CliResult, CliRunner};

#[derive(Clone)]
pub struct MiradorClient {
    bin: String,
    runner: CliRunner,
}

impl MiradorClient {
    pub fn new(bin: String, runner: CliRunner) -> Self {
        Self { bin, runner }
    }

    pub fn bin(&self) -> &str {
        &self.bin
    }

    /// Vérifie que Mirador répond (status / version / watch --help via list).
    pub async fn status(&self) -> CliResult<String> {
        let attempts: &[&[&str]] = &[&["status"], &["version"], &["--version"]];
        let mut last_err = None;
        for args in attempts {
            match self.runner.run_raw(&self.bin, args).await {
                Ok(bytes) => {
                    let s = String::from_utf8_lossy(&bytes).trim().to_string();
                    if !s.is_empty() {
                        return Ok(s);
                    }
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            super::runner::CliError::Message("Mirador: statut indisponible".into())
        }))
    }

    /// Déclenche un watch ponctuel / diagnose (selon CLI).
    pub async fn watch_once(&self, mailbox: Option<&str>) -> CliResult<String> {
        let mut args = vec!["watch".to_string()];
        if let Some(m) = mailbox.filter(|s| !s.is_empty()) {
            args.push(m.to_string());
        }
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        match self.runner.run_raw(&self.bin, &refs).await {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().to_string()),
            Err(e) => Err(e),
        }
    }
}
