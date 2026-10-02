use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const ACCOUNT_ALL: &str = "__all__";
pub const NTFY_MERGED: &str = "__ntfy__";
pub const NTFY_KEY_PREFIX: &str = "__ntfy__:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NtfySource {
    pub id: String,
    #[serde(default = "default_ntfy_server")]
    pub server: String,
    pub topic: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefs {
    pub theme: String,
    pub layout: String,
    /// Affichage barre du haut : `icon-text` | `icon` | `text`
    #[serde(default = "default_topbar_mode")]
    pub topbar_mode: String,
    /// Affichage barre d’outils compose : `icon-text` | `icon` | `text`
    #[serde(default = "default_topbar_mode")]
    pub compose_toolbar_mode: String,
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
    /// Demander confirmation avant suppression de messages
    #[serde(default = "default_true")]
    pub confirm_delete: bool,
    /// Ouvrir le navigateur au démarrage de HimaWeb
    #[serde(default = "default_true")]
    pub open_browser_on_start: bool,
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
    /// Notifications NTFY (legacy mono-topic — migré vers `ntfy_sources`)
    #[serde(default)]
    pub ntfy_enabled: bool,
    #[serde(default = "default_ntfy_server")]
    pub ntfy_server: String,
    #[serde(default)]
    pub ntfy_topic: String,
    /// Sources NTFY (plusieurs topics)
    #[serde(default)]
    pub ntfy_sources: Vec<NtfySource>,
    /// Une seule entrée « Ntfy » dans l’ordre / la barre, sinon une par source
    #[serde(default = "default_true")]
    pub ntfy_merged: bool,
    /// IDs de notifications NTFY marquées lues (`sourceId::msgId`)
    #[serde(default)]
    pub ntfy_read_ids: Vec<String>,
    /// IDs de notifications NTFY masquées localement (pas de delete serveur)
    #[serde(default)]
    pub ntfy_deleted_ids: Vec<String>,
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
    /// Ollama : raisonnement — `default` | `off` | `low` | `medium` | `high`
    #[serde(default = "default_ai_ollama_think")]
    pub ai_ollama_think: String,
    /// Ollama : température (None = défaut modèle)
    #[serde(default)]
    pub ai_ollama_temperature: Option<f32>,
    /// Préprompt global (style / persona) — appliqué à tous les providers
    #[serde(default)]
    pub ai_ollama_preprompt: String,
    /// Préprompt IA par compte mail (propriétaire + style)
    #[serde(default)]
    pub account_ai_preprompt: std::collections::BTreeMap<String, String>,
    /// Journal request/response IA (opt-in)
    #[serde(default)]
    pub ai_log_enabled: bool,
    /// Adresse domicile (contexte calendrier / trajet)
    #[serde(default)]
    pub home_address: String,
    /// Fournisseur cartes : `google` | `osm` | `apple`
    #[serde(default = "default_maps_provider")]
    pub maps_provider: String,
    /// Préprompt calendrier (complète le global)
    #[serde(default)]
    pub ai_calendar_preprompt: String,
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
    2
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

fn default_ai_ollama_think() -> String {
    "default".into()
}

fn default_maps_provider() -> String {
    "google".into()
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
            compose_toolbar_mode: default_topbar_mode(),
            account: None,
            account_order: vec![],
            pinned_folders: vec![],
            hidden_folders: vec![],
            watched_folders: vec![],
            badge_only_folders: vec![],
            notifications: true,
            confirm_delete: true,
            open_browser_on_start: true,
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
            ntfy_sources: vec![],
            ntfy_merged: true,
            ntfy_read_ids: vec![],
            ntfy_deleted_ids: vec![],
            ai_enabled: false,
            ai_provider: default_ai_provider(),
            ai_endpoint: default_ai_endpoint(),
            ai_model: default_ai_model(),
            ai_remote_endpoint: String::new(),
            ai_api_key: String::new(),
            ai_ollama_think: default_ai_ollama_think(),
            ai_ollama_temperature: None,
            ai_ollama_preprompt: String::new(),
            account_ai_preprompt: Default::default(),
            ai_log_enabled: false,
            home_address: String::new(),
            maps_provider: default_maps_provider(),
            ai_calendar_preprompt: String::new(),
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
        if self.compose_toolbar_mode != "icon-text"
            && self.compose_toolbar_mode != "icon"
            && self.compose_toolbar_mode != "text"
        {
            self.compose_toolbar_mode = default_topbar_mode();
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
        self.migrate_ntfy_sources();
        self
    }

    fn migrate_ntfy_sources(&mut self) {
        if self.ntfy_sources.is_empty() && !self.ntfy_topic.trim().is_empty() {
            let topic = self.ntfy_topic.trim().to_string();
            let id = make_ntfy_id(&topic);
            self.ntfy_sources.push(NtfySource {
                id,
                server: if self.ntfy_server.trim().is_empty() {
                    default_ntfy_server()
                } else {
                    self.ntfy_server.trim_end_matches('/').to_string()
                },
                topic,
                enabled: self.ntfy_enabled,
            });
        }
        // IDs uniques + serveurs nettoyés
        let mut seen = std::collections::HashSet::new();
        for s in &mut self.ntfy_sources {
            s.server = s.server.trim().trim_end_matches('/').to_string();
            if s.server.is_empty() {
                s.server = default_ntfy_server();
            }
            s.topic = s.topic.trim().to_string();
            if s.id.trim().is_empty() {
                s.id = make_ntfy_id(&s.topic);
            }
            let mut id = s.id.clone();
            let mut n = 2u32;
            while !seen.insert(id.clone()) {
                id = format!("{}-{n}", s.id);
                n += 1;
            }
            s.id = id;
        }
        self.ntfy_sources.retain(|s| !s.topic.is_empty());
        // Sync legacy pour ancien code / affichage plugins
        self.ntfy_enabled = self.ntfy_sources.iter().any(|s| s.enabled);
        if let Some(first) = self.ntfy_sources.iter().find(|s| s.enabled).or(self.ntfy_sources.first())
        {
            self.ntfy_server = first.server.clone();
            self.ntfy_topic = first.topic.clone();
        } else if self.ntfy_sources.is_empty() {
            self.ntfy_topic.clear();
        }
    }

    pub fn is_ntfy_key(key: &str) -> bool {
        key == NTFY_MERGED || key.starts_with(NTFY_KEY_PREFIX)
    }

    pub fn ntfy_source_key(id: &str) -> String {
        format!("{NTFY_KEY_PREFIX}{id}")
    }

    pub fn ntfy_order_keys(&self) -> Vec<String> {
        let enabled: Vec<&NtfySource> = self
            .ntfy_sources
            .iter()
            .filter(|s| s.enabled && !s.topic.is_empty())
            .collect();
        if enabled.is_empty() {
            return vec![];
        }
        if self.ntfy_merged {
            vec![NTFY_MERGED.into()]
        } else {
            enabled
                .into_iter()
                .map(|s| Self::ntfy_source_key(&s.id))
                .collect()
        }
    }

    pub fn ntfy_sources_for_key(&self, key: &str) -> Vec<&NtfySource> {
        if key == NTFY_MERGED {
            self.ntfy_sources
                .iter()
                .filter(|s| s.enabled && !s.topic.is_empty())
                .collect()
        } else if let Some(id) = key.strip_prefix(NTFY_KEY_PREFIX) {
            self.ntfy_sources
                .iter()
                .filter(|s| s.id == id && s.enabled && !s.topic.is_empty())
                .collect()
        } else {
            vec![]
        }
    }

    pub fn is_ntfy_read(&self, composite_id: &str) -> bool {
        self.ntfy_read_ids.iter().any(|id| id == composite_id)
    }

    pub fn set_ntfy_read(&mut self, composite_id: &str, read: bool) {
        if read {
            if !self.ntfy_read_ids.iter().any(|id| id == composite_id) {
                self.ntfy_read_ids.push(composite_id.to_string());
            }
            const MAX: usize = 1500;
            if self.ntfy_read_ids.len() > MAX {
                let drop = self.ntfy_read_ids.len() - MAX;
                self.ntfy_read_ids.drain(0..drop);
            }
        } else {
            self.ntfy_read_ids.retain(|id| id != composite_id);
        }
    }

    pub fn is_ntfy_deleted(&self, composite_id: &str) -> bool {
        self.ntfy_deleted_ids.iter().any(|id| id == composite_id)
    }

    pub fn set_ntfy_deleted(&mut self, composite_id: &str, deleted: bool) {
        if deleted {
            if !self.ntfy_deleted_ids.iter().any(|id| id == composite_id) {
                self.ntfy_deleted_ids.push(composite_id.to_string());
            }
            // Une notif supprimée est aussi « lue »
            self.set_ntfy_read(composite_id, true);
            const MAX: usize = 1500;
            if self.ntfy_deleted_ids.len() > MAX {
                let drop = self.ntfy_deleted_ids.len() - MAX;
                self.ntfy_deleted_ids.drain(0..drop);
            }
        } else {
            self.ntfy_deleted_ids.retain(|id| id != composite_id);
        }
    }

    /// Clé de préférence dossier pour une entrée NTFY virtuelle.
    pub fn ntfy_folder_key(mailbox_key: &str) -> String {
        Self::folder_key(Some(NTFY_MERGED), mailbox_key)
    }

    /// Ordre rail / liste : comptes mail + entrées NTFY (fusionnées ou individuelles).
    pub fn rail_order(&self, known_mail: &[String]) -> Vec<String> {
        let ntfy = self.ntfy_order_keys();
        let mut known: Vec<String> = known_mail.to_vec();
        for k in &ntfy {
            if !known.contains(k) {
                known.push(k.clone());
            }
        }
        let mut out = Vec::new();
        for a in &self.account_order {
            if known.iter().any(|k| k == a) && !out.contains(a) {
                out.push(a.clone());
            }
        }
        for a in &known {
            if !out.contains(a) {
                out.push(a.clone());
            }
        }
        out
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
            Some(a) if Self::is_ntfy_key(a) => None,
            Some(a) => Some(a),
        }
    }

    /// Compte actif = une boîte NTFY (virtuelle).
    pub fn selected_ntfy_key(&self) -> Option<&str> {
        self.account.as_deref().filter(|a| Self::is_ntfy_key(a))
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
            .unwrap_or_else(|| {
                if Self::is_ntfy_key(name) {
                    "#0ea5e9".into()
                } else {
                    crate::account_colors::default_color_for(name)
                }
            })
    }

    pub fn account_label(&self, name: &str) -> String {
        if let Some(label) = self
            .account_labels
            .get(name)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            return label.to_string();
        }
        if name == NTFY_MERGED {
            return "Ntfy".into();
        }
        if let Some(id) = name.strip_prefix(NTFY_KEY_PREFIX) {
            if let Some(s) = self.ntfy_sources.iter().find(|s| s.id == id) {
                return format!("Ntfy · {}", s.topic);
            }
            return format!("Ntfy · {id}");
        }
        name.to_string()
    }

    pub fn account_icon(&self, name: &str) -> String {
        self.account_icons
            .get(name)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if Self::is_ntfy_key(name) {
                    "bell".into()
                } else {
                    "circle-user".into()
                }
            })
    }

    pub fn account_ai_preprompt_for(&self, name: &str) -> String {
        self.account_ai_preprompt
            .get(name)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default()
    }

    pub fn ai_log_path() -> Result<PathBuf, String> {
        let base = dirs::data_local_dir().ok_or("LOCALAPPDATA introuvable")?;
        let dir = base.join("HimaWeb");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir.join("ai-log.jsonl"))
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
            if Self::is_ntfy_key(a) {
                continue;
            }
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

fn make_ntfy_id(topic: &str) -> String {
    let mut s: String = topic
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "ntfy".into()
    } else {
        s.chars().take(32).collect()
    }
}

pub fn make_ntfy_id_pub(topic: &str) -> String {
    make_ntfy_id(topic)
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
