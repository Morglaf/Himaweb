use std::sync::Arc;

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Json, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;
use serde_json::json;

use crate::cli::cardamum::CardamumClient;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/contacts", get(contacts_page))
        .route("/api/contacts/suggest", get(suggest))
        .route("/api/contacts/list", get(list_api))
        .route("/contacts/create", post(create_contact))
        .route("/contacts/delete", post(delete_contact))
}

#[derive(Template)]
#[template(path = "shell.html")]
struct ShellTemplate {
    pub title: String,
    pub active_tab: String,
    pub offline: bool,
    pub himalaya_available: bool,
    pub calendula_available: bool,
    pub cardamum_available: bool,
    pub theme: String,
    pub layout: String,
    pub ui_style: String,
    pub error: Option<String>,
    pub content: String,
}

#[derive(Template)]
#[template(path = "contacts.html")]
struct ContactsTemplate {
    pub contacts: Vec<ContactRow>,
    pub books: Vec<BookOpt>,
    pub current_book: String,
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
        current_book: book,
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
        calendula_available: state.calendula_available,
        cardamum_available: state.cardamum_available,
        theme,
        layout,
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
    let q = query.trim().to_ascii_lowercase();
    let filter = |name: &str, email: &str| {
        if q.is_empty() {
            return true;
        }
        name.to_ascii_lowercase().contains(&q) || email.to_ascii_lowercase().contains(&q)
    };

    let mut books = vec![BookOpt {
        id: "__all__".into(),
        name: "Tous les carnets".into(),
        selected: book == "__all__" || book.is_empty(),
    }];

    let cache_count = {
        let cache = state.cache.lock().await;
        cache.contacts_count().unwrap_or(0)
    };

    if !force_refresh && (book == "__all__" || book.is_empty()) && cache_count > 0 {
        let all = load_all_cached(state).await;
        let contacts: Vec<_> = all
            .into_iter()
            .filter(|(email, name)| filter(name, email))
            .map(|(email, name)| to_row(String::new(), name, email, String::new(), String::new()))
            .collect();
        let bg = Arc::clone(state);
        tokio::spawn(async move {
            let _ = refresh_contacts_into_cache(&bg).await;
        });
        return (contacts, books, "cache".into(), None);
    }

    if let Some(client) = &state.cardamum {
        let _permit = state.cli_limit.acquire().await.ok();
        let book_list = match client.list_all_addressbooks().await {
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
        };

        let book_refs: Vec<(Option<String>, String)> = if book == "__all__" || book.is_empty() {
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

        match client.list_contacts_from_books(&book_refs).await {
            Ok(items) if !items.is_empty() => {
                let pairs: Vec<(String, String)> = items
                    .iter()
                    .filter(|c| !c.email.is_empty())
                    .map(|c| (c.email.clone(), c.name.clone()))
                    .collect();
                {
                    let cache = state.cache.lock().await;
                    let _ = cache.save_contacts(&pairs);
                }
                let contacts: Vec<_> = items
                    .into_iter()
                    .filter(|c| filter(&c.name, &c.email))
                    .map(|c| {
                        let bref = book_ref(&c.account, &c.addressbook);
                        to_row(c.id, c.name, c.email, c.addressbook, bref)
                    })
                    .collect();
                return (contacts, books, "cardamum".into(), None);
            }
            Ok(_) => {
                let all = load_all_cached(state).await;
                if !all.is_empty() {
                    let contacts: Vec<_> = all
                        .into_iter()
                        .filter(|(email, name)| filter(name, email))
                        .map(|(email, name)| {
                            to_row(String::new(), name, email, String::new(), String::new())
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
                let all = load_all_cached(state).await;
                let contacts: Vec<_> = all
                    .into_iter()
                    .filter(|(email, name)| filter(name, email))
                    .map(|(email, name)| {
                        to_row(String::new(), name, email, String::new(), String::new())
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
        .filter(|(email, name)| filter(name, email))
        .map(|(email, name)| to_row(String::new(), name, email, String::new(), String::new()))
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

fn to_row(id: String, name: String, email: String, book: String, book_ref: String) -> ContactRow {
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
        initial,
        book,
        book_ref,
    }
}

async fn load_all_cached(state: &AppState) -> Vec<(String, String)> {
    let cache = state.cache.lock().await;
    cache.list_contacts("", 5000).unwrap_or_default()
}

/// Remplit le cache contacts depuis Cardamum (démarrage / background).
pub async fn refresh_contacts_into_cache(state: &AppState) -> Result<usize, String> {
    let Some(client) = &state.cardamum else {
        return Ok(0);
    };
    let _permit = state.cli_limit.acquire().await.map_err(|e| e.to_string())?;
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
    let pairs: Vec<(String, String)> = items
        .iter()
        .filter(|c| !c.email.is_empty())
        .map(|c| (c.email.clone(), c.name.clone()))
        .collect();
    let n = pairs.len();
    {
        let cache = state.cache.lock().await;
        cache.save_contacts(&pairs).map_err(|e| e.to_string())?;
    }
    tracing::info!("cache contacts: {n} entrées");
    Ok(n)
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

#[derive(Deserialize)]
pub struct CreateContactForm {
    pub book: String,
    pub name: String,
    pub email: String,
    pub tel: Option<String>,
}

async fn create_contact(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateContactForm>,
) -> impl IntoResponse {
    let Some(client) = &state.cardamum else {
        return Redirect::to("/contacts?msg=Cardamum%20absent").into_response();
    };
    let book = form.book.trim();
    if book.is_empty() || book == "__all__" {
        return Redirect::to("/contacts?msg=Choisissez%20un%20carnet").into_response();
    }
    let vcard = CardamumClient::build_vcard(
        form.name.trim(),
        form.email.trim(),
        form.tel.as_deref().unwrap_or(""),
    );
    let _permit = state.cli_limit.acquire().await.ok();
    match client.create_card(book, vcard.as_bytes()).await {
        Ok(()) => {
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
