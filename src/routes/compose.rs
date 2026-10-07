use std::sync::Arc;

use askama::Template;
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;
use serde_json::json;

use crate::cli::himalaya::ComposeKind;
use crate::prefs::ACCOUNT_ALL;
use crate::state::AppState;

/// Limite corps multipart (PJ mail) — défaut Axum = 2 Mo, trop bas pour les pièces jointes.
const COMPOSE_BODY_LIMIT: usize = 50 * 1024 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/compose", get(compose_get))
        .route("/compose/send", post(compose_send))
        .route("/compose/draft", post(compose_draft))
        .layer(DefaultBodyLimit::max(COMPOSE_BODY_LIMIT))
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
    pub body: String,
    /// JSON meta sans le corps (évite de casser x-data)
    pub compose_boot: String,
    pub error: Option<String>,
    /// Champs reply (rendus serveur — fiables au submit, contrairement aux seuls binds Alpine)
    pub source_mailbox: String,
    pub source_id: String,
    pub source_account: String,
    pub in_reply_to: String,
    pub references: String,
}

pub struct AccountOpt {
    pub name: String,
    pub email: String,
    pub selected: bool,
    pub icon: String,
    pub color: String,
    pub signature: String,
    pub signature_html: String,
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
    if crate::data_backup::is_archive_account(Some(preferred.as_str()))
        || crate::data_backup::is_archive_account(prefs_snap.selected_account())
    {
        return Redirect::to("/?mailbox=Inbox").into_response();
    }

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
            let self_email = crate::accounts_config::list_editable_accounts()
                .ok()
                .and_then(|list| {
                    list.into_iter()
                        .find(|a| {
                            if preferred.is_empty() {
                                a.is_default
                            } else {
                                a.name == preferred
                            }
                        })
                        .map(|a| a.email)
                })
                .filter(|e| !e.trim().is_empty());
            let self_email_ref = self_email.as_deref();
            let _permit = state.cli_limit.acquire().await.ok();
            match state
                .himalaya
                .compose_template(kind, &mailbox, Some(id), account_ref, self_email_ref)
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
                        if matches!(kind, ComposeKind::ReplyAll) {
                            let (to, cc) = crate::cli::himalaya::reply_all_recipients(
                                &msg.from,
                                &msg.reply_to,
                                &msg.to,
                                &msg.cc,
                                self_email_ref,
                            );
                            draft.to = to;
                            draft.cc = cc;
                        } else if !matches!(kind, ComposeKind::Forward) {
                            draft.to = crate::cli::himalaya::normalize_addr_header(&msg.from);
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

    let is_reply = matches!(kind, ComposeKind::Reply | ComposeKind::ReplyAll);
    let source_mailbox = if is_reply {
        mailbox.clone()
    } else {
        String::new()
    };
    let source_id = if is_reply {
        q.id.clone().unwrap_or_default()
    } else {
        String::new()
    };
    let source_account = if is_reply {
        selected_account.clone()
    } else {
        String::new()
    };
    let compose_boot = serde_json::json!({
        "cardamum": state.cardamum_available,
        "ai": prefs_snap.ai_enabled,
        "toolbarMode": prefs_snap.compose_toolbar_mode,
        "widthNormal": prefs_snap.compose_width_normal,
        "widthDocked": prefs_snap.compose_width_docked,
        "kind": if is_reply { "reply" } else { "compose" },
        "title": title,
        "to": draft.to,
        "cc": draft.cc,
        "bcc": draft.bcc,
        "subject": draft.subject,
        "account": selected_account,
        "sourceMailbox": source_mailbox,
        "sourceId": source_id,
        "sourceAccount": source_account,
        "inReplyTo": draft.in_reply_to,
        "references": draft.references,
        "accounts": accounts.iter().map(|a| serde_json::json!({
            "name": a.name,
            "email": a.email,
            "icon": a.icon,
            "color": a.color,
            "signature": a.signature,
            "signatureHtml": a.signature_html,
        })).collect::<Vec<_>>(),
    })
    .to_string();

    let inner = ComposeTemplate {
        title: title.into(),
        body: draft.body,
        compose_boot,
        error,
        source_mailbox,
        source_id,
        source_account,
        in_reply_to: draft.in_reply_to,
        references: draft.references,
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
            .iter()
            .map(|a| (a.name.clone(), a.is_default, a.email.clone()))
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
            let (signature, signature_html) = editable
                .iter()
                .find(|e| e.name == name)
                .map(|e| (e.signature.clone(), e.signature_html.clone()))
                .unwrap_or_default();
            AccountOpt {
                name,
                email,
                selected,
                icon,
                color,
                signature,
                signature_html,
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
    reply_to: Option<String>,
    subject: String,
    body: String,
    body_html: Option<String>,
    html: Option<String>,
    files: Vec<(String, Vec<u8>)>,
    /// Message d’origine (reply) — pour poser le flag IMAP Answered
    source_mailbox: Option<String>,
    source_id: Option<String>,
    source_account: Option<String>,
    in_reply_to: Option<String>,
    references: Option<String>,
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
        let data = field
            .bytes()
            .await
            .map_err(|e| map_multipart_field_err(&name, &e))?;
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
            "reply_to" => {
                let s = String::from_utf8_lossy(&data).to_string();
                if !s.trim().is_empty() {
                    out.reply_to = Some(s);
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
            "source_mailbox" => {
                let s = String::from_utf8_lossy(&data).trim().to_string();
                if !s.is_empty() {
                    out.source_mailbox = Some(s);
                }
            }
            "source_id" => {
                let s = String::from_utf8_lossy(&data).trim().to_string();
                if !s.is_empty() {
                    out.source_id = Some(s);
                }
            }
            "source_account" => {
                let s = String::from_utf8_lossy(&data).trim().to_string();
                if !s.is_empty() {
                    out.source_account = Some(s);
                }
            }
            "in_reply_to" => {
                let s = String::from_utf8_lossy(&data).trim().to_string();
                if !s.is_empty() {
                    out.in_reply_to = Some(s);
                }
            }
            "references" => {
                let s = String::from_utf8_lossy(&data).trim().to_string();
                if !s.is_empty() {
                    out.references = Some(s);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn map_multipart_field_err(name: &str, e: &axum::extract::multipart::MultipartError) -> String {
    let s = e.to_string();
    if s.contains("too large") || s.contains("Limit") || s.contains("limit") {
        let mb = COMPOSE_BODY_LIMIT / (1024 * 1024);
        return format!("Pièce jointe ou message trop volumineux (max. {mb} Mo).");
    }
    format!("champ {name}: {s}")
}

/// Erreur compose → JSON (modal côté client), plus de page blanche.
fn compose_fail(msg: impl AsRef<str>) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": msg.as_ref() })),
    )
        .into_response()
}

async fn compose_send(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> impl IntoResponse {
    let form = match parse_compose_multipart(multipart).await {
        Ok(f) => f,
        Err(e) => return compose_fail(e),
    };
    finish_send(state, form).await
}

async fn finish_send(state: Arc<AppState>, form: ParsedCompose) -> axum::response::Response {
    let prefs_acc = state.prefs.lock().await.selected_account().map(str::to_string);
    if crate::data_backup::is_archive_account(form.account.as_deref())
        || crate::data_backup::is_archive_account(prefs_acc.as_deref())
    {
        return compose_fail("Compte archive en lecture seule — impossible d’envoyer.");
    }
    let to = match crate::cli::himalaya::smtp_address_list(&form.to) {
        Ok(t) => t,
        Err(e) => return compose_fail(format!("Destinataire : {e}")),
    };
    let cc = match form.cc.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => match crate::cli::himalaya::smtp_address_list(raw) {
            Ok(c) => Some(c),
            Err(e) => return compose_fail(format!("Cc : {e}")),
        },
        None => None,
    };
    let bcc = match form.bcc.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => match crate::cli::himalaya::smtp_address_list(raw) {
            Ok(c) => Some(c),
            Err(e) => return compose_fail(format!("Cci : {e}")),
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
    let has_reply_to = form
        .reply_to
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_thread_hdrs = form
        .in_reply_to
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        || form
            .references
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
    let sent_mailbox = resolve_sent_mailbox(account.as_deref());

    let _permit = state.cli_limit.acquire().await.ok();

    // PJ, HTML, Reply-To ou en-têtes de fil : EML (compose CLI = plain sans ces champs)
    let send_result = if as_html || has_files || has_reply_to || has_thread_hdrs {
        let eml = build_eml(
            &form,
            from.as_deref().unwrap_or(""),
            &to,
            cc.as_deref(),
            bcc.as_deref(),
            &form.files,
        );
        send_eml_with_sent_fallback(
            &state,
            eml.as_bytes(),
            account.as_deref(),
            &sent_mailbox,
        )
        .await
    } else {
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
                Some(&sent_mailbox),
            )
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => {
                // Pause courte si coupure SMTP (10054) : éviter d’enchaîner 3 envois qui reset
                if is_smtp_conn_reset(&e.to_string()) {
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                }
                // Échec souvent dû à l’alias Sent (Gmail) : réessayer sans --save, puis EML
                let retry = state
                    .himalaya
                    .send_message(
                        &to,
                        cc.as_deref(),
                        bcc.as_deref(),
                        &form.subject,
                        &form.body,
                        account.as_deref(),
                        from.as_deref(),
                        None,
                    )
                    .await;
                if retry.is_ok() {
                    tracing::warn!("envoi OK sans copie Sent ({sent_mailbox}): {e}");
                    Ok(())
                } else {
                    if is_smtp_conn_reset(&e.to_string())
                        || retry
                            .as_ref()
                            .err()
                            .map(|e2| is_smtp_conn_reset(&e2.to_string()))
                            .unwrap_or(false)
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    }
                    let eml = build_eml(
                        &form,
                        from.as_deref().unwrap_or(""),
                        &to,
                        cc.as_deref(),
                        bcc.as_deref(),
                        &form.files,
                    );
                    match send_eml_with_sent_fallback(
                        &state,
                        eml.as_bytes(),
                        account.as_deref(),
                        &sent_mailbox,
                    )
                    .await
                    {
                        Ok(()) => Ok(()),
                        Err(e2) => Err(format_send_error(&e.to_string(), Some(&e2))),
                    }
                }
            }
        }
    };

    match send_result {
        Ok(()) => {
            drop(_permit); // libérer avant un 2e acquire dans mark_source_answered
            mark_source_answered(&state, &form, account.as_deref()).await;
            Redirect::to("/").into_response()
        }
        Err(e) => compose_fail(format!("Envoi échoué : {e}")),
    }
}

async fn mark_source_answered(
    state: &AppState,
    form: &ParsedCompose,
    send_account: Option<&str>,
) {
    let Some(mb) = form
        .source_mailbox
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        tracing::warn!("reply: source_mailbox manquant — flag answered non posé");
        return;
    };
    let Some(id) = form
        .source_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        tracing::warn!("reply: source_id manquant — flag answered non posé");
        return;
    };
    // Compte du message d’origine (pas forcément le From choisi à l’envoi)
    let flag_account = form
        .source_account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != ACCOUNT_ALL)
        .or(send_account);
    let _permit = state.cli_limit.acquire().await.ok();
    if let Err(e) = state
        .himalaya
        .set_flag(mb, id, "answered", true, flag_account)
        .await
    {
        tracing::warn!("flag answered {mb}/{id} account={flag_account:?}: {e}");
    } else {
        tracing::info!("flag answered ok {mb}/{id} account={flag_account:?}");
    }
    // Mise à jour cache pour l’icône « répondu » dès le redirect
    let acc_key = flag_account.unwrap_or("");
    let cache = state.cache.lock().await;
    let _ = cache.add_envelope_flag(acc_key, mb, id, "answered");
}

fn resolve_sent_mailbox(account: Option<&str>) -> String {
    let editable = crate::accounts_config::list_editable_accounts().unwrap_or_default();
    account
        .and_then(|name| editable.into_iter().find(|a| a.name == name))
        .map(|a| a.sent_alias)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "sent".into())
}

/// Envoi EML avec `--save` Sent ; si l’alias Sent est invalide, renvoie quand même le mail.
async fn send_eml_with_sent_fallback(
    state: &AppState,
    eml: &[u8],
    account: Option<&str>,
    sent_mailbox: &str,
) -> Result<(), String> {
    match state
        .himalaya
        .send_raw_eml(eml, account, Some(sent_mailbox))
        .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            let e1 = e.to_string();
            if is_smtp_conn_reset(&e1) {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
            match state.himalaya.send_raw_eml(eml, account, None).await {
                Ok(()) => {
                    tracing::warn!("envoi OK sans copie Sent ({sent_mailbox}): {e1}");
                    Ok(())
                }
                Err(e2) => Err(format_send_error(&e1, Some(&e2.to_string()))),
            }
        }
    }
}

fn is_smtp_conn_reset(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    e.contains("10054")
        || e.contains("connection reset")
        || e.contains("fermée par l")
        || e.contains("fermee par l")
        || e.contains("broken pipe")
        || e.contains("connection abort")
}

fn format_send_error(primary: &str, secondary: Option<&str>) -> String {
    let detail = secondary
        .filter(|s| !s.is_empty() && *s != primary)
        .unwrap_or(primary);
    if is_smtp_conn_reset(primary) || secondary.is_some_and(is_smtp_conn_reset) {
        format!(
            "le serveur SMTP a coupé la connexion (souvent temporaire). Réessayez dans quelques secondes. Détail : {detail}"
        )
    } else if let Some(sec) = secondary.filter(|s| !s.is_empty() && *s != primary) {
        format!("{primary} / {sec}")
    } else {
        primary.to_string()
    }
}

async fn compose_draft(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> impl IntoResponse {
    let form = match parse_compose_multipart(multipart).await {
        Ok(f) => f,
        Err(e) => return compose_fail(e),
    };
    finish_draft(state, form).await
}

async fn finish_draft(state: Arc<AppState>, form: ParsedCompose) -> axum::response::Response {
    let to = match crate::cli::himalaya::smtp_address_list(&form.to) {
        Ok(t) => t,
        Err(e) => return compose_fail(format!("Destinataire : {e}")),
    };
    let cc = match form.cc.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => match crate::cli::himalaya::smtp_address_list(raw) {
            Ok(c) => Some(c),
            Err(e) => return compose_fail(format!("Cc : {e}")),
        },
        None => None,
    };
    let bcc = match form.bcc.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => match crate::cli::himalaya::smtp_address_list(raw) {
            Ok(c) => Some(c),
            Err(e) => return compose_fail(format!("Cci : {e}")),
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
    let has_reply_to = form
        .reply_to
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    let _permit = state.cli_limit.acquire().await.ok();
    let saved = if as_html || has_files || has_reply_to {
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
        Err(e) => compose_fail(format!("Brouillon échoué : {e}")),
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
    if let Some(rt) = form
        .reply_to
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        headers.push_str(&format!("Reply-To: {rt}\r\n"));
    }
    if let Some(irt) = form
        .in_reply_to
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        headers.push_str(&format!("In-Reply-To: {irt}\r\n"));
    }
    if let Some(refs) = form
        .references
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        headers.push_str(&format!("References: {refs}\r\n"));
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
