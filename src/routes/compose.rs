use std::sync::Arc;

use askama::Template;
use axum::extract::{Multipart, Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::Router;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;

use crate::cli::himalaya::ComposeKind;
use crate::prefs::ACCOUNT_ALL;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/compose", get(compose_get))
        .route("/compose/send", post(compose_send))
        .route("/compose/draft", post(compose_draft))
}

#[derive(Deserialize)]
pub struct ComposeQuery {
    pub kind: Option<String>,
    pub mailbox: Option<String>,
    pub id: Option<String>,
    pub account: Option<String>,
    pub to: Option<String>,
    pub body: Option<String>,
    pub subject: Option<String>,
    /// Si `1` : fragment seul (overlay mail), sans shell
    pub embed: Option<String>,
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
    /// JSON meta sans le corps (évite de casser x-data)
    pub compose_boot: String,
    pub error: Option<String>,
}

pub struct AccountOpt {
    pub name: String,
    pub email: String,
    pub selected: bool,
    pub icon: String,
    pub color: String,
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
    pub topbar_mode: String,
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
    if let Some(body) = q.body.filter(|s| !s.is_empty()) {
        draft.body = body;
    }
    if let Some(subject) = q.subject.filter(|s| !s.is_empty()) {
        draft.subject = subject;
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
                            ComposeKind::Forward => {
                                format!("Fwd: {}", crate::cli::himalaya::decode_rfc2047(&msg.subject))
                            }
                            _ => format!("Re: {}", crate::cli::himalaya::decode_rfc2047(&msg.subject)),
                        };
                        if !matches!(kind, ComposeKind::Forward) {
                            draft.to = crate::cli::himalaya::normalize_addr_header(&msg.from);
                        }
                        if matches!(kind, ComposeKind::ReplyAll) {
                            draft.cc = crate::cli::himalaya::normalize_addr_header(&msg.cc);
                        }
                        let body_src = if msg.body_text.is_empty() {
                            "(voir HTML)".to_string()
                        } else {
                            crate::cli::himalaya::decode_quoted_printable(&msg.body_text)
                        };
                        draft.body = format!(
                            "\n\n----- Message original -----\nDe: {}\nDate: {}\nSujet: {}\n\n{}",
                            crate::cli::himalaya::decode_rfc2047(&msg.from),
                            msg.date,
                            crate::cli::himalaya::decode_rfc2047(&msg.subject),
                            body_src
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

    let compose_boot = serde_json::json!({
        "cardamum": state.cardamum_available,
        "ai": prefs_snap.ai_enabled,
        "kind": match kind {
            ComposeKind::Reply | ComposeKind::ReplyAll => "reply",
            _ => "compose",
        },
        "to": draft.to,
        "cc": draft.cc,
        "bcc": draft.bcc,
        "subject": draft.subject,
        "account": selected_account,
        "accounts": accounts.iter().map(|a| serde_json::json!({
            "name": a.name,
            "email": a.email,
            "icon": a.icon,
            "color": a.color,
        })).collect::<Vec<_>>(),
    })
    .to_string();

    let inner = ComposeTemplate {
        title: title.into(),
        to: draft.to.clone(),
        cc: draft.cc.clone(),
        bcc: draft.bcc.clone(),
        subject: draft.subject.clone(),
        body: draft.body,
        accounts,
        selected_account,
        cardamum_available: state.cardamum_available,
        compose_boot,
        error,
    };

    let content = match inner.render() {
        Ok(c) => c,
        Err(e) => format!("<pre>{e}</pre>"),
    };

    let embed =
        q.embed.as_deref() == Some("1") || q.embed.as_deref() == Some("true");
    if embed {
        return Html(content).into_response();
    }

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

async fn load_account_opts(state: &AppState, preferred: &str) -> Vec<AccountOpt> {
    let prefs = state.prefs.lock().await.clone();
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
            let icon = prefs.account_icon(&name);
            let color = prefs.account_color(&name);
            AccountOpt {
                name,
                email,
                selected,
                icon,
                color,
            }
        })
        .collect()
}



#[derive(Default)]
struct ParsedCompose {
    account: Option<String>,
    to: String,
    cc: Option<String>,
    bcc: Option<String>,
    subject: String,
    body: String,
    body_html: Option<String>,
    html: Option<String>,
    files: Vec<(String, Vec<u8>)>,
}

fn form_wants_html(form: &ParsedCompose) -> bool {
    matches!(form.html.as_deref(), Some("1") | Some("true"))
}

async fn parse_compose_multipart(mut multipart: Multipart) -> Result<ParsedCompose, String> {
    let mut out = ParsedCompose::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| format!("multipart: {e}"))?
    {
        let name = field.name().unwrap_or("").to_string();
        let filename = field.file_name().map(|s| s.to_string());
        let data = field.bytes().await.map_err(|e| format!("champ {name}: {e}"))?;
        match name.as_str() {
            "account" => {
                let s = String::from_utf8_lossy(&data).trim().to_string();
                if !s.is_empty() {
                    out.account = Some(s);
                }
            }
            "to" => out.to = String::from_utf8_lossy(&data).to_string(),
            "cc" => {
                let s = String::from_utf8_lossy(&data).to_string();
                if !s.trim().is_empty() {
                    out.cc = Some(s);
                }
            }
            "bcc" => {
                let s = String::from_utf8_lossy(&data).to_string();
                if !s.trim().is_empty() {
                    out.bcc = Some(s);
                }
            }
            "subject" => out.subject = String::from_utf8_lossy(&data).to_string(),
            "body" => out.body = String::from_utf8_lossy(&data).to_string(),
            "body_html" => {
                let s = String::from_utf8_lossy(&data).to_string();
                if !s.trim().is_empty() {
                    out.body_html = Some(s);
                }
            }
            "html" => {
                let s = String::from_utf8_lossy(&data).trim().to_string();
                if !s.is_empty() {
                    out.html = Some(s);
                }
            }
            "attachments" => {
                if let Some(name) = filename.filter(|s| !s.is_empty()) {
                    if !data.is_empty() {
                        out.files.push((name, data.to_vec()));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

async fn compose_send(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> impl IntoResponse {
    let form = match parse_compose_multipart(multipart).await {
        Ok(f) => f,
        Err(e) => {
            return Html(format!(r#"<div class="error">{e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#))
                .into_response();
        }
    };
    finish_send(state, form).await
}

async fn finish_send(state: Arc<AppState>, form: ParsedCompose) -> axum::response::Response {
    let to = match crate::cli::himalaya::smtp_address_list(&form.to) {
        Ok(t) => t,
        Err(e) => {
            return Html(format!(r#"<div class="error">Destinataire : {e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#))
                .into_response();
        }
    };
    let cc = match form.cc.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => match crate::cli::himalaya::smtp_address_list(raw) {
            Ok(c) => Some(c),
            Err(e) => {
                return Html(format!(r#"<div class="error">Cc : {e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#))
                    .into_response();
            }
        },
        None => None,
    };
    let bcc = match form.bcc.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => match crate::cli::himalaya::smtp_address_list(raw) {
            Ok(c) => Some(c),
            Err(e) => {
                return Html(format!(r#"<div class="error">Cci : {e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#))
                    .into_response();
            }
        },
        None => None,
    };

    let account = form
        .account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != ACCOUNT_ALL)
        .map(str::to_string)
        .or(state.account().await);

    let from = resolve_from_header(account.as_deref());
    let as_html = form_wants_html(&form);
    let has_files = !form.files.is_empty();

    let _permit = state.cli_limit.acquire().await.ok();

    // PJ ou HTML : EML multipart (himalaya compose = plain sans fichiers)
    if as_html || has_files {
        let eml = build_eml(&form, from.as_deref().unwrap_or(""), &to, cc.as_deref(), bcc.as_deref(), &form.files);
        return match state
            .himalaya
            .send_raw_eml(eml.as_bytes(), account.as_deref())
            .await
        {
            Ok(()) => Redirect::to("/").into_response(),
            Err(e) => Html(format!(
                r#"<div class="error">Envoi échoué : {e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#
            ))
            .into_response(),
        };
    }

    match state
        .himalaya
        .send_message(
            &to,
            cc.as_deref(),
            bcc.as_deref(),
            &form.subject,
            &form.body,
            account.as_deref(),
            from.as_deref(),
        )
        .await
    {
        Ok(()) => Redirect::to("/").into_response(),
        Err(e) => {
            let eml = build_eml(
                &form,
                from.as_deref().unwrap_or(""),
                &to,
                cc.as_deref(),
                bcc.as_deref(),
                &form.files,
            );
            match state
                .himalaya
                .send_raw_eml(eml.as_bytes(), account.as_deref())
                .await
            {
                Ok(()) => Redirect::to("/").into_response(),
                Err(e2) => Html(format!(
                    r#"<div class="error">Envoi échoué : {e} / {e2}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#
                ))
                .into_response(),
            }
        }
    }
}

async fn compose_draft(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> impl IntoResponse {
    let form = match parse_compose_multipart(multipart).await {
        Ok(f) => f,
        Err(e) => {
            return Html(format!(r#"<div class="error">{e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#))
                .into_response();
        }
    };
    finish_draft(state, form).await
}

async fn finish_draft(state: Arc<AppState>, form: ParsedCompose) -> axum::response::Response {
    let to = match crate::cli::himalaya::smtp_address_list(&form.to) {
        Ok(t) => t,
        Err(e) => {
            return Html(format!(r#"<div class="error">Destinataire : {e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#))
                .into_response();
        }
    };
    let cc = match form.cc.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => match crate::cli::himalaya::smtp_address_list(raw) {
            Ok(c) => Some(c),
            Err(e) => {
                return Html(format!(r#"<div class="error">Cc : {e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#))
                    .into_response();
            }
        },
        None => None,
    };
    let bcc = match form.bcc.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => match crate::cli::himalaya::smtp_address_list(raw) {
            Ok(c) => Some(c),
            Err(e) => {
                return Html(format!(r#"<div class="error">Cci : {e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#))
                    .into_response();
            }
        },
        None => None,
    };

    let account = form
        .account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != ACCOUNT_ALL)
        .map(str::to_string)
        .or(state.account().await);

    let editable = crate::accounts_config::list_editable_accounts().unwrap_or_default();
    let drafts_mailbox = account
        .as_deref()
        .and_then(|name| editable.iter().find(|a| a.name == name))
        .map(|a| a.drafts_alias.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "drafts".into());

    let from = resolve_from_header(account.as_deref());
    let as_html = form_wants_html(&form);
    let has_files = !form.files.is_empty();

    let _permit = state.cli_limit.acquire().await.ok();
    let saved = if as_html || has_files {
        let eml = build_eml(
            &form,
            from.as_deref().unwrap_or(""),
            &to,
            cc.as_deref(),
            bcc.as_deref(),
            &form.files,
        );
        state
            .himalaya
            .save_raw_draft(eml.as_bytes(), &drafts_mailbox, account.as_deref())
            .await
    } else {
        match state
            .himalaya
            .save_draft(
                &to,
                cc.as_deref(),
                bcc.as_deref(),
                &form.subject,
                &form.body,
                account.as_deref(),
                from.as_deref(),
                &drafts_mailbox,
            )
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => {
                let eml = build_eml(
                    &form,
                    from.as_deref().unwrap_or(""),
                    &to,
                    cc.as_deref(),
                    bcc.as_deref(),
                    &form.files,
                );
                match state
                    .himalaya
                    .save_raw_draft(eml.as_bytes(), &drafts_mailbox, account.as_deref())
                    .await
                {
                    Ok(()) => Ok(()),
                    Err(e2) => Err(crate::cli::runner::CliError::Message(format!("{e} / {e2}"))),
                }
            }
        }
    };

    match saved {
        Ok(()) => Redirect::to("/").into_response(),
        Err(e) => Html(format!(
            r#"<div class="error">Brouillon échoué : {e}</div>
               <p><a href="javascript:history.back()">Retour</a></p>"#
        ))
        .into_response(),
    }
}

fn resolve_from_header(account: Option<&str>) -> Option<String> {
    let editable = crate::accounts_config::list_editable_accounts().unwrap_or_default();
    account
        .and_then(|name| editable.into_iter().find(|a| a.name == name))
        .map(|a| {
            if a.display_name.is_empty() {
                a.email.clone()
            } else if a.email.is_empty() {
                a.display_name.clone()
            } else {
                format!("{} <{}>", a.display_name, a.email)
            }
        })
        .filter(|s| s.contains('@'))
}

fn build_eml(
    form: &ParsedCompose,
    from: &str,
    to: &str,
    cc: Option<&str>,
    bcc: Option<&str>,
    files: &[(String, Vec<u8>)],
) -> String {
    let subject = form.subject.trim();
    let subject_hdr = if subject.is_ascii() && !subject.contains(['\r', '\n']) {
        subject.to_string()
    } else {
        encode_rfc2047(subject)
    };

    let wants_html = form_wants_html(form);
    let html_body = form
        .body_html
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            if wants_html {
                Some(format!(
                    "<pre style=\"font-family:inherit;white-space:pre-wrap\">{}</pre>",
                    html_escape(&form.body)
                ))
            } else {
                None
            }
        });

    let mut headers = String::new();
    if !from.trim().is_empty() {
        headers.push_str(&format!("From: {}\r\n", from.trim()));
    }
    headers.push_str(&format!("To: {to}\r\n"));
    if let Some(cc) = cc.filter(|s| !s.is_empty()) {
        headers.push_str(&format!("Cc: {cc}\r\n"));
    }
    if let Some(bcc) = bcc.filter(|s| !s.is_empty()) {
        headers.push_str(&format!("Bcc: {bcc}\r\n"));
    }
    // Obligatoire (RFC 5322) — sans Date, IMAP/Himalaya renvoient date=null.
    headers.push_str(&format!(
        "Date: {}\r\n",
        chrono::Local::now().to_rfc2822()
    ));
    headers.push_str(&format!("Subject: {subject_hdr}\r\n"));
    headers.push_str("MIME-Version: 1.0\r\n");

    let text_part = {
        let mut p = String::new();
        p.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        p.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
        p.push_str(&form.body.replace('\n', "\r\n"));
        if !form.body.ends_with('\n') {
            p.push_str("\r\n");
        }
        p
    };

    let html_part = html_body.as_ref().map(|html| {
        let mut p = String::new();
        p.push_str("Content-Type: text/html; charset=utf-8\r\n");
        p.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
        p.push_str(&html.replace('\n', "\r\n"));
        if !html.ends_with('\n') {
            p.push_str("\r\n");
        }
        p
    });

    let body_part = if let Some(hp) = html_part {
        let alt = "----=_hima_alt_001";
        let mut p = String::new();
        p.push_str(&format!(
            "Content-Type: multipart/alternative; boundary=\"{alt}\"\r\n\r\n"
        ));
        p.push_str(&format!("--{alt}\r\n{text_part}"));
        p.push_str(&format!("--{alt}\r\n{hp}"));
        p.push_str(&format!("--{alt}--\r\n"));
        p
    } else {
        text_part
    };

    if files.is_empty() {
        if html_body.is_some() {
            let mixed_already = body_part.starts_with("Content-Type: multipart/");
            if mixed_already {
                // body_part already has Content-Type header — splice into message
                return format!("{headers}{body_part}");
            }
        }
        if html_body.is_some() {
            return format!("{headers}{body_part}");
        }
        headers.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        headers.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
        headers.push_str(&form.body.replace('\n', "\r\n"));
        if !form.body.ends_with('\n') {
            headers.push_str("\r\n");
        }
        return headers;
    }

    let bound = "----=_hima_mix_001";
    headers.push_str(&format!(
        "Content-Type: multipart/mixed; boundary=\"{bound}\"\r\n\r\n"
    ));
    let mut out = headers;
    out.push_str(&format!("--{bound}\r\n{body_part}"));
    for (name, data) in files {
        let safe_name = name.replace(['"', '\r', '\n'], "_");
        let mime = guess_mime(&safe_name);
        out.push_str(&format!("--{bound}\r\n"));
        out.push_str(&format!(
            "Content-Type: {mime}; name=\"{safe_name}\"\r\n"
        ));
        out.push_str("Content-Transfer-Encoding: base64\r\n");
        out.push_str(&format!(
            "Content-Disposition: attachment; filename=\"{safe_name}\"\r\n\r\n"
        ));
        let b64 = B64.encode(data);
        for chunk in b64.as_bytes().chunks(76) {
            out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            out.push_str("\r\n");
        }
    }
    out.push_str(&format!("--{bound}--\r\n"));
    out
}

fn guess_mime(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".pdf") {
        "application/pdf"
    } else if lower.ends_with(".txt") {
        "text/plain"
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        "text/html"
    } else if lower.ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    }
}

fn encode_rfc2047(s: &str) -> String {
    let b64 = B64.encode(s.as_bytes());
    format!("=?UTF-8?B?{b64}?=")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
