//! Backup contenu PIM via Neverest (store pimdir) + compte Archive Himalaya.
//! Export lisible contacts/agendas (JSON + CSV) via Cardamum / Calendula.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Local};
use tokio::sync::Mutex;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::cli::calendula::CalendulaClient;
use crate::cli::cardamum::CardamumClient;
use crate::cli::neverest::NeverestClient;
use crate::prefs::{self, Prefs};

pub const BACKUP_ACCOUNT: &str = "himaweb-backup";
pub const ARCHIVE_ACCOUNT: &str = "himaweb-archive";

pub fn is_archive_account(name: Option<&str>) -> bool {
    name.map(|n| n == ARCHIVE_ACCOUNT).unwrap_or(false)
}

static BACKUP_RUNNING: AtomicBool = AtomicBool::new(false);

const MAX_LOG_LINES: usize = 120;

#[derive(Debug, Clone, Default)]
pub struct BackupStatus {
    pub running: bool,
    pub message: String,
    pub last_ok: bool,
    /// 0..=100
    pub progress: u8,
    pub logs: Vec<String>,
}

impl BackupStatus {
    pub fn reset_for_run(&mut self) {
        self.running = true;
        self.last_ok = false;
        self.progress = 0;
        self.logs.clear();
        self.message = "Sauvegarde lancée…".into();
    }

    pub fn push_log(&mut self, line: impl Into<String>) {
        let line = strip_ansi(line.into());
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.logs.push(line.to_string());
        if self.logs.len() > MAX_LOG_LINES {
            let drop_n = self.logs.len() - MAX_LOG_LINES;
            self.logs.drain(0..drop_n);
        }
    }

    pub fn set_phase(&mut self, progress: u8, message: impl Into<String>) {
        self.progress = progress.min(100);
        let message = message.into();
        self.message = message.clone();
        self.push_log(message);
    }
}

pub type BackupStatusHandle = std::sync::Arc<Mutex<BackupStatus>>;

pub fn new_status_handle() -> BackupStatusHandle {
    std::sync::Arc::new(Mutex::new(BackupStatus::default()))
}

pub fn is_running() -> bool {
    BACKUP_RUNNING.load(Ordering::SeqCst)
}

/// Remet les flags à zéro (sauvegarde coincée / annulation utilisateur).
pub async fn cancel_backup(status: &BackupStatusHandle) {
    BACKUP_RUNNING.store(false, Ordering::SeqCst);
    let mut s = status.lock().await;
    s.running = false;
    s.progress = 0;
    s.message = "Sauvegarde annulée / réinitialisée.".into();
    s.push_log("Flags réinitialisés — vous pouvez relancer.");
    s.last_ok = false;
}

fn list_source_keys(config: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(config) else {
        return vec![];
    };
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return vec![];
    };
    let Some(sources) = doc
        .get("accounts")
        .and_then(|a| a.get(BACKUP_ACCOUNT))
        .and_then(|a| a.get("sources"))
        .and_then(|s| s.as_table())
    else {
        return vec![];
    };
    sources.iter().map(|(k, _)| k.to_string()).collect()
}

pub fn neverest_backup_config_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("HimaWeb")
        .join("neverest-backup.toml")
}

fn strip_ansi(s: String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(x) = chars.next() {
                    if x.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub id: String,
    pub path: String,
    pub label: String,
}

/// Un dossier ressemble à un store pimdir Neverest.
pub fn is_pimdir_store(path: &Path) -> bool {
    path.join("pimdir.db").is_file()
        || path.join("objects").is_dir()
        || path.join("neverest").is_dir()
}

/// Liste les snapshots sous le dossier racine de backup.
pub fn list_snapshots(root: &str) -> Vec<SnapshotInfo> {
    let root = root.trim();
    if root.is_empty() {
        return vec![];
    }
    let root_path = PathBuf::from(root);
    if !root_path.is_dir() {
        return vec![];
    }
    let mut out = Vec::new();
    if is_pimdir_store(&root_path) {
        out.push(SnapshotInfo {
            id: ".".into(),
            path: root_path.display().to_string(),
            label: "Racine (ancien layout)".into(),
        });
    }
    if let Ok(rd) = std::fs::read_dir(&root_path) {
        let mut dirs: Vec<_> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        dirs.sort_by_key(|e| std::cmp::Reverse(e.metadata().and_then(|m| m.modified()).ok()));
        for e in dirs {
            let p = e.path();
            if !is_pimdir_store(&p) {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            out.push(SnapshotInfo {
                id: name.clone(),
                path: p.display().to_string(),
                label: name,
            });
        }
    }
    out
}

pub fn new_snapshot_dir(root: &str) -> Result<PathBuf, String> {
    let root = root.trim();
    if root.is_empty() {
        return Err("Choisissez un dossier de destination.".into());
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H%M%S").to_string();
    let path = PathBuf::from(root).join(&stamp);
    std::fs::create_dir_all(&path).map_err(|e| format!("création snapshot: {e}"))?;
    Ok(path)
}

/// Staging local (LOCALAPPDATA) : Neverest écrit ici, puis on publie vers le dossier choisi.
/// Réduit les « Accès refusé » quand l’AV verrouille le dossier de destination pendant le sync.
pub fn staging_snapshot_dir(stamp: &str) -> Result<PathBuf, String> {
    let path = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("HimaWeb")
        .join("backup-staging")
        .join(stamp);
    std::fs::create_dir_all(&path).map_err(|e| format!("création staging: {e}"))?;
    Ok(path)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("création {}: {e}", dst.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("lecture {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_file() {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&from, &to)
                .map_err(|e| format!("copie {} → {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Publie le store staging vers la destination finale, puis tente de supprimer le staging.
pub fn publish_snapshot(staging: &Path, final_dir: &Path) -> Result<(), String> {
    if staging == final_dir {
        return Ok(());
    }
    copy_dir_recursive(staging, final_dir)?;
    let _ = std::fs::remove_dir_all(staging);
    Ok(())
}

/// Dossier effectif à synchroniser / lire (browse ou racine).
pub fn effective_store_dir(prefs: &Prefs) -> Result<PathBuf, String> {
    let root = prefs.backup_data_dir.trim();
    if root.is_empty() {
        return Err("Choisissez un dossier de destination.".into());
    }
    let browse = prefs.backup_browse_dir.trim();
    if !browse.is_empty() {
        let p = PathBuf::from(browse);
        if p.is_dir() {
            return Ok(p);
        }
    }
    let snaps = list_snapshots(root);
    if let Some(s) = snaps.first() {
        return Ok(PathBuf::from(&s.path));
    }
    Ok(PathBuf::from(root))
}

/// Génère la config Neverest HimaWeb (sources = comptes Himalaya/Cardamum/Calendula).
pub fn write_neverest_config(prefs: &Prefs, store_path: &Path) -> Result<PathBuf, String> {
    write_neverest_config_excluding(prefs, store_path, &[])
}

fn write_neverest_config_excluding(
    prefs: &Prefs,
    store_path: &Path,
    exclude: &[String],
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(store_path).map_err(|e| format!("création dossier: {e}"))?;

    let mut doc = DocumentMut::new();
    let mut accounts = Table::new();
    let mut acc = Table::new();
    acc.insert("one-way", Item::Value(Value::from(true)));
    acc.insert("retain", Item::Value(Value::from(true)));

    let mut store_t = Table::new();
    store_t.set_implicit(false);
    store_t.insert(
        "root",
        Item::Value(Value::from(store_path.to_string_lossy().as_ref())),
    );
    acc.insert("store", Item::Table(store_t));

    let mut sources = Table::new();
    sources.set_implicit(false);
    if prefs.backup_include_mail {
        append_imap_sources(&mut sources, prefs, exclude)?;
    }
    if prefs.backup_include_contacts {
        append_dav_sources(
            &mut sources,
            &prefs::cardamum_config_path(),
            "carddav",
            exclude,
            prefs,
        )?;
    }
    if prefs.backup_include_calendars {
        append_dav_sources(
            &mut sources,
            &prefs::calendula_config_path(),
            "caldav",
            exclude,
            prefs,
        )?;
    }
    if sources.is_empty() {
        return Err(
            "Aucune source à synchroniser (cochez mail/contacts/agendas et au moins un compte)."
                .into(),
        );
    }
    acc.set_implicit(false);
    accounts.set_implicit(false);
    acc.insert("sources", Item::Table(sources));
    accounts.insert(BACKUP_ACCOUNT, Item::Table(acc));
    doc.insert("accounts", Item::Table(accounts));

    let path = neverest_backup_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let rendered = doc.to_string();
    // Garde-fou : un TOML illisible casse le skip de sources à l'init.
    rendered
        .parse::<DocumentMut>()
        .map_err(|e| format!("config Neverest invalide après génération: {e}"))?;
    std::fs::write(&path, rendered).map_err(|e| e.to_string())?;
    Ok(path)
}

fn is_excluded(exclude: &[String], key: &str) -> bool {
    exclude.iter().any(|e| e == key)
}

fn account_selected_for_backup(prefs: &Prefs, name: &str) -> bool {
    account_in_list(&prefs.backup_mail_accounts, name)
}

fn account_in_list(list: &[String], name: &str) -> bool {
    if list.is_empty() {
        return true;
    }
    list.iter().any(|a| a.eq_ignore_ascii_case(name))
}

fn account_selected_for_contacts(prefs: &Prefs, name: &str) -> bool {
    account_in_list(&prefs.backup_contact_accounts, name)
}

fn account_selected_for_calendars(prefs: &Prefs, name: &str) -> bool {
    account_in_list(&prefs.backup_calendar_accounts, name)
}

fn append_imap_sources(
    sources: &mut Table,
    prefs: &Prefs,
    exclude: &[String],
) -> Result<(), String> {
    let path = prefs::himalaya_config_path();
    if !path.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let him: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let Some(Item::Table(him_accs)) = him.get("accounts") else {
        return Ok(());
    };
    for (name, item) in him_accs.iter() {
        let Some(t) = item.as_table() else { continue };
        if name == ARCHIVE_ACCOUNT {
            continue;
        }
        if !account_selected_for_backup(prefs, name) {
            continue;
        }
        let key = sanitize_source_key(name);
        if is_excluded(exclude, &key) {
            continue;
        }
        let Some(imap) = t.get("imap").and_then(|i| i.as_table()) else {
            continue;
        };
        let Some(server) = table_str(imap, "server") else {
            continue;
        };
        let mut src = Table::new();
        src.set_implicit(false);
        let mut imap_out = Table::new();
        imap_out.set_implicit(false);
        imap_out.insert("server", Item::Value(Value::from(server.as_str())));
        if let Some(sasl) = rebuild_imap_sasl(imap) {
            imap_out.insert("sasl", Item::Table(sasl));
        }
        if let Some(tls) = imap.get("tls") {
            imap_out.insert("tls", tls.clone());
        }
        apply_backup_permissions(&mut imap_out);
        src.insert("imap", Item::Table(imap_out));
        sources.insert(&key, Item::Table(src));
    }
    Ok(())
}

fn rebuild_imap_sasl(imap: &Table) -> Option<Table> {
    let sasl = imap.get("sasl")?.as_table()?;
    let plain = sasl.get("plain")?.as_table()?;
    let username = table_str(plain, "username")?;
    let mut plain_out = Table::new();
    plain_out.set_implicit(false);
    plain_out.insert("username", Item::Value(Value::from(username.as_str())));
    if let Some(pwd) = plain.get("password") {
        plain_out.insert("password", normalize_password_item(pwd));
    }
    let mut sasl_out = Table::new();
    sasl_out.set_implicit(false);
    sasl_out.insert("plain", Item::Table(plain_out));
    Some(sasl_out)
}

fn append_dav_sources(
    sources: &mut Table,
    config_path: &Path,
    kind: &str,
    exclude: &[String],
    prefs: &Prefs,
) -> Result<(), String> {
    if !config_path.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(config_path).map_err(|e| e.to_string())?;
    let doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let Some(Item::Table(accs)) = doc.get("accounts") else {
        return Ok(());
    };
    for (name, item) in accs.iter() {
        let Some(t) = item.as_table() else { continue };
        let allowed = if kind == "carddav" {
            account_selected_for_contacts(prefs, name)
        } else {
            account_selected_for_calendars(prefs, name)
        };
        if !allowed {
            continue;
        }
        let dav = t
            .get(kind)
            .and_then(|i| i.as_table())
            .or_else(|| t.get("backend").and_then(|i| i.as_table()));
        let Some(dav) = dav else { continue };
        let Some(dav_out) = sanitize_dav_table(dav) else {
            continue;
        };
        let key = format!("{kind}-{}", sanitize_source_key(name));
        if is_excluded(exclude, &key) {
            continue;
        }
        let mut src = Table::new();
        src.set_implicit(false);
        src.insert(kind, Item::Table(dav_out));
        sources.insert(&key, Item::Table(src));
    }
    Ok(())
}

/// Mappe Cardamum/Calendula → Neverest.
/// Neverest n'a pas de clé `home` : on met l'URL utile dans `server`.
/// Préférer `home` (chemin SOGo complet) à `server` hôte seul — sinon découverte 405.
/// Auth reconstruite en tables explicites (évite dotted keys → TOML `duplicate key`).
fn sanitize_dav_table(dav: &Table) -> Option<Table> {
    let server = table_str(dav, "home")
        .or_else(|| table_str(dav, "url"))
        .or_else(|| table_str(dav, "server"))?;
    let mut out = Table::new();
    out.set_implicit(false);
    out.insert("server", Item::Value(Value::from(server.as_str())));
    if let Some(auth) = rebuild_dav_auth(dav) {
        out.insert("auth", Item::Table(auth));
    }
    if let Some(tls) = dav.get("tls") {
        out.insert("tls", tls.clone());
    }
    apply_backup_permissions(&mut out);
    Some(out)
}

fn rebuild_dav_auth(dav: &Table) -> Option<Table> {
    let auth = dav.get("auth")?.as_table()?;
    let basic = auth.get("basic")?.as_table()?;
    let username = table_str(basic, "username")?;

    let mut basic_out = Table::new();
    basic_out.set_implicit(false);
    basic_out.insert("username", Item::Value(Value::from(username.as_str())));

    if let Some(pwd_item) = basic.get("password") {
        basic_out.insert("password", normalize_password_item(pwd_item));
    }

    let mut auth_out = Table::new();
    auth_out.set_implicit(false);
    auth_out.insert("basic", Item::Table(basic_out));
    Some(auth_out)
}

fn normalize_password_item(pwd: &Item) -> Item {
    if let Item::Value(Value::String(s)) = pwd {
        let mut t = Table::new();
        t.set_implicit(false);
        t.insert("raw", Item::Value(Value::from(s.value())));
        return Item::Table(t);
    }
    let Some(src) = pwd.as_table() else {
        return pwd.clone();
    };
    let mut out = Table::new();
    out.set_implicit(false);
    for key in ["raw", "cmd", "keyring"] {
        if let Some(v) = src.get(key) {
            out.insert(key, v.clone());
        }
    }
    if out.is_empty() {
        pwd.clone()
    } else {
        Item::Table(out)
    }
}

/// Extrait la clé source depuis `Error: Initialize carddav-Foo`.
fn parse_init_failed_source(msg: &str) -> Option<String> {
    for line in msg.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Error: Initialize ") {
            let key = rest.split_whitespace().next().unwrap_or("").trim();
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
        if let Some(idx) = t.find("Initialize ") {
            let rest = &t[idx + "Initialize ".len()..];
            let key = rest
                .split(|c: char| c.is_whitespace() || c == ':' || c == ',')
                .next()
                .unwrap_or("")
                .trim();
            if !key.is_empty() && key != "account" {
                return Some(key.to_string());
            }
        }
    }
    None
}

/// Recipe backup Neverest : `item|collection.{create,delete}` (les deux obligatoires).
fn apply_backup_permissions(backend: &mut Table) {
    let mut item = Table::new();
    item.set_implicit(false);
    item.insert("create", Item::Value(Value::from(true)));
    item.insert("delete", Item::Value(Value::from(false)));
    backend.insert("item", Item::Table(item));
    let mut collection = Table::new();
    collection.set_implicit(false);
    collection.insert("create", Item::Value(Value::from(true)));
    collection.insert("delete", Item::Value(Value::from(false)));
    backend.insert("collection", Item::Table(collection));
}

fn table_str(t: &Table, key: &str) -> Option<String> {
    t.get(key).and_then(|i| i.as_str()).map(str::to_string)
}

fn sanitize_source_key(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Config Himalaya **séparée** pour la lecture archive (ne touche jamais config.toml).
pub fn archive_himalaya_config_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("HimaWeb")
        .join("himaweb-archive.toml")
}

/// Prépare la config sidecar lecture seule (pimdir). N'écrit pas dans le config Himalaya principal.
/// `inbox_alias` : id pimdir complet ex. `compte/INBOX`.
pub fn ensure_archive_himalaya_account(
    store_dir: &str,
    inbox_alias: Option<&str>,
) -> Result<(), String> {
    // Nettoie un éventuel résidu d'anciennes versions dans la config principale.
    let _ = strip_archive_from_main_himalaya_config();

    let path = archive_himalaya_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut doc = DocumentMut::new();
    let mut accounts = Table::new();
    let mut acc = Table::new();
    let mut pim = Table::new();
    pim.insert("root", Item::Value(Value::from(store_dir)));
    pim.insert("account", Item::Value(Value::from(BACKUP_ACCOUNT)));
    acc.insert("pimdir", Item::Table(pim));
    if let Some(inbox) = inbox_alias.map(str::trim).filter(|s| !s.is_empty()) {
        let mut alias = Table::new();
        alias.insert("inbox", Item::Value(Value::from(inbox)));
        let mut mailbox = Table::new();
        mailbox.insert("alias", Item::Table(alias));
        acc.insert("mailbox", Item::Table(mailbox));
    }
    accounts.insert(ARCHIVE_ACCOUNT, Item::Table(acc));
    doc.insert("accounts", Item::Table(accounts));
    std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Retire le compte archive de la config Himalaya **principale** (legacy).
pub fn strip_archive_from_main_himalaya_config() -> Result<(), String> {
    let path = prefs::himalaya_config_path();
    if !path.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let removed = if let Some(accounts) = doc.get_mut("accounts").and_then(|i| i.as_table_mut()) {
        accounts.remove(ARCHIVE_ACCOUNT).is_some()
    } else {
        false
    };
    if removed {
        std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Sortie mode archive : retire le sidecar + tout résidu dans la config principale.
pub fn remove_archive_himalaya_account() -> Result<(), String> {
    let side = archive_himalaya_config_path();
    if side.is_file() {
        let _ = std::fs::remove_file(&side);
    }
    strip_archive_from_main_himalaya_config()
}

/// Choisit un mailbox pimdir par défaut (`…/INBOX` ou premier de la liste).
pub fn pick_default_pimdir_mailbox(names: &[String]) -> Option<String> {
    names
        .iter()
        .find(|n| {
            let l = n.to_ascii_lowercase();
            l == "inbox" || l.ends_with("/inbox")
        })
        .cloned()
        .or_else(|| names.first().cloned())
}

/// Résout un nom court (`archives`, `Inbox`) vers l'id pimdir complet (`source/archives`).
pub fn resolve_pimdir_mailbox(requested: &str, available: &[String]) -> String {
    let req = requested.trim();
    if req.is_empty() {
        return pick_default_pimdir_mailbox(available).unwrap_or_else(|| "Inbox".into());
    }
    if let Some(exact) = available.iter().find(|n| n.eq_ignore_ascii_case(req)) {
        return exact.clone();
    }
    if req.eq_ignore_ascii_case("inbox") {
        if let Some(picked) = pick_default_pimdir_mailbox(available) {
            return picked;
        }
    }
    let req_l = req.to_ascii_lowercase();
    let mut matches: Vec<&String> = available
        .iter()
        .filter(|n| {
            let last = n.rsplit('/').next().unwrap_or(n.as_str());
            last.eq_ignore_ascii_case(req)
                || n.to_ascii_lowercase().ends_with(&format!("/{req_l}"))
        })
        .collect();
    // Préférer le match le plus court (ex. `…/archives` avant un faux positif improbable).
    matches.sort_by_key(|n| n.len());
    if let Some(m) = matches.first() {
        return (*m).clone();
    }
    req.to_string()
}

/// Profondeur d'affichage pimdir : ignore le préfixe `source/`.
pub fn pimdir_tree_depth(name: &str) -> u8 {
    match name.split_once('/') {
        Some((_, rest)) => rest.matches('/').count().min(8) as u8,
        None => 0,
    }
}

/// Parent d'arbre pimdir : pas de nœud fantôme pour le seul préfixe source.
pub fn pimdir_tree_parent(name: &str) -> String {
    match name.rsplit_once('/') {
        Some((p, _)) if p.contains('/') => p.to_string(),
        _ => String::new(),
    }
}

pub fn open_folder(path: &str) -> Result<(), String> {
    let p = PathBuf::from(path.trim());
    if !p.exists() {
        return Err(format!("dossier introuvable: {}", p.display()));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(p.as_os_str())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Export lisible (JSON + CSV) via Cardamum/Calendula — un fichier par compte.
pub async fn export_readable_pim(
    dest: &Path,
    prefs: &Prefs,
    cardamum: Option<&CardamumClient>,
    calendula: Option<&CalendulaClient>,
    status: &BackupStatusHandle,
) -> Result<String, String> {
    let export_root = dest.join("export");
    std::fs::create_dir_all(&export_root).map_err(|e| format!("export/: {e}"))?;
    let mut parts: Vec<String> = Vec::new();

    if prefs.backup_include_contacts {
        if let Some(client) = cardamum {
            {
                let mut s = status.lock().await;
                s.push_log("Export contacts (Cardamum → JSON/CSV par compte)…");
            }
            match export_contacts_per_account(client, prefs, &export_root).await {
                Ok((n_acc, n_cards)) => {
                    parts.push(format!("{n_cards} contacts / {n_acc} compte(s)"));
                    let mut s = status.lock().await;
                    s.push_log(format!(
                        "Export contacts OK → {}/contacts/ ({n_cards} fiches, {n_acc} compte(s))",
                        export_root.display()
                    ));
                }
                Err(e) => {
                    let mut s = status.lock().await;
                    s.push_log(format!("Export contacts échoué : {e}"));
                }
            }
        } else {
            let mut s = status.lock().await;
            s.push_log("Export contacts ignoré (Cardamum absent).");
        }
    }

    if prefs.backup_include_calendars {
        if let Some(client) = calendula {
            {
                let mut s = status.lock().await;
                s.push_log("Export agendas (Calendula → JSON/CSV par compte)…");
            }
            match export_calendars_per_account(client, prefs, &export_root).await {
                Ok((n_acc, n_ev)) => {
                    parts.push(format!("{n_ev} événements / {n_acc} compte(s)"));
                    let mut s = status.lock().await;
                    s.push_log(format!(
                        "Export agendas OK → {}/calendars/ ({n_ev} événements, {n_acc} compte(s))",
                        export_root.display()
                    ));
                }
                Err(e) => {
                    let mut s = status.lock().await;
                    s.push_log(format!("Export agendas échoué : {e}"));
                }
            }
        } else {
            let mut s = status.lock().await;
            s.push_log("Export agendas ignoré (Calendula absent).");
        }
    }

    if parts.is_empty() {
        Ok("aucun export lisible".into())
    } else {
        Ok(parts.join(", "))
    }
}

fn safe_export_filename(name: &str) -> String {
    let s = sanitize_source_key(name);
    if s.is_empty() {
        "account".into()
    } else {
        s
    }
}

async fn export_contacts_per_account(
    client: &CardamumClient,
    prefs: &Prefs,
    export_root: &Path,
) -> Result<(usize, usize), String> {
    let dir = export_root.join("contacts");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let all = client
        .list_contacts()
        .await
        .map_err(|e| e.to_string())?;
    use std::collections::BTreeMap;
    let mut by_account: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for c in all {
        if !account_selected_for_contacts(prefs, &c.account) {
            continue;
        }
        by_account.entry(c.account.clone()).or_default().push(c);
    }
    let mut n_cards = 0usize;
    for (acc, list) in &by_account {
        let stem = safe_export_filename(acc);
        let json_path = dir.join(format!("{stem}.json"));
        let csv_path = dir.join(format!("{stem}.csv"));
        let json =
            serde_json::to_string_pretty(list).map_err(|e| format!("{stem}.json: {e}"))?;
        std::fs::write(&json_path, json).map_err(|e| e.to_string())?;
        let mut csv = String::from("account,addressbook,id,name,email,tel\n");
        for c in list {
            csv.push_str(&format!(
                "{},{},{},{},{},{}\n",
                csv_escape(&c.account),
                csv_escape(&c.addressbook),
                csv_escape(&c.id),
                csv_escape(&c.name),
                csv_escape(&c.email),
                csv_escape(&c.tel),
            ));
        }
        std::fs::write(&csv_path, csv).map_err(|e| e.to_string())?;
        n_cards += list.len();
    }
    Ok((by_account.len(), n_cards))
}

async fn export_calendars_per_account(
    client: &CalendulaClient,
    prefs: &Prefs,
    export_root: &Path,
) -> Result<(usize, usize), String> {
    let dir = export_root.join("calendars");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let calendars = client
        .list_calendars()
        .await
        .map_err(|e| e.to_string())?;
    let now = Local::now().date_naive();
    let from = (now - ChronoDuration::days(365 * 2))
        .format("%Y-%m-%d")
        .to_string();
    let to = (now + ChronoDuration::days(365 * 2))
        .format("%Y-%m-%d")
        .to_string();

    #[derive(serde::Serialize)]
    struct EventExport {
        account: String,
        calendar: String,
        calendar_id: String,
        id: String,
        summary: String,
        start: String,
        end: String,
        location: String,
        description: String,
        rrule: String,
    }

    use std::collections::BTreeMap;
    let mut by_account: BTreeMap<String, Vec<EventExport>> = BTreeMap::new();
    for cal in &calendars {
        if !account_selected_for_calendars(prefs, &cal.account) {
            continue;
        }
        let events = client
            .list_events(&cal.id, Some(&from), Some(&to))
            .await
            .unwrap_or_default();
        let bucket = by_account.entry(cal.account.clone()).or_default();
        for ev in events {
            bucket.push(EventExport {
                account: cal.account.clone(),
                calendar: cal.name.clone(),
                calendar_id: cal.id.clone(),
                id: ev.id,
                summary: ev.summary,
                start: ev.date,
                end: ev.end,
                location: ev.location,
                description: ev.description,
                rrule: ev.rrule,
            });
        }
    }

    let mut n_ev = 0usize;
    for (acc, rows) in &by_account {
        let stem = safe_export_filename(acc);
        let json_path = dir.join(format!("{stem}.json"));
        let csv_path = dir.join(format!("{stem}.csv"));
        let json =
            serde_json::to_string_pretty(rows).map_err(|e| format!("{stem}.json: {e}"))?;
        std::fs::write(&json_path, json).map_err(|e| e.to_string())?;
        let mut csv = String::from(
            "account,calendar,calendar_id,id,summary,start,end,location,description,rrule\n",
        );
        for r in rows {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{}\n",
                csv_escape(&r.account),
                csv_escape(&r.calendar),
                csv_escape(&r.calendar_id),
                csv_escape(&r.id),
                csv_escape(&r.summary),
                csv_escape(&r.start),
                csv_escape(&r.end),
                csv_escape(&r.location),
                csv_escape(&r.description),
                csv_escape(&r.rrule),
            ));
        }
        std::fs::write(&csv_path, csv).map_err(|e| e.to_string())?;
        n_ev += rows.len();
    }
    Ok((by_account.len(), n_ev))
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Lance init + sync Neverest (timeout long). Met à jour `status` (logs + progression).
/// Retourne `(message, chemin du snapshot)`.
pub async fn run_backup(
    client: &NeverestClient,
    prefs: &Prefs,
    status: &BackupStatusHandle,
    cardamum: Option<&CardamumClient>,
    calendula: Option<&CalendulaClient>,
) -> Result<(String, PathBuf), String> {
    if BACKUP_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // Désync éventuel : flag atomique true mais statut arrêté → on reprend.
        let mut s = status.lock().await;
        if !s.running {
            BACKUP_RUNNING.store(false, Ordering::SeqCst);
            drop(s);
            if BACKUP_RUNNING
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                let mut s = status.lock().await;
                s.set_phase(0, "Une sauvegarde est déjà en cours.");
                return Err(s.message.clone());
            }
        } else {
            s.push_log("Relance refusée : une sauvegarde tourne déjà.");
            s.message = "Une sauvegarde est déjà en cours (utilisez Réinitialiser si coincé).".into();
            return Err(s.message.clone());
        }
    }
    {
        let mut s = status.lock().await;
        s.reset_for_run();
        s.set_phase(2, "Création du snapshot daté…");
    }

    let result = async {
        let final_path = new_snapshot_dir(prefs.backup_data_dir.trim())?;
        let stamp = final_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("snap")
            .to_string();
        // Sync dans LOCALAPPDATA puis copie → destination (contourne souvent l’AV mid-write).
        let store_path = staging_snapshot_dir(&stamp)?;
        {
            let mut s = status.lock().await;
            s.set_phase(
                5,
                format!(
                    "Snapshot : {} (staging {})",
                    final_path.display(),
                    store_path.display()
                ),
            );
            s.set_phase(8, "Préparation de la config Neverest…");
        }
        let config = write_neverest_config(prefs, &store_path)?;
        {
            let mut s = status.lock().await;
            s.set_phase(9, format!("Config écrite : {}", config.display()));
            s.set_phase(10, format!("Init Neverest ({BACKUP_ACCOUNT})…"));
        }

        let mut skipped_sources: Vec<String> = Vec::new();
        // Une source CardDAV/CalDAV HS ne doit pas faire échouer tout le backup.
        loop {
            match client
                .run_config_cmd_logged(
                    &config,
                    &["init", "-a", BACKUP_ACCOUNT],
                    Duration::from_secs(180),
                    status,
                )
                .await
            {
                Ok(out) => {
                    let mut s = status.lock().await;
                    if !out.trim().is_empty() {
                        s.push_log(out.chars().take(400).collect::<String>());
                    }
                    s.set_phase(15, "Init OK (ou replica déjà présente).");
                    break;
                }
                Err(e) => {
                    let msg = e.to_string();
                    let already = msg.to_ascii_lowercase();
                    {
                        let mut s = status.lock().await;
                        s.push_log(format!("init: {msg}"));
                    }
                    if already.contains("already")
                        || already.contains("exists")
                        || already.contains("refuse")
                    {
                        let mut s = status.lock().await;
                        s.set_phase(15, "Replica déjà initialisée — sync…");
                        break;
                    }
                    if let Some(bad) = parse_init_failed_source(&msg) {
                        if skipped_sources.iter().any(|s| s == &bad) {
                            return Err(format!(
                                "init Neverest: source {bad} échoue encore après exclusion: {msg}"
                            ));
                        }
                        skipped_sources.push(bad.clone());
                        {
                            let mut s = status.lock().await;
                            s.push_log(format!(
                                "Source ignorée (init KO) : {bad} — régénération config…"
                            ));
                        }
                        write_neverest_config_excluding(prefs, &store_path, &skipped_sources)?;
                        continue;
                    }
                    return Err(format!("init Neverest: {msg}"));
                }
            }
        }

        let sources = list_source_keys(&config);
        {
            let mut s = status.lock().await;
            s.push_log(format!("{} source(s) : {}", sources.len(), sources.join(", ")));
            if !skipped_sources.is_empty() {
                s.push_log(format!(
                    "Ignorée(s) à l’init : {}",
                    skipped_sources.join(", ")
                ));
            }
        }
        if sources.is_empty() {
            return Err("Aucune source Neverest valide après init.".into());
        }

        let n = sources.len().max(1);
        let mut last_out = String::new();
        let mut sync_ok: usize = 0;
        for (i, src) in sources.iter().enumerate() {
            let pct = 15 + ((i as u32 * 80) / n as u32) as u8;
            {
                let mut s = status.lock().await;
                s.set_phase(
                    pct,
                    format!("Sync « {src} » ({}/{n})…", i + 1),
                );
            }
            match client
                .run_config_cmd_logged(
                    &config,
                    &["sync", "-a", BACKUP_ACCOUNT, "-s", src],
                    Duration::from_secs(60 * 20),
                    status,
                )
                .await
            {
                Ok(out) => {
                    last_out = out;
                    sync_ok += 1;
                    let mut s = status.lock().await;
                    s.push_log(format!("OK « {src} »"));
                }
                Err(e) => {
                    skipped_sources.push(src.clone());
                    let mut s = status.lock().await;
                    s.push_log(format!("Échec sync « {src} » (on continue) : {e}"));
                }
            }
        }

        {
            let mut s = status.lock().await;
            s.set_phase(90, "Export lisible contacts/agendas (JSON/CSV)…");
        }
        // Écrit dans le staging pour être copié avec le snapshot.
        let export_note = match export_readable_pim(
            &store_path,
            prefs,
            cardamum,
            calendula,
            status,
        )
        .await
        {
            Ok(n) => n,
            Err(e) => {
                let mut s = status.lock().await;
                s.push_log(format!("Export lisible : {e}"));
                format!("export erreur: {e}")
            }
        };

        {
            let mut s = status.lock().await;
            s.set_phase(94, format!("Publication vers {}…", final_path.display()));
        }
        publish_snapshot(&store_path, &final_path)?;
        let store_s = final_path.display().to_string();
        let mut warn = String::new();
        if !skipped_sources.is_empty() {
            warn.push_str(&format!(
                " (partiel Neverest : {} ignorée(s)/échec : {})",
                skipped_sources.len(),
                skipped_sources.join(", ")
            ));
        } else if last_out.to_ascii_lowercase().contains("error")
            || last_out.contains("Accès refusé")
            || last_out.contains("os error 5")
        {
            warn.push_str(" (partiel Neverest : écriture locale refusée sur certains items)");
        }
        Ok::<(String, PathBuf), String>((
            format!(
                "Sauvegarde terminée ({sync_ok}/{n} source(s)) → {store_s} · export : {export_note}{warn}"
            ),
            final_path,
        ))
    }
    .await;

    BACKUP_RUNNING.store(false, Ordering::SeqCst);
    match &result {
        Ok((msg, _)) => {
            let mut s = status.lock().await;
            s.running = false;
            s.progress = 100;
            s.message = msg.clone();
            s.push_log(msg.clone());
            s.last_ok = true;
        }
        Err(e) => {
            let mut s = status.lock().await;
            s.running = false;
            s.message = e.clone();
            s.push_log(format!("ERREUR: {e}"));
            s.last_ok = false;
        }
    }
    result
}
