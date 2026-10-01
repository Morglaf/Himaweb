use super::runner::{CliError, CliResult, CliRunner};

#[derive(Clone)]
pub struct OrtieClient {
    bin: String,
    #[allow(dead_code)]
    runner: CliRunner,
}

impl OrtieClient {
    pub fn new(bin: String, runner: CliRunner) -> Self {
        Self { bin, runner }
    }


    /// Lance `ortie [-a ACCOUNT] auth get` dans une console dédiée, avec log fichier.
    pub async fn authorize(&self, account: Option<&str>) -> CliResult<String> {
        let log_path = crate::accounts_config::ortie_config_path()
            .parent()
            .map(|p| p.join("auth.log"))
            .unwrap_or_else(|| std::path::PathBuf::from("ortie-auth.log"));
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut args: Vec<String> = Vec::new();
        if let Some(a) = account.filter(|s| !s.is_empty()) {
            args.push("-a".into());
            args.push(a.to_string());
        }
        args.push("--log-level".into());
        args.push("debug".into());
        args.push("--log-file".into());
        args.push(log_path.display().to_string());
        args.push("auth".into());
        args.push("get".into());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
            let mut cmd = std::process::Command::new(&self.bin);
            cmd.args(&args).creation_flags(CREATE_NEW_CONSOLE);
            match cmd.spawn() {
                Ok(_) => {
                    let name = account.filter(|s| !s.is_empty()).unwrap_or("(défaut)");
                    Ok(format!(
                        "Fenêtre Ortie ouverte pour « {name} ». Terminez Google dans le navigateur et attendez « success » dans la console (ne la fermez pas trop tôt). Log: {}",
                        log_path.display()
                    ))
                }
                Err(e) => Err(CliError::Spawn(e.to_string())),
            }
        }

        #[cfg(not(windows))]
        {
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            match self.runner.run_raw(&self.bin, &refs).await {
                Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().to_string()),
                Err(e) => Err(e),
            }
        }
    }
}
