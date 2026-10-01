use std::sync::Arc;

use crate::cache::Cache;
use crate::cli::calendula::CalendulaClient;
use crate::cli::cardamum::CardamumClient;
use crate::cli::himalaya::HimalayaClient;
use crate::cli::mirador::MiradorClient;
use crate::cli::neverest::NeverestClient;
use crate::cli::ortie::OrtieClient;
use crate::cli::runner::CliRunner;
use crate::prefs::{self, Prefs};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone)]
pub struct AppState {
    pub himalaya: HimalayaClient,
    pub cardamum: Option<CardamumClient>,
    pub calendula: Option<CalendulaClient>,
    pub neverest: Option<NeverestClient>,
    pub mirador: Option<MiradorClient>,
    pub ortie: Option<OrtieClient>,
    pub cache: Arc<Mutex<Cache>>,
    pub prefs: Arc<Mutex<Prefs>>,
    pub cli_limit: Arc<Semaphore>,
    pub himalaya_available: bool,
    pub cardamum_available: bool,
    pub calendula_available: bool,
    pub neverest_available: bool,
    pub mirador_available: bool,
    pub ortie_available: bool,
}

impl AppState {
    pub async fn init() -> Result<Self, String> {
        let himalaya_bin = std::env::var("HIMAWEB_HIMALAYA_BIN")
            .ok()
            .or_else(|| which::which("himalaya").ok().map(|p| p.display().to_string()))
            .unwrap_or_else(|| "himalaya".into());

        let cardamum_bin = std::env::var("HIMAWEB_CARDAMUM_BIN")
            .ok()
            .or_else(|| which::which("cardamum").ok().map(|p| p.display().to_string()));

        let calendula_bin = std::env::var("HIMAWEB_CALENDULA_BIN")
            .ok()
            .or_else(|| which::which("calendula").ok().map(|p| p.display().to_string()));

        let neverest_bin = std::env::var("HIMAWEB_NEVEREST_BIN")
            .ok()
            .or_else(|| which::which("neverest").ok().map(|p| p.display().to_string()));

        let mirador_bin = std::env::var("HIMAWEB_MIRADOR_BIN")
            .ok()
            .or_else(|| which::which("mirador").ok().map(|p| p.display().to_string()));

        let ortie_bin = std::env::var("HIMAWEB_ORTIE_BIN")
            .ok()
            .or_else(|| which::which("ortie").ok().map(|p| p.display().to_string()));

        let runner = CliRunner::new(Duration::from_secs(60));
        let himalaya = HimalayaClient::new(himalaya_bin.clone(), runner.clone());
        let himalaya_available =
            which::which(&himalaya_bin).is_ok() || PathBuf::from(&himalaya_bin).exists();

        let cardamum_available = cardamum_bin
            .as_ref()
            .map(|b| which::which(b).is_ok() || PathBuf::from(b).exists())
            .unwrap_or(false);
        let calendula_available = calendula_bin
            .as_ref()
            .map(|b| which::which(b).is_ok() || PathBuf::from(b).exists())
            .unwrap_or(false);
        let neverest_available = neverest_bin
            .as_ref()
            .map(|b| which::which(b).is_ok() || PathBuf::from(b).exists())
            .unwrap_or(false);
        let mirador_available = mirador_bin
            .as_ref()
            .map(|b| which::which(b).is_ok() || PathBuf::from(b).exists())
            .unwrap_or(false);
        let ortie_available = ortie_bin
            .as_ref()
            .map(|b| which::which(b).is_ok() || PathBuf::from(b).exists())
            .unwrap_or(false);

        let cardamum = cardamum_bin.map(|b| CardamumClient::new(b, runner.clone()));
        let calendula = calendula_bin.map(|b| CalendulaClient::new(b, runner.clone()));
        let neverest = neverest_bin.map(|b| NeverestClient::new(b, runner.clone()));
        let mirador = mirador_bin.map(|b| MiradorClient::new(b, runner.clone()));
        let ortie = ortie_bin.map(|b| OrtieClient::new(b, runner.clone()));

        let cache_path = cache_db_path()?;
        let cache = Cache::open(&cache_path).map_err(|e| format!("cache SQLite: {e}"))?;
        let prefs = Prefs::load().normalize();
        let _ = crate::config_fix::migrate_cardamum_config();
        let _ = crate::config_fix::migrate_calendula_config();

        if !himalaya_available {
            tracing::warn!(
                "himalaya introuvable ({himalaya_bin}) — mode hors-ligne / page d'état"
            );
        }
        if !prefs::himalaya_config_exists() {
            tracing::warn!(
                "config Himalaya absente ({}) — ouvrez /settings",
                prefs::himalaya_config_path().display()
            );
        }
        if !cardamum_available {
            tracing::info!("cardamum absent — autocomplete contacts désactivé");
        }
        if !calendula_available {
            tracing::info!("calendula absent — onglet calendrier dégradé");
        }

        Ok(Self {
            himalaya,
            cardamum,
            calendula,
            neverest,
            mirador,
            ortie,
            cache: Arc::new(Mutex::new(cache)),
            prefs: Arc::new(Mutex::new(prefs)),
            cli_limit: Arc::new(Semaphore::new(6)),
            himalaya_available,
            cardamum_available,
            calendula_available,
            neverest_available,
            mirador_available,
            ortie_available,
        })
    }

    /// Compte Himalaya pour les appels CLI (jamais `__all__`).
    pub async fn account(&self) -> Option<String> {
        self.prefs
            .lock()
            .await
            .selected_account()
            .map(str::to_string)
    }


    pub async fn theme_layout(&self) -> (String, String) {
        let p = self.prefs.lock().await;
        (p.theme.clone(), p.layout.clone())
    }

    pub async fn topbar_mode(&self) -> String {
        self.prefs.lock().await.topbar_mode.clone()
    }

    pub async fn ui_style(&self) -> String {
        self.prefs.lock().await.ui_style_attr()
    }
}

fn cache_db_path() -> Result<PathBuf, String> {
    let base = dirs::data_local_dir().ok_or("impossible de résoudre LOCALAPPDATA")?;
    let dir = base.join("HimaWeb");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("cache.db"))
}
