//! Routes Phase 4 : mises à jour, i18n, assets plugins, RSS / FreshRSS.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/updates/check", get(updates_check))
        .route("/api/i18n", get(i18n_catalog))
        .route("/api/rss/items", get(rss_items))
        .route("/api/rss/mark-read", post(rss_mark_read))
        .route("/api/rss/star", post(rss_star))
        .route("/api/rss/mark-all-read", post(rss_mark_all_read))
        .route("/plugins/{id}/{*path}", get(plugin_asset))
        .route("/settings/locale", post(save_locale))
        .route("/settings/rss", post(save_rss))
        .route("/settings/freshrss", post(save_freshrss))
        .route("/settings/builtin-plugins", post(save_builtin_plugins))
}

#[derive(Deserialize)]
struct CheckQuery {
    force: Option<String>,
}

async fn updates_check(Query(q): Query<CheckQuery>) -> impl IntoResponse {
    let force = q.force.as_deref() == Some("1") || q.force.as_deref() == Some("true");
    let info = crate::updates::check(force).await;
    Json(info)
}

async fn i18n_catalog(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let locale = state.prefs.lock().await.locale.clone();
    let loc = crate::i18n::normalize_locale(&locale);
    let strings = crate::i18n::catalog_for(loc);
    Json(serde_json::json!({
        "locale": loc,
        "strings": strings,
    }))
}

#[derive(Deserialize)]
struct RssQuery {
    limit: Option<usize>,
    filter: Option<String>,
}

fn rss_item_json(i: &crate::plugins::RssItem) -> serde_json::Value {
    serde_json::json!({
        "id": i.id,
        "title": i.title,
        "link": i.link,
        "feed": i.feed_title,
        "unread": i.unread,
        "starred": i.starred,
    })
}

fn freshrss_creds(prefs: &crate::prefs::Prefs) -> Option<crate::freshrss::FreshRssCreds> {
    if prefs.plugin_freshrss
        && crate::freshrss::is_configured(
            &prefs.freshrss_url,
            &prefs.freshrss_user,
            &prefs.freshrss_api_password,
        )
    {
        Some(crate::freshrss::FreshRssCreds {
            api_base: prefs.freshrss_url.clone(),
            user: prefs.freshrss_user.clone(),
            api_password: prefs.freshrss_api_password.clone(),
        })
    } else {
        None
    }
}

async fn rss_items(
    State(state): State<Arc<AppState>>,
    Query(q): Query<RssQuery>,
) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    let limit = q.limit.unwrap_or(10).clamp(1, 40);
    let filter = crate::freshrss::RssFilter::parse(q.filter.as_deref().unwrap_or("unread"));

    // Priorité FreshRSS si activé + configuré
    if let Some(creds) = freshrss_creds(&prefs) {
        match crate::freshrss::fetch_items(&creds, limit, filter).await {
            Ok(items) => {
                return Json(serde_json::json!({
                    "source": "freshrss",
                    "filter": match filter {
                        crate::freshrss::RssFilter::Unread => "unread",
                        crate::freshrss::RssFilter::Starred => "starred",
                        crate::freshrss::RssFilter::All => "all",
                    },
                    "items": items.iter().map(rss_item_json).collect::<Vec<_>>(),
                }));
            }
            Err(e) => {
                return Json(serde_json::json!({
                    "source": "freshrss",
                    "error": e,
                    "items": [],
                }));
            }
        }
    }

    if !prefs.plugin_rss {
        return Json(serde_json::json!({
            "source": "none",
            "items": [],
        }));
    }
    let feeds = prefs.rss_feeds;
    if feeds.is_empty() {
        return Json(serde_json::json!({
            "source": "none",
            "items": [],
        }));
    }
    let per = (limit / feeds.len().max(1)).max(2);
    let mut join = tokio::task::JoinSet::new();
    for f in feeds {
        let url = f.url.clone();
        let title = if f.title.trim().is_empty() {
            f.url.clone()
        } else {
            f.title.clone()
        };
        join.spawn(async move { crate::plugins::fetch_rss(&url, &title, per).await });
    }
    let mut items = Vec::new();
    while let Some(res) = join.join_next().await {
        if let Ok(Ok(mut list)) = res {
            items.append(&mut list);
        }
    }
    items.truncate(limit);
    Json(serde_json::json!({
        "source": "rss",
        "items": items.iter().map(rss_item_json).collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
struct RssItemAction {
    id: String,
    #[serde(default)]
    value: Option<String>,
}

async fn rss_mark_read(
    State(state): State<Arc<AppState>>,
    Form(form): Form<RssItemAction>,
) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    let Some(creds) = freshrss_creds(&prefs) else {
        return Json(serde_json::json!({ "ok": false, "error": "FreshRSS non configuré" }));
    };
    let read = form.value.as_deref() != Some("0");
    match crate::freshrss::set_read(&creds, &form.id, read).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

async fn rss_star(
    State(state): State<Arc<AppState>>,
    Form(form): Form<RssItemAction>,
) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    let Some(creds) = freshrss_creds(&prefs) else {
        return Json(serde_json::json!({ "ok": false, "error": "FreshRSS non configuré" }));
    };
    let starred = form.value.as_deref() != Some("0");
    match crate::freshrss::set_starred(&creds, &form.id, starred).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

async fn rss_mark_all_read(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    let Some(creds) = freshrss_creds(&prefs) else {
        return Json(serde_json::json!({ "ok": false, "error": "FreshRSS non configuré" }));
    };
    match crate::freshrss::mark_all_read(&creds).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

async fn plugin_asset(
    Path((id, path)): Path<(String, String)>,
) -> impl IntoResponse {
    match crate::plugins::plugin_file(&id, &path) {
        Ok((file_path, bytes)) => {
            let mime = mime_guess::from_path(&file_path)
                .first_or_octet_stream()
                .essence_str()
                .to_string();
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_str(&mime)
                    .unwrap_or_else(|_| header::HeaderValue::from_static("application/octet-stream")),
            );
            (StatusCode::OK, headers, bytes).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

#[derive(Deserialize)]
struct LocaleForm {
    locale: String,
}

async fn save_locale(
    State(state): State<Arc<AppState>>,
    Form(form): Form<LocaleForm>,
) -> impl IntoResponse {
    {
        let mut p = state.prefs.lock().await;
        p.locale = crate::i18n::normalize_locale(&form.locale).to_string();
        let _ = p.save();
    }
    axum::response::Redirect::to("/settings#apparence")
}

#[derive(Deserialize)]
struct RssForm {
    /// Lignes `titre|url` ou juste `url`
    feeds: String,
}

async fn save_rss(
    State(state): State<Arc<AppState>>,
    Form(form): Form<RssForm>,
) -> impl IntoResponse {
    let mut feeds = Vec::new();
    for line in form.feeds.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((title, url)) = line.split_once('|') {
            let url = url.trim();
            if url.starts_with("http://") || url.starts_with("https://") {
                feeds.push(crate::prefs::RssFeed {
                    title: title.trim().to_string(),
                    url: url.to_string(),
                });
            }
        } else if line.starts_with("http://") || line.starts_with("https://") {
            feeds.push(crate::prefs::RssFeed {
                title: String::new(),
                url: line.to_string(),
            });
        }
    }
    {
        let mut p = state.prefs.lock().await;
        p.rss_feeds = feeds;
        let _ = p.save();
    }
    axum::response::Redirect::to("/settings#plugins")
}

#[derive(Deserialize)]
struct FreshRssForm {
    freshrss_url: String,
    freshrss_user: String,
    freshrss_api_password: String,
    clear: Option<String>,
}

async fn save_freshrss(
    State(state): State<Arc<AppState>>,
    Form(form): Form<FreshRssForm>,
) -> impl IntoResponse {
    {
        let mut p = state.prefs.lock().await;
        if form.clear.as_deref() == Some("1") {
            p.freshrss_url.clear();
            p.freshrss_user.clear();
            p.freshrss_api_password.clear();
        } else {
            let url = crate::freshrss::normalize_api_base(&form.freshrss_url);
            p.freshrss_url = if url.is_empty()
                || url.starts_with("http://")
                || url.starts_with("https://")
            {
                url
            } else {
                String::new()
            };
            p.freshrss_user = form.freshrss_user.trim().to_string();
            let pass = form.freshrss_api_password.trim();
            if !pass.is_empty() {
                p.freshrss_api_password = pass.to_string();
            }
            // Si URL/user vidés → wipe password aussi
            if p.freshrss_url.is_empty() || p.freshrss_user.is_empty() {
                p.freshrss_api_password.clear();
            }
        }
        let _ = p.save();
    }
    axum::response::Redirect::to("/settings#plugins")
}

#[derive(Deserialize)]
struct BuiltinPluginsForm {
    plugin_ntfy: Option<String>,
    plugin_freshrss: Option<String>,
    plugin_rss: Option<String>,
}

async fn save_builtin_plugins(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Form(form): Form<BuiltinPluginsForm>,
) -> impl IntoResponse {
    let quiet = headers.get("HX-Request").is_some();
    let on = |v: &Option<String>| v.as_deref() == Some("1") || v.as_deref() == Some("on");
    {
        let mut p = state.prefs.lock().await;
        p.plugin_ntfy = on(&form.plugin_ntfy);
        p.plugin_freshrss = on(&form.plugin_freshrss);
        p.plugin_rss = on(&form.plugin_rss);
        let _ = p.save();
    }
    if quiet {
        return axum::http::StatusCode::NO_CONTENT.into_response();
    }
    axum::response::Redirect::to("/settings#plugins").into_response()
}
