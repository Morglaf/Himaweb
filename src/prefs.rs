use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const ACCOUNT_ALL: &str = "__all__";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefs {
    pub theme: String,
    pub layout: String,
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
    /// Dossier de destination par défaut au déplacement (`compte` → nom dossier)
    #[serde(default)]
    pub default_move: std::collections::BTreeMap<String, String>,
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
}

fn default_true() -> bool {
    true
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

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            layout: "classic".into(),
            account: None,
            account_order: vec![],
            pinned_folders: vec![],
            hidden_folders: vec![],
            watched_folders: vec![],
            notifications: true,
            account_colors: Default::default(),
            account_labels: Default::default(),
            account_icons: Default::default(),
            merged_inbox: true,
            default_move: Default::default(),
            ui_font_scale: default_font_scale(),
            ui_radius: default_radius(),
            ui_space: default_font_scale(),
            ui_rail: default_rail(),
            ui_list: default_list(),
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
