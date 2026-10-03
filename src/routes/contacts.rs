use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use askama::Template;
use axum::extract::{Multipart, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Json, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;
use serde_json::json;

use crate::cli::cardamum::{CardamumClient, VcardFields};
use crate::cli::tcard as tcard_bridge;
use crate::cli::tcard::PhotoEdit;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/contacts", get(contacts_page))
        .route("/api/contacts/suggest", get(suggest))
        .route("/api/contacts/list", get(list_api))
        .route("/api/contacts/avatar", get(avatar_api))
        .route("/api/contacts/avatars", get(avatars_batch_api))
        .route("/api/contacts/resolve", get(resolve_api))
        .route("/api/contacts/detail", get(detail_api))
        .route("/contacts/create", post(create_contact))
        .route("/contacts/update", post(update_contact))
        .route("/contacts/delete", post(delete_contact))
}

pub fn avatars_dir() -> Result<PathBuf, String> {
    let base = dirs::data_local_dir().ok_or("impossible de résoudre LOCALAPPDATA")?;
    let dir = base.join("HimaWeb").join("avatars");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn avatar_stem(email: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    email.trim().to_ascii_lowercase().hash(&mut h);
    format!("{:016x}", h.finish())
}

pub fn avatar_path(email: &str, ext: &str) -> Result<PathBuf, String> {
    Ok(avatars_dir()?.join(format!("{}.{}", avatar_stem(email), ext)))
}

const AVATAR_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

/// Fichier avatar déjà présent sur disque (indépendamment de SQLite).
pub fn find_avatar_file(email: &str) -> Option<(String, PathBuf)> {
    for ext in AVATAR_EXTS {
        if let Ok(path) = avatar_path(email, ext) {
            if path.is_file() {
                let ext = if *ext == "jpeg" { "jpg" } else { *ext };
                return Some((ext.to_string(), path));
            }
        }
    }
    None
}

fn avatar_url_for(email: &str, etag: &str) -> String {
    let mut url = format!(
        "/api/contacts/avatar?email={}",
        urlencoding::encode(email)
    );
    let v = if !etag.is_empty() {
        etag.to_string()
    } else if let Some((_, path)) = find_avatar_file(email) {
        std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    if !v.is_empty() {
        url.push_str("&v=");
        url.push_str(&urlencoding::encode(&v));
    }
    url
}

/// Écrit la PHOTO vCard en cache pour chaque email (mails + fiche).
async fn cache_photo_bytes(
    state: &AppState,
    emails: &[String],
    ext: &str,
    bytes: &[u8],
    etag: &str,
) {
    if bytes.is_empty() || emails.is_empty() {
        return;
    }
    let cache = state.cache.lock().await;
    for email in emails {
        let email = email.trim();
        if email.is_empty() || !email.contains('@') {
            continue;
        }
        if let Ok(path) = avatar_path(email, ext) {
            if let Err(e) = std::fs::write(&path, bytes) {
                tracing::warn!("avatar write {}: {e}", path.display());
                continue;
            }
            let _ = cache.set_contact_photo(email, 1, ext, etag);
        }
    }
}

fn emails_for_photo_cache(fields: &VcardFields, extra: &str) -> Vec<String> {
    let mut out = Vec::new();
    for e in &fields.emails {
        let e = e.trim().to_ascii_lowercase();
        if !e.is_empty() && !out.contains(&e) {
            out.push(e);
        }
    }
    if !fields.email.trim().is_empty() {
        let e = fields.email.trim().to_ascii_lowercase();
        if !out.contains(&e) {
            out.push(e);
        }
    }
    let extra = extra.trim().to_ascii_lowercase();
    if !extra.is_empty() && extra.contains('@') && !out.contains(&extra) {
        out.push(extra);
    }
    out
}

#[derive(Template)]
#[template(path = "shell.html")]
struct ShellTemplate {
    pub title: String,
    pub active_tab: String,
    pub offline: bool,
    pub himalaya_available: bool,

    pub theme: String,
    pub layout: String,
    pub topbar_mode: String,
    pub ui_style: String,
    pub error: Option<String>,
    pub content: String,
}

#[derive(Template)]
#[template(path = "contacts.html")]
struct ContactsTemplate {
    pub contacts: Vec<ContactRow>,
    pub books: Vec<BookOpt>,
    pub current_book_enc: String,
    pub query: String,
    pub query_enc: String,
    pub source: String,
    pub cardamum_available: bool,
    pub error: Option<String>,
    pub flash: Option<String>,
}

pub struct ContactRow {
    pub id: String,
    pub name: String,
    pub email: String,
    pub tel: String,
    pub initial: String,
    pub book: String,
    pub book_ref: String,
}

pub struct BookOpt {
    pub id: String,
    pub name: String,
    pub selected: bool,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub q: Option<String>,
    pub book: Option<String>,
    pub refresh: Option<String>,
    pub msg: Option<String>,
}

async fn contacts_page(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let query = q.q.unwrap_or_default();
    let book = q.book.unwrap_or_else(|| "__all__".into());
    let force = q.refresh.as_deref() == Some("1") || q.refresh.as_deref() == Some("true");
    let (contacts, books, source, error) = load_contacts(&state, &query, &book, force).await;

    let query_enc = urlencoding::encode(&query).into_owned();
    let current_book_enc = urlencoding::encode(&book).into_owned();
    let inner = ContactsTemplate {
        contacts,
        books,

        current_book_enc,
        query,
        query_enc,
        source,
        cardamum_available: state.cardamum_available,
        error,
        flash: q.msg,
    };
    let content = match inner.render() {
        Ok(c) => c,
        Err(e) => format!("<pre>{e}</pre>"),
    };

    let (theme, layout) = state.theme_layout().await;
    let shell = ShellTemplate {
        title: "HimaWeb — Contacts".into(),
        active_tab: "contacts".into(),
        offline: false,
        himalaya_available: state.himalaya_available,

        theme,
        layout,
        topbar_mode: state.topbar_mode().await,
        ui_style: state.ui_style().await,
        error: None,
        content,
    };
    match shell.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => Html(format!("<pre>{e}</pre>")).into_response(),
    }
}

fn book_ref(account: &str, book: &str) -> String {
    if account.is_empty() {
        book.to_string()
    } else if book.contains("::") {
        book.to_string()
    } else {
        format!("{account}::{book}")
    }
}

async fn populate_books(
    state: &Arc<AppState>,
    books: &mut Vec<BookOpt>,
    book: &str,
) -> Vec<crate::cli::cardamum::AddressBookInfo> {
    let Some(client) = &state.cardamum else {
        return vec![];
    };
    let _permit = state.cli_limit.acquire().await.ok();
    match client.list_all_addressbooks().await {
        Ok(list) => {
            for b in &list {
                books.push(BookOpt {
                    selected: b.id == book,
                    id: b.id.clone(),
                    name: b.name.clone(),
                });
            }
            list
        }
        Err(e) => {
            tracing::warn!("addressbooks: {e}");
            vec![]
        }
    }
}

fn book_is_all(book: &str) -> bool {
    book == "__all__" || book.is_empty()
}

async fn load_contacts(
    state: &Arc<AppState>,
    query: &str,
    book: &str,
    force_refresh: bool,
) -> (
    Vec<ContactRow>,
    Vec<BookOpt>,
    String,
    Option<String>,
) {
    // Filtre texte géré en live côté client — on charge le carnet complet.
    let _ = query;

    let loc = {
        let prefs = state.prefs.lock().await;
        crate::i18n::normalize_locale(&prefs.locale)
    };
    let mut books = vec![BookOpt {
        id: "__all__".into(),
        name: crate::i18n::t(loc, "contacts.all_books"),
        selected: book_is_all(book),
    }];

    let cache_count = {
        let cache = state.cache.lock().await;
        cache.contacts_count().unwrap_or(0)
    };

    if !force_refresh && book_is_all(book) && cache_count > 0 {
        let _ = populate_books(state, &mut books, book).await;
        let all = load_all_cached(state).await;
        let contacts: Vec<_> = all
            .into_iter()
            .map(|c| {
                let book = c.book_ref.clone();
                to_row(c.card_id, c.name, c.email, String::new(), book.clone(), book)
            })
            .collect();
        let bg = Arc::clone(state);
        tokio::spawn(async move {
            let _ = refresh_contacts_into_cache(&bg).await;
        });
        return (contacts, books, "cache".into(), None);
    }

    if state.cardamum.is_some() {
        let book_list = populate_books(state, &mut books, book).await;

        let book_refs: Vec<(Option<String>, String)> = if book_is_all(book) {
            book_list
                .iter()
                .map(|b| {
                    if let Some((a, id)) = b.id.split_once("::") {
                        (Some(a.to_string()), id.to_string())
                    } else {
                        (None, b.id.clone())
                    }
                })
                .collect()
        } else if let Some((a, id)) = book.split_once("::") {
            vec![(Some(a.to_string()), id.to_string())]
        } else {
            vec![(None, book.to_string())]
        };

        let client = state.cardamum.as_ref().unwrap();
        let _permit = state.cli_limit.acquire().await.ok();
        match client.list_contacts_from_books(&book_refs).await {
            Ok(items) if !items.is_empty() => {
                let records = CardamumClient::to_records(&items);
                {
                    let cache = state.cache.lock().await;
                    let _ = cache.save_contact_records(&records);
                }
                let contacts: Vec<_> = items
                    .into_iter()
                    .map(|c| {
                        let bref = book_ref(&c.account, &c.addressbook);
                        to_row(c.id, c.name, c.email, c.tel, c.addressbook, bref)
                    })
                    .collect();
                return (contacts, books, "cardamum".into(), None);
            }
            Ok(_) => {
                // Carnet précis : ne pas servir le cache global non filtré.
                if !book_is_all(book) {
                    return (
                        vec![],
                        books,
                        "cardamum".into(),
                        Some("Aucun contact dans ce carnet (ou carnets vides).".into()),
                    );
                }
                let all = load_all_cached(state).await;
                if !all.is_empty() {
                    let contacts: Vec<_> = all
                        .into_iter()
                        .map(|c| {
                            let book = c.book_ref.clone();
                            to_row(c.card_id, c.name, c.email, String::new(), book.clone(), book)
                        })
                        .collect();
                    return (
                        contacts,
                        books,
                        "cache".into(),
                        Some("Carnets distants vides — cache local.".into()),
                    );
                }
                return (
                    vec![],
                    books,
                    "cardamum".into(),
                    Some("Aucun contact dans ce carnet (ou carnets vides).".into()),
                );
            }
            Err(e) => {
                if !book_is_all(book) {
                    return (
                        vec![],
                        books,
                        "cache".into(),
                        Some(format!("Cardamum: {e}")),
                    );
                }
                let all = load_all_cached(state).await;
                let contacts: Vec<_> = all
                    .into_iter()
                    .map(|c| {
                        let book = c.book_ref.clone();
                        to_row(c.card_id, c.name, c.email, String::new(), book.clone(), book)
                    })
                    .collect();
                return (
                    contacts,
                    books,
                    "cache".into(),
                    Some(format!("Cardamum: {e} — cache local.")),
                );
            }
        }
    }

    let all = load_all_cached(state).await;
    let contacts: Vec<_> = all
        .into_iter()
        .map(|c| {
            let book = c.book_ref.clone();
            to_row(c.card_id, c.name, c.email, String::new(), book.clone(), book)
        })
        .collect();
    let err = if contacts.is_empty() {
        Some(
            "Aucun contact. Importez Thunderbird (Paramètres → Contacts) ou corrigez Cardamum."
                .into(),
        )
    } else {
        None
    };
    (contacts, books, "cache".into(), err)
}

fn to_row(
    id: String,
    name: String,
    email: String,
    tel: String,
    book: String,
    book_ref: String,
) -> ContactRow {
    let initial = name
        .chars()
        .next()
        .or_else(|| email.chars().next())
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    ContactRow {
        id,
        name,
        email,
        tel,
        initial,
        book,
        book_ref,
    }
}

async fn load_all_cached(state: &AppState) -> Vec<crate::cli::cardamum::ContactRecord> {
    let cache = state.cache.lock().await;
    cache.list_contacts_full("", 5000).unwrap_or_default()
}

/// Remplit le cache contacts depuis Cardamum (démarrage / background).
pub async fn refresh_contacts_into_cache(state: &AppState) -> Result<usize, String> {
    let Some(client) = &state.cardamum else {
        return Ok(0);
    };
    // Tâche de fond : ne pas consommer un créneau du pool interactif.
    let n = {
        let _permit = state
            .cli_bg_limit
            .acquire()
            .await
            .map_err(|e| e.to_string())?;
        let books = client
            .list_all_addressbooks()
            .await
            .map_err(|e| e.to_string())?;
        let refs: Vec<(Option<String>, String)> = books
            .iter()
            .map(|b| {
                if let Some((a, id)) = b.id.split_once("::") {
                    (Some(a.to_string()), id.to_string())
                } else {
                    (None, b.id.clone())
                }
            })
            .collect();
        let items = client
            .list_contacts_from_books(&refs)
            .await
            .map_err(|e| e.to_string())?;
        let records = CardamumClient::to_records(&items);
        let n = records.len();
        {
            let cache = state.cache.lock().await;
            cache
                .save_contact_records(&records)
                .map_err(|e| e.to_string())?;
        }
        tracing::info!("cache contacts: {n} entrées");
        n
    };
    Ok(n)
}

/// Lookup batch email → URL avatar (SQLite **ou** fichier disque).
/// Ne lance aucun `card read` — safe sur le chemin liste mail.
pub async fn avatar_urls_for(
    state: &AppState,
    emails: &[String],
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if emails.is_empty() {
        return out;
    }
    let meta = {
        let cache = state.cache.lock().await;
        cache.photo_meta_for_emails(emails).unwrap_or_default()
    };
    for email in emails {
        let key = email.trim().to_ascii_lowercase();
        if key.is_empty() || out.contains_key(&key) {
            continue;
        }
        if let Some((has_photo, _ext, etag)) = meta.get(&key) {
            if *has_photo == 1 && find_avatar_file(&key).is_some() {
                out.insert(key.clone(), avatar_url_for(&key, etag));
                continue;
            }
        }
        // Fiche contact a pu écrire le fichier avant que SQLite soit à jour / après wipe etag
        if let Some((ext, _)) = find_avatar_file(&key) {
            let etag = meta
                .get(&key)
                .map(|(_, _, e)| e.clone())
                .unwrap_or_default();
            {
                let cache = state.cache.lock().await;
                let _ = cache.set_contact_photo(&key, 1, &ext, &etag);
            }
            out.insert(key.clone(), avatar_url_for(&key, &etag));
        }
    }
    out
}

/// File d’emails prioritaires (expéditeurs visibles) — jamais ignorée si un sync tourne.
static PHOTO_PRIORITY: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PHOTO_SYNC_BUSY: AtomicBool = AtomicBool::new(false);

fn enqueue_photo_priority(priority: &[String]) {
    if priority.is_empty() {
        return;
    }
    let Ok(mut q) = PHOTO_PRIORITY.lock() else {
        return;
    };
    for e in priority {
        let e = e.trim().to_ascii_lowercase();
        if e.contains('@') && !q.iter().any(|x| x == &e) {
            q.push(e);
        }
    }
}

fn drain_photo_priority(max: usize) -> Vec<String> {
    let Ok(mut q) = PHOTO_PRIORITY.lock() else {
        return Vec::new();
    };
    let n = max.min(q.len());
    q.drain(..n).collect()
}

fn photo_priority_pending() -> bool {
    PHOTO_PRIORITY
        .lock()
        .map(|q| !q.is_empty())
        .unwrap_or(false)
}

/// Démarre un sync photos en fond (max 2 via `cli_bg_limit`), priorisant `priority`.
///
/// Les emails prioritaires sont mis en file : un sync déjà en cours ne les perd pas.
pub fn spawn_photo_sync(state: Arc<AppState>, priority: Vec<String>) {
    if !state.cardamum_available {
        return;
    }
    enqueue_photo_priority(&priority);
    start_photo_sync_worker(state);
}

fn start_photo_sync_worker(state: Arc<AppState>) {
    if PHOTO_SYNC_BUSY
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    tokio::spawn(async move {
        let mut total = 0usize;
        loop {
            // 1) Toujours vider la priorité mail avant le backlog carnet
            let pri = drain_photo_priority(40);
            if !pri.is_empty() {
                match refresh_contact_photos(&state, &pri).await {
                    Ok(n) => total += n,
                    Err(e) => tracing::debug!("photo sync priority: {e}"),
                }
                continue;
            }
            // 2) Backlog général (warm), par petits lots pour rester interruptible
            match refresh_contact_photos(&state, &[]).await {
                Ok(0) => {
                    if photo_priority_pending() {
                        continue;
                    }
                    break;
                }
                Ok(n) => {
                    total += n;
                    if photo_priority_pending() {
                        continue;
                    }
                    // Un lot de backlog à la fois ; s’il reste du pending DB, boucler
                    let more = {
                        let cache = state.cache.lock().await;
                        cache
                            .contacts_pending_photo(1)
                            .map(|v| !v.is_empty())
                            .unwrap_or(false)
                    };
                    if more {
                        continue;
                    }
                    break;
                }
                Err(e) => {
                    tracing::debug!("photo sync: {e}");
                    break;
                }
            }
        }
        PHOTO_SYNC_BUSY.store(false, Ordering::SeqCst);
        // Course : des priorités ont pu arriver juste après le break
        if photo_priority_pending() {
            start_photo_sync_worker(state);
        } else if total > 0 {
            tracing::info!("avatars contacts: {total} photos mises en cache (sync terminé)");
        }
    });
}

/// Télécharge les PHOTO vCard manquantes en parallèle (pool bg).
pub async fn refresh_contact_photos(
    state: &AppState,
    priority: &[String],
) -> Result<usize, String> {
    let Some(client) = state.cardamum.clone() else {
        return Ok(0);
    };

    // Si les expéditeurs ne sont pas encore dans le carnet local, rafraîchir Cardamum d’abord
    if !priority.is_empty() {
        let need = {
            let cache = state.cache.lock().await;
            cache
                .contacts_need_refresh_for_photos(priority)
                .unwrap_or(true)
        };
        if need {
            match refresh_contacts_into_cache(state).await {
                Ok(n) => tracing::debug!("photo sync: refresh carnet ({n} contacts)"),
                Err(e) => tracing::debug!("photo sync: refresh carnet échoué: {e}"),
            }
        }
    }

    let limit = if priority.is_empty() { 24 } else { 40 };
    let pending = {
        let cache = state.cache.lock().await;
        cache
            .contacts_pending_photo_for(priority, limit)
            .map_err(|e| e.to_string())?
    };
    if pending.is_empty() {
        return Ok(0);
    }

    // Dédupliquer les card_id (un contact multi-email → un seul card read)
    let mut seen_cards = HashSet::new();
    let mut tasks = tokio::task::JoinSet::new();
    for (email, card_id, book_ref, etag) in pending {
        let card_key = format!("{book_ref}::{card_id}");
        if !seen_cards.insert(card_key) {
            continue;
        }
        let st = state.cache.clone();
        let bg = Arc::clone(&state.cli_bg_limit);
        let client = client.clone();
        let state_emails = state.cache.clone();
        tasks.spawn(async move {
            let _permit = bg.acquire().await.ok()?;
            match client.read_card(&book_ref, &card_id).await {
                Ok((new_etag, contents)) => {
                    let etag_use = if new_etag.is_empty() {
                        etag
                    } else {
                        new_etag
                    };
                    // Tous les emails de cette fiche dans le cache (pas seulement celui demandé)
                    let sibling_emails = {
                        let cache = state_emails.lock().await;
                        cache
                            .emails_for_card(&card_id, &book_ref)
                            .unwrap_or_else(|_| vec![email.clone()])
                    };
                    let emails = if sibling_emails.is_empty() {
                        vec![email]
                    } else {
                        sibling_emails
                    };
                    if let Some((ext, bytes)) = CardamumClient::parse_vcard_photo(&contents) {
                        let mut ok = false;
                        for em in &emails {
                            if let Ok(path) = avatar_path(em, &ext) {
                                if let Err(e) = std::fs::write(&path, &bytes) {
                                    tracing::warn!("avatar write {}: {e}", path.display());
                                    continue;
                                }
                                let cache = st.lock().await;
                                let _ = cache.set_contact_photo(em, 1, &ext, &etag_use);
                                ok = true;
                            }
                        }
                        if ok {
                            Some(1usize)
                        } else {
                            None
                        }
                    } else {
                        let cache = st.lock().await;
                        for em in &emails {
                            let _ = cache.set_contact_photo(em, -1, "", &etag_use);
                        }
                        Some(0usize)
                    }
                }
                Err(e) => {
                    tracing::debug!("card read {card_id}: {e}");
                    None
                }
            }
        });
    }

    let mut fetched = 0usize;
    while let Some(joined) = tasks.join_next().await {
        if let Ok(Some(n)) = joined {
            fetched += n;
        }
    }
    if fetched > 0 {
        tracing::debug!("avatars: lot +{fetched}");
    }
    Ok(fetched)
}

#[derive(Deserialize)]
struct AvatarQuery {
    email: Option<String>,
}

#[derive(Deserialize)]
struct ResolveQuery {
    email: Option<String>,
}

/// Retrouve le vrai contact CardDAV (id + carnet) depuis le cache, sinon refresh.
async fn resolve_api(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ResolveQuery>,
) -> impl IntoResponse {
    let email = q.email.unwrap_or_default();
    let email = email.trim().to_ascii_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Json(json!({ "error": "email requis" })).into_response();
    }

    let mut found = {
        let cache = state.cache.lock().await;
        cache.contact_by_email(&email).ok().flatten()
    };

    let needs_refresh = found
        .as_ref()
        .map(|c| c.card_id.is_empty() || c.book_ref.is_empty())
        .unwrap_or(true);

    if needs_refresh && state.cardamum_available {
        let _ = refresh_contacts_into_cache(&state).await;
        found = {
            let cache = state.cache.lock().await;
            cache.contact_by_email(&email).ok().flatten()
        };
    }

    // Dernier recours : scan Cardamum live pour cet email
    if found
        .as_ref()
        .map(|c| c.card_id.is_empty() || c.book_ref.is_empty())
        .unwrap_or(true)
    {
        if let Some(client) = &state.cardamum {
            let _permit = state.cli_limit.acquire().await.ok();
            if let Ok(items) = client.list_contacts().await {
                let records = CardamumClient::to_records(&items);
                {
                    let cache = state.cache.lock().await;
                    let _ = cache.save_contact_records(&records);
                }
                if let Some(c) = items.into_iter().find(|c| {
                    c.email.trim().eq_ignore_ascii_case(&email) && !c.id.is_empty()
                }) {
                    let bref = book_ref(&c.account, &c.addressbook);
                    return Json(json!({
                        "ok": true,
                        "id": c.id,
                        "book": bref,
                        "name": c.name,
                        "email": c.email,
                        "tel": c.tel,
                        "source": "cardamum",
                    }))
                    .into_response();
                }
            }
        }
    }

    match found {
        Some(c) if !c.card_id.is_empty() && !c.book_ref.is_empty() => Json(json!({
            "ok": true,
            "id": c.card_id,
            "book": c.book_ref,
            "name": c.name,
            "email": c.email,
            "tel": "",
            "source": "cache",
        }))
        .into_response(),
        _ => Json(json!({
            "error": "Contact distant introuvable pour cet email",
            "email": email,
        }))
        .into_response(),
    }
}

async fn avatar_api(
    State(state): State<Arc<AppState>>,
    Query(q): Query<AvatarQuery>,
) -> impl IntoResponse {
    let email = q.email.unwrap_or_default();
    let email = email.trim();
    if email.is_empty() || !email.contains('@') {
        return StatusCode::NOT_FOUND.into_response();
    }
    let meta = {
        let cache = state.cache.lock().await;
        cache.contact_photo_file(email).ok().flatten()
    };
    let (ext, path) = if let Some((ext, _)) = meta {
        match avatar_path(email, &ext) {
            Ok(p) if p.is_file() => (ext, p),
            _ => match find_avatar_file(email) {
                Some(v) => v,
                None => return StatusCode::NOT_FOUND.into_response(),
            },
        }
    } else if let Some(v) = find_avatar_file(email) {
        // Répare SQLite au passage
        {
            let cache = state.cache.lock().await;
            let _ = cache.set_contact_photo(email, 1, &v.0, "");
        }
        v
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) if !b.is_empty() => b,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let mime = match ext.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (
                header::CACHE_CONTROL,
                "private, max-age=604800, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

#[derive(Deserialize)]
struct AvatarsBatchQuery {
    /// Emails séparés par des virgules
    emails: Option<String>,
}

async fn avatars_batch_api(
    State(state): State<Arc<AppState>>,
    Query(q): Query<AvatarsBatchQuery>,
) -> impl IntoResponse {
    let raw = q.emails.unwrap_or_default();
    let emails: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| s.contains('@'))
        .take(80)
        .collect();
    let urls = avatar_urls_for(&state, &emails).await;
    // Relance un sync fond pour ceux encore absents
    let missing: Vec<String> = emails
        .into_iter()
        .filter(|e| !urls.contains_key(e))
        .collect();
    if !missing.is_empty() {
        spawn_photo_sync(Arc::clone(&state), missing);
    }
    Json(json!({ "avatars": urls })).into_response()
}

async fn list_api(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let query = q.q.unwrap_or_default();
    let book = q.book.unwrap_or_else(|| "__all__".into());
    let force = q.refresh.as_deref() == Some("1");
    let (contacts, _books, source, error) = load_contacts(&state, &query, &book, force).await;
    Json(json!({
        "source": source,
        "error": error,
        "items": contacts.into_iter().map(|c| json!({
            "id": c.id,
            "name": c.name,
            "email": c.email,
            "label": if c.name.is_empty() { c.email.clone() } else { format!("{} <{}>", c.name, c.email) }
        })).collect::<Vec<_>>()
    }))
}

#[derive(Deserialize)]
pub struct SuggestQuery {
    pub q: Option<String>,
}

async fn suggest(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SuggestQuery>,
) -> impl IntoResponse {
    let query = q.q.unwrap_or_default();
    if query.trim().len() < 2 {
        return Json(json!({ "items": [] })).into_response();
    }

    let cached = {
        let cache = state.cache.lock().await;
        cache.suggest_contacts(&query).unwrap_or_default()
    };
    if !cached.is_empty() {
        let payload: Vec<_> = cached
            .into_iter()
            .map(|(email, name)| {
                json!({
                    "name": name,
                    "email": email,
                    "label": if name.is_empty() {
                        email.clone()
                    } else {
                        format!("{name} <{email}>")
                    }
                })
            })
            .collect();
        return Json(json!({ "items": payload, "source": "cache" })).into_response();
    }

    if let Some(client) = &state.cardamum {
        let _permit = state.cli_limit.acquire().await.ok();
        match client.suggest(&query).await {
            Ok(items) => {
                let pairs: Vec<(String, String)> = items
                    .iter()
                    .filter(|c| !c.email.is_empty())
                    .map(|c| (c.email.clone(), c.name.clone()))
                    .collect();
                if !pairs.is_empty() {
                    let cache = state.cache.lock().await;
                    let _ = cache.merge_contacts(&pairs);
                }
                let payload: Vec<_> = items
                    .into_iter()
                    .map(|c| {
                        json!({
                            "name": c.name,
                            "email": c.email,
                            "label": if c.name.is_empty() {
                                c.email.clone()
                            } else {
                                format!("{} <{}>", c.name, c.email)
                            }
                        })
                    })
                    .collect();
                return Json(json!({ "items": payload, "source": "cardamum" })).into_response();
            }
            Err(e) => tracing::warn!("cardamum suggest: {e}"),
        }
    }

    Json(json!({ "items": [], "disabled": !state.cardamum_available })).into_response()
}

#[derive(Default)]
struct ContactForm {
    book: String,
    id: String,
    name: String,
    email: String,
    tel: String,
    nickname: String,
    org: String,
    title: String,
    street: String,
    city: String,
    region: String,
    postal: String,
    country: String,
    address: String,
    url: String,
    note: String,
    /// octets image uploadée (vide = inchangé)
    photo_bytes: Vec<u8>,
    photo_mime: String,
    /// "1" = supprimer la photo
    clear_photo: bool,
}

fn form_to_fields(form: &ContactForm) -> VcardFields {
    let email = form.email.trim().to_string();
    let tel = form.tel.trim().to_string();
    VcardFields {
        fn_name: form.name.trim().to_string(),
        nickname: form.nickname.trim().to_string(),
        email: email.clone(),
        emails: if email.is_empty() {
            vec![]
        } else {
            vec![email]
        },
        tel: tel.clone(),
        tels: if tel.is_empty() { vec![] } else { vec![tel] },
        org: form.org.trim().to_string(),
        title: form.title.trim().to_string(),
        note: form.note.trim().to_string(),
        url: form.url.trim().to_string(),
        street: form.street.trim().to_string(),
        city: form.city.trim().to_string(),
        region: form.region.trim().to_string(),
        postal: form.postal.trim().to_string(),
        country: form.country.trim().to_string(),
        address: form.address.trim().to_string(),
        has_photo: false,
    }
}

fn photo_edit_from_form(form: &ContactForm) -> PhotoEdit {
    if form.clear_photo {
        return PhotoEdit::Clear;
    }
    if form.photo_bytes.is_empty() {
        return PhotoEdit::Keep;
    }
    let mime = if form.photo_mime.starts_with("image/") {
        form.photo_mime.clone()
    } else {
        "image/jpeg".into()
    };
    // Limite ~1.5 Mo décodés côté CardDAV
    if form.photo_bytes.len() > 1_500_000 {
        return PhotoEdit::Keep;
    }
    PhotoEdit::Set(tcard_bridge::photo_data_uri(&mime, &form.photo_bytes))
}

fn mime_to_avatar_ext(mime: &str) -> &'static str {
    let m = mime.to_ascii_lowercase();
    if m.contains("png") {
        "png"
    } else if m.contains("gif") {
        "gif"
    } else if m.contains("webp") {
        "webp"
    } else {
        "jpg"
    }
}

async fn parse_contact_multipart(mut multipart: Multipart) -> Result<ContactForm, String> {
    let mut form = ContactForm::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| format!("multipart: {e}"))?
    {
        let name = field.name().unwrap_or("").to_string();
        let content_type = field.content_type().map(|m| m.to_string());
        let data = field
            .bytes()
            .await
            .map_err(|e| format!("champ {name}: {e}"))?;
        let text = || String::from_utf8_lossy(&data).trim().to_string();
        match name.as_str() {
            "book" => form.book = text(),
            "id" => form.id = text(),
            "name" => form.name = text(),
            "email" => form.email = text(),
            "tel" => form.tel = text(),
            "nickname" => form.nickname = text(),
            "org" => form.org = text(),
            "title" => form.title = text(),
            "street" => form.street = text(),
            "city" => form.city = text(),
            "region" => form.region = text(),
            "postal" => form.postal = text(),
            "country" => form.country = text(),
            "address" => form.address = text(),
            "url" => form.url = text(),
            "note" => form.note = text(),
            "clear_photo" => {
                let v = text();
                form.clear_photo = v == "1" || v.eq_ignore_ascii_case("on") || v == "true";
            }
            "photo" => {
                if !data.is_empty() {
                    form.photo_bytes = data.to_vec();
                    form.photo_mime = content_type
                        .filter(|c| c.starts_with("image/"))
                        .unwrap_or_else(|| "image/jpeg".into());
                }
            }
            _ => {}
        }
    }
    Ok(form)
}

async fn create_contact(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> impl IntoResponse {
    let Some(client) = &state.cardamum else {
        return Redirect::to("/contacts?msg=Cardamum%20absent").into_response();
    };
    let form = match parse_contact_multipart(multipart).await {
        Ok(f) => f,
        Err(e) => {
            let err_s = format!("Erreur: {e}");
            let msg = urlencoding::encode(&err_s);
            return Redirect::to(&format!("/contacts?msg={msg}")).into_response();
        }
    };
    let book = form.book.trim();
    if book.is_empty() || book == "__all__" {
        return Redirect::to("/contacts?msg=Choisissez%20un%20carnet").into_response();
    }
    let fields = form_to_fields(&form);
    if fields.fn_name.is_empty() {
        return Redirect::to(&format!(
            "/contacts?book={}&msg=Nom%20requis",
            urlencoding::encode(book)
        ))
        .into_response();
    }
    let photo = photo_edit_from_form(&form);
    let vcard = match tcard_bridge::build_vcard_with_photo(
        &uuid::Uuid::new_v4().to_string(),
        &fields,
        photo,
    ) {
        Ok(v) => v,
        Err(e) => {
            let err_s = format!("Erreur tcard: {e}");
            let msg = urlencoding::encode(&err_s);
            return Redirect::to(&format!("/contacts?book={}&msg={msg}", urlencoding::encode(book)))
                .into_response();
        }
    };
    let _permit = state.cli_limit.acquire().await.ok();
    match client.create_card(book, vcard.as_bytes()).await {
        Ok(()) => {
            if !form.photo_bytes.is_empty() {
                let emails = emails_for_photo_cache(&fields, "");
                let ext = mime_to_avatar_ext(&form.photo_mime);
                cache_photo_bytes(&state, &emails, ext, &form.photo_bytes, "").await;
            }
            let _ = refresh_contacts_into_cache(&state).await;
            Redirect::to(&format!(
                "/contacts?book={}&refresh=1&msg=Contact%20créé",
                urlencoding::encode(book)
            ))
            .into_response()
        }
        Err(e) => {
            let err_s = format!("Erreur: {e}");
            let msg = urlencoding::encode(&err_s);
            Redirect::to(&format!("/contacts?book={}&msg={msg}", urlencoding::encode(book)))
                .into_response()
        }
    }
}

async fn update_contact(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> impl IntoResponse {
    let Some(client) = &state.cardamum else {
        return Redirect::to("/contacts?msg=Cardamum%20absent").into_response();
    };
    let form = match parse_contact_multipart(multipart).await {
        Ok(f) => f,
        Err(e) => {
            let err_s = format!("Erreur: {e}");
            let msg = urlencoding::encode(&err_s);
            return Redirect::to(&format!("/contacts?msg={msg}")).into_response();
        }
    };
    let book = form.book.trim();
    let id = form.id.trim();
    if book.is_empty() || book == "__all__" || id.is_empty() {
        return Redirect::to("/contacts?msg=Contact%20incomplet").into_response();
    }
    let fields = form_to_fields(&form);
    if fields.fn_name.is_empty() {
        return Redirect::to(&format!(
            "/contacts?book={}&msg=Nom%20requis",
            urlencoding::encode(book)
        ))
        .into_response();
    }
    let email = fields.email.clone();
    let photo = photo_edit_from_form(&form);

    let _permit = state.cli_limit.acquire().await.ok();
    let vcard = match client.read_card(book, id).await {
        Ok((etag, contents)) => {
            let patched = if contents.trim().is_empty() {
                tcard_bridge::build_vcard_with_photo(id, &fields, photo.clone())
            } else {
                tcard_bridge::apply_fields_with_photo(&contents, &fields, photo.clone())
            };
            match patched {
                Ok(v) => (etag, v),
                Err(e) => {
                    let err_s = format!("Erreur tcard: {e}");
                    let msg = urlencoding::encode(&err_s);
                    return Redirect::to(&format!(
                        "/contacts?book={}&msg={msg}",
                        urlencoding::encode(book)
                    ))
                    .into_response();
                }
            }
        }
        Err(_) => match tcard_bridge::build_vcard_with_photo(id, &fields, photo) {
            Ok(v) => (String::new(), v),
            Err(e) => {
                let err_s = format!("Erreur tcard: {e}");
                let msg = urlencoding::encode(&err_s);
                return Redirect::to(&format!(
                    "/contacts?book={}&msg={msg}",
                    urlencoding::encode(book)
                ))
                .into_response();
            }
        },
    };

    let if_match = if vcard.0.is_empty() {
        None
    } else {
        Some(vcard.0.as_str())
    };
    match client
        .update_card(book, id, vcard.1.as_bytes(), if_match)
        .await
    {
        Ok(()) => {
            after_contact_photo_save(&state, &form, &fields).await;
            let _ = refresh_contacts_into_cache(&state).await;
            if !email.is_empty() && form.photo_bytes.is_empty() && !form.clear_photo {
                spawn_photo_sync(Arc::clone(&state), vec![email.to_ascii_lowercase()]);
            }
            Redirect::to(&format!(
                "/contacts?book={}&refresh=1&msg=Contact%20modifié",
                urlencoding::encode(book)
            ))
            .into_response()
        }
        Err(e) => {
            let retry = client.update_card(book, id, vcard.1.as_bytes(), None).await;
            match retry {
                Ok(()) => {
                    after_contact_photo_save(&state, &form, &fields).await;
                    let _ = refresh_contacts_into_cache(&state).await;
                    Redirect::to(&format!(
                        "/contacts?book={}&refresh=1&msg=Contact%20modifié",
                        urlencoding::encode(book)
                    ))
                    .into_response()
                }
                Err(_) => {
                    let err_s = format!("Erreur: {e}");
                    let msg = urlencoding::encode(&err_s);
                    Redirect::to(&format!(
                        "/contacts?book={}&msg={msg}",
                        urlencoding::encode(book)
                    ))
                    .into_response()
                }
            }
        }
    }
}

async fn after_contact_photo_save(state: &AppState, form: &ContactForm, fields: &VcardFields) {
    let emails = emails_for_photo_cache(fields, "");
    if form.clear_photo {
        let cache = state.cache.lock().await;
        for email in &emails {
            let _ = cache.set_contact_photo(email, -1, "", "");
            for ext in AVATAR_EXTS {
                if let Ok(p) = avatar_path(email, ext) {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
        return;
    }
    if !form.photo_bytes.is_empty() {
        let ext = mime_to_avatar_ext(&form.photo_mime);
        cache_photo_bytes(state, &emails, ext, &form.photo_bytes, "").await;
    }
}

#[derive(Deserialize)]
struct DetailQuery {
    book: Option<String>,
    id: Option<String>,
    email: Option<String>,
}

/// Détail complet via `card read` (list Cardamum ne renvoie que FN/EMAIL/TEL).
async fn detail_api(
    State(state): State<Arc<AppState>>,
    Query(q): Query<DetailQuery>,
) -> impl IntoResponse {
    let mut book = q.book.unwrap_or_default();
    let mut id = q.id.unwrap_or_default();
    let email = q.email.unwrap_or_default();

    if (book.trim().is_empty() || id.trim().is_empty()) && !email.trim().is_empty() {
        let cache = state.cache.lock().await;
        if let Ok(Some(c)) = cache.contact_by_email(email.trim()) {
            if !c.card_id.is_empty() && !c.book_ref.is_empty() {
                id = c.card_id;
                book = c.book_ref;
            }
        }
    }

    let book = book.trim().to_string();
    let id = id.trim().to_string();
    if book.is_empty() || id.is_empty() {
        return Json(json!({ "error": "id/book requis" })).into_response();
    }

    let Some(client) = &state.cardamum else {
        return Json(json!({ "error": "Cardamum absent" })).into_response();
    };

    let _permit = state.cli_limit.acquire().await.ok();
    let (etag, contents) = match client.read_card(&book, &id).await {
        Ok(v) => v,
        Err(e) => {
            return Json(json!({ "error": format!("Lecture vCard: {e}") })).into_response();
        }
    };

    let fields = match tcard_bridge::parse_fields(&contents) {
        Ok(f) => f,
        Err(e) => {
            return Json(json!({ "error": format!("tcard: {e}") })).into_response();
        }
    };
    let email_key = if !fields.email.is_empty() {
        fields.email.trim().to_ascii_lowercase()
    } else {
        email.trim().to_ascii_lowercase()
    };
    let photo_emails = emails_for_photo_cache(&fields, &email);

    // Met en cache PHOTO pour tous les emails de la fiche (liste mail incluse)
    let mut avatar_url = String::new();
    if fields.has_photo {
        if let Some((ext, bytes)) = CardamumClient::parse_vcard_photo(&contents) {
            cache_photo_bytes(&state, &photo_emails, &ext, &bytes, &etag).await;
        }
    }
    if !email_key.is_empty() {
        let urls = avatar_urls_for(&state, &[email_key.clone()]).await;
        if let Some(u) = urls.get(&email_key) {
            avatar_url = u.clone();
        } else if fields.has_photo {
            // Fichier peut exister même si la ligne SQLite manque encore
            avatar_url = avatar_url_for(&email_key, &etag);
        }
    }

    Json(json!({
        "ok": true,
        "id": id,
        "book": book,
        "etag": etag,
        "avatar_url": avatar_url,
        "name": fields.fn_name,
        "nickname": fields.nickname,
        "email": fields.email,
        "emails": fields.emails,
        "tel": fields.tel,
        "tels": fields.tels,
        "org": fields.org,
        "title": fields.title,
        "street": fields.street,
        "city": fields.city,
        "region": fields.region,
        "postal": fields.postal,
        "country": fields.country,
        "address": fields.address,
        "url": fields.url,
        "note": fields.note,
        "has_photo": fields.has_photo,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct DeleteContactForm {
    pub book: String,
    pub id: String,
}

async fn delete_contact(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteContactForm>,
) -> impl IntoResponse {
    let Some(client) = &state.cardamum else {
        return Redirect::to("/contacts?msg=Cardamum%20absent").into_response();
    };
    if form.id.trim().is_empty() || form.book.trim().is_empty() {
        return Redirect::to("/contacts?msg=Contact%20incomplet").into_response();
    }
    let _permit = state.cli_limit.acquire().await.ok();
    match client.delete_card(form.book.trim(), form.id.trim()).await {
        Ok(()) => {
            let _ = refresh_contacts_into_cache(&state).await;
            Redirect::to(&format!(
                "/contacts?book={}&refresh=1&msg=Contact%20supprimé",
                urlencoding::encode(form.book.trim())
            ))
            .into_response()
        }
        Err(e) => {
            let err_s = format!("Erreur: {e}");
            let msg = urlencoding::encode(&err_s);
            Redirect::to(&format!("/contacts?msg={msg}")).into_response()
        }
    }
}
