use std::sync::Arc;

use askama::Template;
use axum::extract::{RawQuery, State};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;

use super::mail::{search_query_tokens, SortSpec};
use super::pages::{render_shell, ShellTemplate};
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/search", get(search_page))
        .route("/partials/search-results", get(search_results))
}

#[derive(Template)]
#[template(path = "search.html")]
struct SearchPageTemplate {
    pub query: String,
    pub accounts: Vec<AccountOpt>,
    pub folders: Vec<FolderOpt>,
    pub all_folders: bool,
    pub include_body: bool,
    pub sort: String,
    pub results_url: String,
}

pub struct AccountOpt {
    pub name: String,
    pub label: String,
    pub icon: String,
    pub color: String,
    pub selected: bool,
}

pub struct FolderOpt {
    pub name: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Template)]
#[template(path = "search_results.html")]
struct SearchResultsTemplate {
    pub query: String,
    pub envelopes: Vec<SearchHit>,
    pub error: Option<String>,
    pub searched: bool,
}

pub struct SearchHit {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub from_initial: String,
    pub date: String,
    pub date_short: String,
    pub unread: bool,
    pub has_attachment: bool,
    pub account: String,
    pub account_enc: String,
    pub account_label: String,
    pub account_icon: String,
    pub mailbox: String,
    pub mailbox_enc: String,
    pub mailbox_label: String,
    pub color: String,
}

const DEFAULT_FOLDERS: &[&str] = &["Inbox", "Sent"];
const FOLDER_CHOICES: &[&str] = &["Inbox", "Sent", "Drafts", "Archive", "Trash"];

fn mailbox_label(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
}

fn short_date(date: &str) -> String {
    if date.len() >= 16 {
        let day = &date[0..10];
        let time = date.get(11..16).unwrap_or("");
        format!("{day} {time}")
    } else {
        date.to_string()
    }
}

fn query_values(raw: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.splitn(2, '=');
        let k = it.next().unwrap_or("");
        let v = it.next().unwrap_or("");
        let k = urlencoding::decode(k).unwrap_or_else(|_| k.into());
        if k.as_ref() != key {
            continue;
        }
        let decoded = urlencoding::decode(v)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| v.to_string());
        let trimmed = decoded.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn query_one(raw: &str, key: &str) -> Option<String> {
    query_values(raw, key).into_iter().next()
}

fn has_flag(raw: &str, key: &str) -> bool {
    query_values(raw, key)
        .iter()
        .any(|v| v == "1" || v == "true" || v == "on")
}

fn match_mailbox(available: &[crate::cli::himalaya::Mailbox], wanted: &str) -> Option<String> {
    let wl = wanted.to_ascii_lowercase();
    available
        .iter()
        .find(|m| {
            m.name.eq_ignore_ascii_case(wanted)
                || mailbox_label(&m.name).eq_ignore_ascii_case(wanted)
                || m.name.to_ascii_lowercase().ends_with(&format!("/{wl}"))
        })
        .map(|m| m.name.clone())
}

fn results_query_string(
    query: &str,
    accounts: &[AccountOpt],
    folders: &[FolderOpt],
    all_folders: bool,
    include_body: bool,
    sort: SortSpec,
) -> String {
    let mut parts = Vec::new();
    if !query.is_empty() {
        parts.push(format!("q={}", urlencoding::encode(query)));
    }
    for a in accounts.iter().filter(|a| a.selected) {
        parts.push(format!("account={}", urlencoding::encode(&a.name)));
    }
    if all_folders {
        parts.push("all_folders=1".into());
    } else {
        for f in folders.iter().filter(|f| f.selected) {
            parts.push(format!("folder={}", urlencoding::encode(&f.name)));
        }
    }
    if include_body {
        parts.push("body=1".into());
    }
    if !sort.is_default() {
        parts.push(format!("sort={}", sort.as_str()));
    }
    parts.join("&")
}

async fn search_page(
    State(state): State<Arc<AppState>>,
    RawQuery(raw): RawQuery,
) -> impl IntoResponse {
    let raw = raw.unwrap_or_default();
    let query = query_one(&raw, "q").unwrap_or_default();
    let prefs = state.prefs.lock().await.clone();
    let (theme, layout) = state.theme_layout().await;

    let account_infos = if state.himalaya_available {
        state.himalaya.list_accounts().await.unwrap_or_default()
    } else {
        vec![]
    };
    let known: Vec<String> = account_infos.iter().map(|a| a.name.clone()).collect();
    let ordered = prefs.ordered_accounts(&known);

    let selected_accounts = query_values(&raw, "account");
    let default_all_accounts = selected_accounts.is_empty();

    let accounts: Vec<AccountOpt> = ordered
        .iter()
        .map(|name| AccountOpt {
            color: prefs.account_color(name),
            label: prefs.account_label(name),
            icon: prefs.account_icon(name),
            selected: default_all_accounts || selected_accounts.iter().any(|a| a == name),
            name: name.clone(),
        })
        .collect();

    let selected_folders = query_values(&raw, "folder");
    let all_folders = has_flag(&raw, "all_folders");
    let include_body = has_flag(&raw, "body");
    let sort = SortSpec::parse(query_one(&raw, "sort").as_deref());
    let default_folders = selected_folders.is_empty() && !all_folders;

    let folders: Vec<FolderOpt> = FOLDER_CHOICES
        .iter()
        .map(|name| FolderOpt {
            label: (*name).into(),
            selected: if default_folders {
                DEFAULT_FOLDERS.contains(name)
            } else {
                !all_folders && selected_folders.iter().any(|f| f.eq_ignore_ascii_case(name))
            },
            name: (*name).into(),
        })
        .collect();

    let qs = results_query_string(&query, &accounts, &folders, all_folders, include_body, sort);
    let results_url = if qs.is_empty() {
        "/partials/search-results".into()
    } else {
        format!("/partials/search-results?{qs}")
    };

    let inner = SearchPageTemplate {
        query,
        accounts,
        folders,
        all_folders,
        include_body,
        sort: sort.as_str().into(),
        results_url,
    };
    let content = match inner.render() {
        Ok(h) => h,
        Err(e) => format!("<pre>template error: {e}</pre>"),
    };

    render_shell(
        &state,
        ShellTemplate {
            title: "HimaWeb — Recherche".into(),
            active_tab: "search".into(),
            offline: false,
            himalaya_available: state.himalaya_available,
            calendula_available: state.calendula_available,
            cardamum_available: state.cardamum_available,
            theme,
            layout,
            ui_style: state.ui_style().await,
            error: None,
            content,
        },
    )
    .await
}

fn is_low_value_folder(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("trash")
        || n.contains("corbeille")
        || n.contains("junk")
        || n.contains("spam")
        || n.contains("bin")
        || n.ends_with("/deleted items")
}

fn hit_from_envelope(
    e: &crate::cli::himalaya::Envelope,
    account: &str,
    mailbox: &str,
    color: &str,
    account_label: &str,
    account_icon: &str,
) -> SearchHit {
    let unread = !e.flags.iter().any(|f| {
        let x = f.to_ascii_lowercase();
        x == "seen" || x == "\\seen"
    });
    SearchHit {
        id: e.id.clone(),
        subject: e.subject.clone(),
        from_initial: e
            .from
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string(),
        from: e.from.clone(),
        to: e.to.clone(),
        date: e.date.clone(),
        date_short: short_date(&e.date),
        unread,
        has_attachment: e.has_attachment,
        account: account.to_string(),
        account_enc: urlencoding::encode(account).into_owned(),
        account_label: account_label.to_string(),
        account_icon: account_icon.to_string(),
        mailbox: mailbox.to_string(),
        mailbox_enc: urlencoding::encode(mailbox).into_owned(),
        mailbox_label: mailbox_label(mailbox),
        color: color.to_string(),
    }
}

fn sort_hits(hits: &mut [SearchHit], sort: SortSpec) {
    use super::mail::{SortDir, SortField};
    hits.sort_by(|a, b| {
        let ord = match sort.field {
            SortField::Date => a.date.cmp(&b.date),
            SortField::From => a
                .from
                .to_ascii_lowercase()
                .cmp(&b.from.to_ascii_lowercase()),
            SortField::To => a.to.to_ascii_lowercase().cmp(&b.to.to_ascii_lowercase()),
        };
        match sort.dir {
            SortDir::Asc => ord,
            SortDir::Desc => ord.reverse(),
        }
    });
}

async fn search_results(
    State(state): State<Arc<AppState>>,
    RawQuery(raw): RawQuery,
) -> impl IntoResponse {
    let raw = raw.unwrap_or_default();
    let query = query_one(&raw, "q").unwrap_or_default();
    let include_body = has_flag(&raw, "body");
    let sort = SortSpec::parse(query_one(&raw, "sort").as_deref());
    let filter_tokens = search_query_tokens(&query, include_body);
    let started = std::time::Instant::now();

    if filter_tokens.is_empty() {
        let html = SearchResultsTemplate {
            query,
            envelopes: vec![],
            error: None,
            searched: false,
        }
        .render()
        .unwrap_or_else(|e| format!("<pre>{e}</pre>"));
        return Html(html).into_response();
    }

    let mut tokens = filter_tokens;
    tokens.extend(sort.order_tokens());

    if !state.himalaya_available {
        let html = SearchResultsTemplate {
            query,
            envelopes: vec![],
            error: Some("Himalaya est introuvable.".into()),
            searched: true,
        }
        .render()
        .unwrap_or_else(|e| format!("<pre>{e}</pre>"));
        return Html(html).into_response();
    }

    let prefs = state.prefs.lock().await.clone();
    let account_infos = state.himalaya.list_accounts().await.unwrap_or_default();
    let known: Vec<String> = account_infos.iter().map(|a| a.name.clone()).collect();
    let ordered = prefs.ordered_accounts(&known);

    let selected_accounts = query_values(&raw, "account");
    let targets: Vec<String> = if selected_accounts.is_empty() {
        ordered
    } else {
        ordered
            .into_iter()
            .filter(|a| selected_accounts.iter().any(|s| s == a))
            .collect()
    };

    let all_folders = has_flag(&raw, "all_folders");
    let mut wanted_folders = query_values(&raw, "folder");
    if wanted_folders.is_empty() && !all_folders {
        wanted_folders = DEFAULT_FOLDERS.iter().map(|s| (*s).to_string()).collect();
    }

    tracing::info!(
        query = %query,
        accounts = targets.len(),
        folders = ?wanted_folders,
        all_folders,
        include_body,
        "mail search start"
    );

    // Résoudre les dossiers par compte (en parallèle)
    let mut jobs: Vec<(String, String, String, String, String)> = Vec::new(); // acc, mb, color, label, icon
    let mut errs: Vec<String> = Vec::new();
    {
        let mut set = tokio::task::JoinSet::new();
        for acc in targets {
            let state = Arc::clone(&state);
            let color = prefs.account_color(&acc);
            let label = prefs.account_label(&acc);
            let icon = prefs.account_icon(&acc);
            let wanted = wanted_folders.clone();
            set.spawn(async move {
                let permit = state.cli_limit.acquire().await.ok();
                let boxes = state.himalaya.list_mailboxes(Some(&acc)).await;
                drop(permit);
                (acc, color, label, icon, boxes, wanted)
            });
        }
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((acc, color, label, icon, Ok(boxes), wanted)) => {
                    let mailboxes: Vec<String> = if all_folders {
                        boxes
                            .iter()
                            .map(|m| m.name.clone())
                            .filter(|n| !is_low_value_folder(n))
                            .collect()
                    } else {
                        wanted
                            .iter()
                            .filter_map(|w| match_mailbox(&boxes, w).or_else(|| Some(w.clone())))
                            .collect()
                    };
                    for mailbox in mailboxes {
                        jobs.push((
                            acc.clone(),
                            mailbox,
                            color.clone(),
                            label.clone(),
                            icon.clone(),
                        ));
                    }
                }
                Ok((acc, _, _, _, Err(e), _)) => errs.push(format!("{acc}: {e}")),
                Err(e) => errs.push(format!("tâche list: {e}")),
            }
        }
    }

    let page_size = 30u32;
    let tokens = Arc::new(tokens);
    let mut set = tokio::task::JoinSet::new();
    let searches = jobs.len() as u32;

    for (acc, mailbox, color, label, icon) in jobs {
        let state = Arc::clone(&state);
        let tokens = Arc::clone(&tokens);
        set.spawn(async move {
            let _permit = state.cli_limit.acquire().await.ok();
            let token_refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
            let res = state
                .himalaya
                .search_envelopes(&mailbox, &token_refs, 1, page_size, Some(&acc))
                .await;
            (acc, mailbox, color, label, icon, res)
        });
    }

    let mut hits: Vec<SearchHit> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((acc, mailbox, color, label, icon, Ok(list))) => {
                for e in list {
                    let key = format!("{acc}::{mailbox}::{}", e.id);
                    if seen.insert(key) {
                        hits.push(hit_from_envelope(
                            &e, &acc, &mailbox, &color, &label, &icon,
                        ));
                    }
                }
            }
            Ok((acc, mailbox, _, _, _, Err(e))) => {
                let msg = e.to_string();
                if !msg.to_ascii_lowercase().contains("not found")
                    && !msg.to_ascii_lowercase().contains("no such")
                {
                    errs.push(format!("{acc}/{mailbox}: {msg}"));
                }
            }
            Err(e) => errs.push(format!("tâche search: {e}")),
        }
    }

    sort_hits(&mut hits, sort);
    hits.truncate(150);

    tracing::info!(
        hits = hits.len(),
        searches,
        elapsed_ms = started.elapsed().as_millis() as u64,
        errs = errs.len(),
        "mail search done"
    );

    let error = if errs.is_empty() {
        None
    } else {
        Some(errs.join(" · "))
    };

    let html = SearchResultsTemplate {
        query,
        envelopes: hits,
        error,
        searched: true,
    }
    .render()
    .unwrap_or_else(|e| format!("<pre>{e}</pre>"));
    Html(html).into_response()
}
