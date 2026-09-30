use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const ACCOUNT_ALL: &str = "__all__";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefs {
    pub theme: String,
    pub layout: String,
    /// Affichage barre du haut : `icon-text` | `icon` | `text`
    #[serde(default = "default_topbar_mode")]
    pub topbar_mode: String,
    /// None / empty = Himalaya default account; `__all__` = tous les comptes
    pub account: Option<String>,
    #[serde(default)]
    pub account_order: Vec<String>,
    #[serde(default)]
    pub pinned_folders: Vec<String>,
    #[serde(default)]
    pub hidden_folders: Vec<String>,
    /// Dossiers surveillés pour compteurs / notifications (`compte::Inbox` ou `Inbox`)
    #[serde(default)]
    pub watched_folders: Vec<String>,
    /// Sous-ensemble des dossiers surveillés : badge dossier seulement, hors total global
    #[serde(default)]
    pub badge_only_folders: Vec<String>,
    /// Notifications navigateur pour nouveaux non-lus
    #[serde(default = "default_true")]
    pub notifications: bool,
    /// Couleurs par nom de compte Himalaya (`#rrggbb`)
    #[serde(default)]
    pub account_colors: std::collections::BTreeMap<String, String>,
    /// Noms d’affichage (clé = nom Himalaya)
    #[serde(default)]
    pub account_labels: std::collections::BTreeMap<String, String>,
    /// Icônes Lucide par compte (ex. `briefcase`, `home`)
    #[serde(default)]
    pub account_icons: std::collections::BTreeMap<String, String>,
    /// En mode tous les comptes : afficher « Toutes les Inbox » en tête
    #[serde(default = "default_true")]
    pub merged_inbox: bool,
    /// Regrouper les mails par conversation (sujet / In-Reply-To)
    #[serde(default = "default_true")]
    pub conversations: bool,
    /// Panneau latéral agenda / contacts (droite mail)
    #[serde(default)]
    pub side_widget: bool,
    /// Nombre de prochains RDV dans le panneau latéral
    #[serde(default = "default_side_events")]
    pub side_widget_events: u16,
    /// Dossier de destination par défaut au déplacement (`compte` → nom dossier)
    #[serde(default)]
    pub default_move: std::collections::BTreeMap<String, String>,
    /// Comptes dont le serveur refuse UID MOVE : déplacer/supprimer via COPY + purge
    #[serde(default)]
    pub imap_copy_move: std::collections::BTreeMap<String, bool>,
    /// Échelle de police (1.0 = 100 %)
    #[serde(default = "default_font_scale")]
    pub ui_font_scale: f32,
    /// Rayon des coins (px)
    #[serde(default = "default_radius")]
    pub ui_radius: u16,
    /// Densité / marges (1.0 = normal)
    #[serde(default = "default_font_scale")]
    pub ui_space: f32,
    /// Largeur colonne dossiers (px)
    #[serde(default = "default_rail")]
    pub ui_rail: u16,
    /// Largeur colonne liste (px)
    #[serde(default = "default_list")]
    pub ui_list: u16,
    /// Watch Mirador (complète le poll)
    #[serde(default)]
    pub mirador_enabled: bool,
    /// Notifications NTFY
    #[serde(default)]
    pub ntfy_enabled: bool,
    #[serde(default = "default_ntfy_server")]
    pub ntfy_server: String,
    #[serde(default)]
    pub ntfy_topic: String,
    /// Assistant IA
    #[serde(default)]
    pub ai_enabled: bool,
    /// `ollama` | `gemini`
    #[serde(default = "default_ai_provider")]
    pub ai_provider: String,
    #[serde(default = "default_ai_endpoint")]
    pub ai_endpoint: String,
    #[serde(default = "default_ai_model")]
    pub ai_model: String,
    #[serde(default)]
    pub ai_remote_endpoint: String,
    /// Clé API Gemini (ou autre distant)
    #[serde(default)]
    pub ai_api_key: String,
    /// Apparence comptes Calendula (comme mail)
    #[serde(default)]
    pub cal_account_colors: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub cal_account_labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub cal_account_icons: std::collections::BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

fn default_topbar_mode() -> String {
    "icon-text".into()
}

fn default_font_scale() -> f32 {
    1.0
}

fn default_radius() -> u16 {
    16
}

fn default_rail() -> u16 {
    260
}

fn default_list() -> u16 {
    380
}

fn default_ntfy_server() -> String {
    "https://ntfy.sh".into()
}

fn default_ai_endpoint() -> String {
    "http://127.0.0.1:11434".into()
}

fn default_ai_model() -> String {
    "llama3.2".into()
}

fn default_ai_provider() -> String {
    "ollama".into()
}

fn default_side_events() -> u16 {
    6
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            layout: "classic".into(),
            topbar_mode: default_topbar_mode(),
            account: None,
            account_order: vec![],
            pinned_folders: vec![],
            hidden_folders: vec![],
            watched_folders: vec![],
            badge_only_folders: vec![],
            notifications: true,
            account_colors: Default::default(),
            account_labels: Default::default(),
            account_icons: Default::default(),
            merged_inbox: true,
            conversations: true,
            side_widget: false,
            side_widget_events: default_side_events(),
            default_move: Default::default(),
            imap_copy_move: Default::default(),
            ui_font_scale: default_font_scale(),
            ui_radius: default_radius(),
            ui_space: default_font_scale(),
            ui_rail: default_rail(),
            ui_list: default_list(),
            mirador_enabled: false,
            ntfy_enabled: false,
            ntfy_server: default_ntfy_server(),
            ntfy_topic: String::new(),
            ai_enabled: false,
            ai_provider: default_ai_provider(),
            ai_endpoint: default_ai_endpoint(),
            ai_model: default_ai_model(),
            ai_remote_endpoint: String::new(),
            ai_api_key: String::new(),
            cal_account_colors: Default::default(),
            cal_account_labels: Default::default(),
            cal_account_icons: Default::default(),
        }
    }
}

impl Prefs {
    pub fn path() -> Result<PathBuf, String> {
        let base = dirs::data_local_dir().ok_or("LOCALAPPDATA introuvable")?;
        let dir = base.join("HimaWeb");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir.join("prefs.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::path() else {
            return Self::default();
        };
        Self::load_from(&path).normalize()
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path()?;
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, s).map_err(|e| e.to_string())
    }

    pub fn normalize(mut self) -> Self {
        if self.theme != "dark" && self.theme != "light" {
            self.theme = "light".into();
        }
        if self.layout != "classic" && self.layout != "compact" && self.layout != "wide-read" {
            self.layout = "classic".into();
        }
        if self.topbar_mode != "icon-text"
            && self.topbar_mode != "icon"
            && self.topbar_mode != "text"
        {
            self.topbar_mode = default_topbar_mode();
        }
        self.ui_font_scale = self.ui_font_scale.clamp(0.8, 1.4);
        self.ui_space = self.ui_space.clamp(0.75, 1.4);
        self.ui_radius = self.ui_radius.clamp(0, 28);
        self.ui_rail = self.ui_rail.clamp(160, 420);
        self.ui_list = self.ui_list.clamp(240, 560);
        self.pinned_folders.sort();
        self.pinned_folders.dedup();
        self.hidden_folders.sort();
        self.hidden_folders.dedup();
        self.watched_folders.sort();
        self.watched_folders.dedup();
        self.badge_only_folders.sort();
        self.badge_only_folders.dedup();
        self.badge_only_folders
            .retain(|b| self.watched_folders.iter().any(|w| w == b));
        self.hidden_folders
            .retain(|h| !self.pinned_folders.iter().any(|p| p == h));
        self
    }

    pub fn ui_style_attr(&self) -> String {
        format!(
            "--font-scale:{:.2};--radius:{}px;--ui-space:{:.2};--rail:{}px;--list:{}px",
            self.ui_font_scale, self.ui_radius, self.ui_space, self.ui_rail, self.ui_list
        )
    }

    pub fn default_move_for(&self, account: &str) -> Option<&str> {
        self.default_move
            .get(account)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

    pub fn is_all_accounts(&self) -> bool {
        self.account.as_deref() == Some(ACCOUNT_ALL)
    }

    pub fn selected_account(&self) -> Option<&str> {
        match self.account.as_deref() {
            None | Some("") => None,
            Some(ACCOUNT_ALL) => None,
            Some(a) => Some(a),
        }
    }

    pub fn folder_key(account: Option<&str>, mailbox: &str) -> String {
        match account {
            Some(a) if !a.is_empty() && a != ACCOUNT_ALL => format!("{a}::{mailbox}"),
            _ => mailbox.to_string(),
        }
    }

    pub fn is_pinned(&self, key: &str) -> bool {
        self.pinned_folders.iter().any(|p| p == key)
    }

    pub fn is_hidden(&self, key: &str) -> bool {
        self.hidden_folders.iter().any(|h| h == key)
    }

    pub fn is_watched(&self, key: &str, mailbox: &str) -> bool {
        if self.watched_folders.is_empty() {
            // Par défaut : toutes les Inbox
            return mailbox.eq_ignore_ascii_case("inbox")
                || mailbox.to_ascii_lowercase().ends_with("/inbox");
        }
        self.watched_folders.iter().any(|w| w == key)
            || self.watched_folders.iter().any(|w| w == mailbox)
    }

    /// Badge dossier sans contribution au total global (ex. Spam).
    pub fn is_badge_only(&self, key: &str, mailbox: &str) -> bool {
        self.badge_only_folders.iter().any(|b| b == key)
            || self.badge_only_folders.iter().any(|b| b == mailbox)
    }

    pub fn contributes_to_unread_total(&self, key: &str, mailbox: &str) -> bool {
        self.is_watched(key, mailbox) && !self.is_badge_only(key, mailbox)
    }

    pub fn account_color(&self, name: &str) -> String {
        self.account_colors
            .get(name)
            .cloned()
            .unwrap_or_else(|| crate::account_colors::default_color_for(name))
    }

    pub fn account_label(&self, name: &str) -> String {
        self.account_labels
            .get(name)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| name.to_string())
    }

    pub fn account_icon(&self, name: &str) -> String {
        self.account_icons
            .get(name)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "circle-user".into())
    }

    /// Serveur sans UID MOVE : COPY + purge au lieu de `message move`.
    pub fn uses_copy_move(&self, account: &str) -> bool {
        self.imap_copy_move.get(account).copied().unwrap_or(false)
    }

    pub fn set_copy_move(&mut self, account: &str, enabled: bool) {
        if enabled {
            self.imap_copy_move.insert(account.to_string(), true);
        } else {
            self.imap_copy_move.remove(account);
        }
    }

    pub fn cal_account_color(&self, name: &str) -> String {
        self.cal_account_colors
            .get(name)
            .cloned()
            .unwrap_or_else(|| crate::account_colors::default_color_for(name))
    }

    pub fn cal_account_label(&self, name: &str) -> String {
        self.cal_account_labels
            .get(name)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| name.to_string())
    }

    pub fn cal_account_icon(&self, name: &str) -> String {
        self.cal_account_icons
            .get(name)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "calendar".into())
    }

    pub fn ordered_accounts(&self, known: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for a in &self.account_order {
            if known.iter().any(|k| k == a) && !out.contains(a) {
                out.push(a.clone());
            }
        }
        for a in known {
            if !out.contains(a) {
                out.push(a.clone());
            }
        }
        out
    }
}

pub fn himalaya_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("himalaya")
        .join("config.toml")
}

pub fn himalaya_config_exists() -> bool {
    himalaya_config_path().is_file()
}

pub fn calendula_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("calendula")
        .join("config.toml")
}

pub fn calendula_config_exists() -> bool {
    calendula_config_path().is_file()
}

pub fn cardamum_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cardamum")
        .join("config.toml")
}

pub fn cardamum_config_exists() -> bool {
    cardamum_config_path().is_file()
}
