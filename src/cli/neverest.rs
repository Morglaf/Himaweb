use super::runner::{CliResult, CliRunner};

#[derive(Clone)]
pub struct NeverestClient {
    bin: String,
    runner: CliRunner,
}

impl NeverestClient {
    pub fn new(bin: String, runner: CliRunner) -> Self {
        Self { bin, runner }
    }


    /// Lance une synchronisation (tente `sync` puis `synchronize`).
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
            match self.runner.run_json(&self.bin, &refs).await {
                Ok(v) => {
                    return Ok(v.to_string());
                }
                Err(e) => {
                    // Essayer aussi en raw (sortie texte)
                    match self.runner.run_raw(&self.bin, &refs).await {
                        Ok(bytes) => {
                            return Ok(String::from_utf8_lossy(&bytes).trim().to_string());
                        }
                        Err(_) => last_err = Some(e),
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            super::runner::CliError::Message("Neverest: aucune sous-commande sync connue".into())
        }))
    }
}
