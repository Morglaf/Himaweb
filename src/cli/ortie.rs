use super::runner::{CliResult, CliRunner};

#[derive(Clone)]
pub struct OrtieClient {
    bin: String,
    runner: CliRunner,
}

impl OrtieClient {
    pub fn new(bin: String, runner: CliRunner) -> Self {
        Self { bin, runner }
    }

    pub fn bin(&self) -> &str {
        &self.bin
    }

    /// Lance le flux OAuth Ortie pour un compte / profil.
    pub async fn authorize(&self, account: Option<&str>) -> CliResult<String> {
        let attempts: &[&[&str]] = &[
            &["authorize"],
            &["auth"],
            &["oauth"],
            &["login"],
        ];
        let mut last_err = None;
        for base in attempts {
            let mut args: Vec<String> = Vec::new();
            if let Some(a) = account.filter(|s| !s.is_empty()) {
                args.push("--account".into());
                args.push(a.to_string());
            }
            args.extend(base.iter().map(|s| (*s).to_string()));
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            match self.runner.run_raw(&self.bin, &refs).await {
                Ok(bytes) => {
                    return Ok(String::from_utf8_lossy(&bytes).trim().to_string());
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            super::runner::CliError::Message(
                "Ortie: aucune sous-commande OAuth reconnue (authorize/auth/oauth/login)".into(),
            )
        }))
    }
}
