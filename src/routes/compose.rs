use std::sync::Arc;

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

use crate::cli::himalaya::ComposeKind;
use crate::prefs::ACCOUNT_ALL;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/compose", get(compose_get))
        .route("/compose/send", post(compose_send))
}

#[derive(Deserialize)]
pub struct ComposeQuery {
    pub kind: Option<String>,
    pub mailbox: Option<String>,
    pub id: Option<String>,
    pub account: Option<String>,
    pub to: Option<String>,
}

#[derive(Template)]
#[template(path = "compose.html")]
struct ComposeTemplate {
    pub title: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    pub accounts: Vec<AccountOpt>,
    pub selected_account: String,
    pub cardamum_available: bool,
    pub compose_init: String,
    pub error: Option<String>,
}

pub struct AccountOpt {
    pub name: String,
    pub email: String,
    pub selected: bool,
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

async fn compose_get(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ComposeQuery>,
) -> impl IntoResponse {
    let mailbox = q.mailbox.clone().unwrap_or_else(|| "Inbox".into());
    let kind = match q.kind.as_deref() {
        Some("reply") => ComposeKind::Reply,
        Some("reply-all") => ComposeKind::ReplyAll,
        Some("forward") => ComposeKind::Forward,
        _ => ComposeKind::New,
    };

    let prefs_snap = state.prefs.lock().await.clone();
    let preferred = q
        .account
        .filter(|s| !s.is_empty() && s != ACCOUNT_ALL)
        .or_else(|| prefs_snap.selected_account().map(str::to_string))
        .unwrap_or_default();

    let mut draft = crate::cli::himalaya::ComposeDraft::default();
    let mut error = None;

    if let Some(to) = q.to.filter(|s| !s.is_empty()) {
        draft.to = to;
    }

    if !matches!(kind, ComposeKind::New) {
        if let Some(id) = q.id.as_deref() {
            let account_ref = if preferred.is_empty() {
                None
            } else {
                Some(preferred.as_str())
            };
            let _permit = state.cli_limit.acquire().await.ok();
            match state
                .himalaya
                .compose_template(kind, &mailbox, Some(id), account_ref)
                .await
            {
                Ok(d) => draft = d,
                Err(e) => {
                    if let Ok(msg) = state
                        .himalaya
                        .read_message(&mailbox, id, account_ref)
                        .await
                    {
                        draft.subject = match kind {
                            ComposeKind::Forward => format!("Fwd: {}", msg.subject),
                            _ => format!("Re: {}", msg.subject),
                        };
                        if !matches!(kind, ComposeKind::Forward) {
                            draft.to = msg.from.clone();
                        }
                        if matches!(kind, ComposeKind::ReplyAll) {
                            draft.cc = msg.cc.clone();
                        }
                        let body_src = if msg.body_text.is_empty() {
                            "(voir HTML)"
                        } else {
                            msg.body_text.as_str()
                        };
                        draft.body = format!(
                            "\n\n----- Message original -----\nDe: {}\nDate: {}\nSujet: {}\n\n{}",
                            msg.from, msg.date, msg.subject, body_src
                        );
                    } else {
                        error = Some(e.to_string());
                    }
                }
            }
        }
    }

    let title = match kind {
        ComposeKind::New => "Nouveau message",
        ComposeKind::Reply => "Répondre",
        ComposeKind::ReplyAll => "Répondre à tous",
        ComposeKind::Forward => "Transférer",
    };

    let accounts = load_account_opts(&state, &preferred).await;

    let selected_account = if preferred.is_empty() {
        accounts
            .iter()
            .find(|a| a.selected)
            .map(|a| a.name.clone())
            .unwrap_or_default()
    } else {
        preferred
    };

    let compose_init = serde_json::json!({
        "cardamum": state.cardamum_available,
        "to": draft.to,
        "cc": draft.cc,
        "bcc": draft.bcc,
    })
    .to_string();

    let inner = ComposeTemplate {
        title: title.into(),
        to: draft.to,
        cc: draft.cc,
        bcc: draft.bcc,
        subject: draft.subject,
        body: draft.body,
        accounts,
        selected_account,
        cardamum_available: state.cardamum_available,
        compose_init,
        error,
    };

    let content = match inner.render() {
        Ok(c) => c,
        Err(e) => format!("<pre>{e}</pre>"),
    };

    let (theme, layout) = state.theme_layout().await;
    let shell = ShellTemplate {
        title: "HimaWeb — Rédaction".into(),
        active_tab: "mail".into(),
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

async fn load_account_opts(state: &AppState, preferred: &str) -> Vec<AccountOpt> {
    let editable = crate::accounts_config::list_editable_accounts().unwrap_or_default();
    let list = if state.himalaya_available {
        let _permit = state.cli_limit.acquire().await.ok();
        state.himalaya.list_accounts().await.unwrap_or_default()
    } else {
        vec![]
    };

    if list.is_empty() && editable.is_empty() {
        return vec![];
    }

    let names: Vec<(String, bool, String)> = if !list.is_empty() {
        list.into_iter()
            .map(|a| {
                let email = editable
                    .iter()
                    .find(|e| e.name == a.name)
                    .map(|e| e.email.clone())
                    .unwrap_or_default();
                (a.name, a.is_default, email)
            })
            .collect()
    } else {
        editable
            .into_iter()
            .map(|a| (a.name, a.is_default, a.email))
            .collect()
    };

    let has_explicit = !preferred.is_empty() && names.iter().any(|(n, _, _)| n == preferred);
    names
        .into_iter()
        .map(|(name, is_default, email)| {
            let selected = if has_explicit {
                name == preferred
            } else {
                is_default
            };
            AccountOpt {
                name,
                email,
                selected,
            }
        })
        .collect()
}

#[derive(Deserialize)]
pub struct SendForm {
    pub account: Option<String>,
    pub to: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub subject: String,
    pub body: String,
}

async fn compose_send(
    State(state): State<Arc<AppState>>,
    Form(form): Form<SendForm>,
) -> impl IntoResponse {
    if form.to.trim().is_empty() {
        return Html(r#"<div class="error">Destinataire requis</div>"#).into_response();
    }

    let account = form
        .account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != ACCOUNT_ALL)
        .map(str::to_string)
        .or(state.account().await);

    let _permit = state.cli_limit.acquire().await.ok();
    match state
        .himalaya
        .send_message(
            form.to.trim(),
            form.cc.as_deref(),
            form.bcc.as_deref(),
            &form.subject,
            &form.body,
            account.as_deref(),
        )
        .await
    {
        Ok(()) => Redirect::to("/").into_response(),
        Err(e) => {
            let eml = build_eml(&form);
            match state
                .himalaya
                .send_raw_eml(eml.as_bytes(), account.as_deref())
                .await
            {
                Ok(()) => Redirect::to("/").into_response(),
                Err(e2) => Html(format!(
                    r#"<div class="error">Envoi échoué: {e} / {e2}</div>
               <p><a href="/compose">Retour</a></p>"#
                ))
                .into_response(),
            }
        }
    }
}

fn build_eml(form: &SendForm) -> String {
    let mut headers = String::new();
    headers.push_str(&format!("To: {}\r\n", form.to.trim()));
    if let Some(cc) = &form.cc {
        if !cc.trim().is_empty() {
            headers.push_str(&format!("Cc: {}\r\n", cc.trim()));
        }
    }
    if let Some(bcc) = &form.bcc {
        if !bcc.trim().is_empty() {
            headers.push_str(&format!("Bcc: {}\r\n", bcc.trim()));
        }
    }
    headers.push_str(&format!("Subject: {}\r\n", form.subject));
    headers.push_str("MIME-Version: 1.0\r\n");
    headers.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    headers.push_str("Content-Transfer-Encoding: 8bit\r\n");
    headers.push_str("\r\n");
    headers.push_str(&form.body.replace('\n', "\r\n"));
    if !headers.ends_with("\r\n") {
        headers.push_str("\r\n");
    }
    headers
}
