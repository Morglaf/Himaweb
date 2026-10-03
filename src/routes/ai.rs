use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post};
use axum::{Form, Router};
use chrono::Local;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::prefs::Prefs;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ai", get(ai_page))
        .route("/ai/draft", post(ai_draft))
        .route("/ai/event", post(ai_event_draft))
        .route("/ai/api/mail", post(ai_api_mail))
        .route("/ai/api/event", post(ai_api_event))
        .route("/ai/api/inbox-summary", post(ai_api_inbox_summary))
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
#[template(path = "ai.html")]
struct AiTemplate {
    pub enabled: bool,
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub draft: Option<String>,
    pub error: Option<String>,
}

async fn ai_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    let inner = AiTemplate {
        enabled: prefs.ai_enabled,
        provider: prefs.ai_provider.clone(),
        endpoint: prefs.ai_endpoint.clone(),
        model: prefs.ai_model.clone(),
        draft: None,
        error: None,
    };
    render_shell(&state, inner).await
}

#[derive(Deserialize)]
pub struct DraftForm {
    pub prompt: String,
    pub context: Option<String>,
    pub kind: Option<String>,
}

async fn ai_draft(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DraftForm>,
) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    if !prefs.ai_enabled {
        let inner = AiTemplate {
            enabled: false,
            provider: prefs.ai_provider,
            endpoint: prefs.ai_endpoint,
            model: prefs.ai_model,
            draft: None,
            error: Some("IA désactivée — activez-la dans Paramètres.".into()),
        };
        return render_shell(&state, inner).await;
    }
    match generate_mail(
        &prefs,
        form.prompt.trim(),
        form.context.as_deref(),
        None,
        form.kind.as_deref(),
        None,
        None,
    )
    .await
    {
        Ok(mail) => {
            let draft = if mail.subject.is_empty() {
                mail.body
            } else {
                format!("Objet: {}\n\n{}", mail.subject, mail.body)
            };
            let inner = AiTemplate {
                enabled: true,
                provider: prefs.ai_provider,
                endpoint: prefs.ai_endpoint,
                model: prefs.ai_model,
                draft: Some(draft),
                error: None,
            };
            render_shell(&state, inner).await
        }
        Err(e) => {
            let inner = AiTemplate {
                enabled: true,
                provider: prefs.ai_provider,
                endpoint: prefs.ai_endpoint,
                model: prefs.ai_model,
                draft: None,
                error: Some(e),
            };
            render_shell(&state, inner).await
        }
    }
}

#[derive(Deserialize)]
pub struct EventAiForm {
    pub prompt: String,
}

async fn ai_event_draft(
    State(state): State<Arc<AppState>>,
    Form(form): Form<EventAiForm>,
) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    if !prefs.ai_enabled {
        return Json(json!({ "error": "IA désactivée" })).into_response();
    }
    match generate_event(&prefs, form.prompt.trim(), None, None).await {
        Ok(v) => Json(json!({ "ok": true, "event": v })).into_response(),
        Err(e) => Json(json!({ "error": e })).into_response(),
    }
}

#[derive(Deserialize)]
pub struct MailApiBody {
    pub prompt: String,
    pub context: Option<String>,
    pub previous: Option<String>,
    pub kind: Option<String>,
    pub account: Option<String>,
    pub now: Option<String>,
}

async fn ai_api_mail(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<MailApiBody>,
) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    if !prefs.ai_enabled {
        return Json(json!({ "error": "IA désactivée" })).into_response();
    }
    match generate_mail(
        &prefs,
        body.prompt.trim(),
        body.context.as_deref(),
        body.previous.as_deref(),
        body.kind.as_deref(),
        body.account.as_deref(),
        body.now.as_deref(),
    )
    .await
    {
        Ok(mail) => Json(json!({
            "ok": true,
            "to": mail.to,
            "subject": mail.subject,
            "body": mail.body,
        }))
        .into_response(),
        Err(e) => Json(json!({ "error": e })).into_response(),
    }
}

#[derive(Deserialize)]
pub struct EventApiBody {
    pub prompt: String,
    pub now: Option<String>,
    pub selected_date: Option<String>,
}

async fn ai_api_event(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<EventApiBody>,
) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    if !prefs.ai_enabled {
        return Json(json!({ "error": "IA désactivée" })).into_response();
    }
    match generate_event(
        &prefs,
        body.prompt.trim(),
        body.now.as_deref(),
        body.selected_date.as_deref(),
    )
    .await
    {
        Ok(v) => Json(json!({ "ok": true, "event": v })).into_response(),
        Err(e) => Json(json!({ "error": e })).into_response(),
    }
}

#[derive(Deserialize)]
pub struct InboxSummaryItemIn {
    pub id: String,
    pub account: Option<String>,
    pub mailbox: Option<String>,
    pub from: Option<String>,
    pub subject: Option<String>,
}

#[derive(Deserialize)]
pub struct InboxSummaryBody {
    pub mailbox: Option<String>,
    pub account: Option<String>,
    pub items: Option<Vec<InboxSummaryItemIn>>,
    #[serde(default)]
    pub limit: Option<u32>,
}

async fn ai_api_inbox_summary(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<InboxSummaryBody>,
) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    if !prefs.ai_enabled {
        return Json(json!({ "error": "IA désactivée" })).into_response();
    }
    if !state.himalaya_available {
        return Json(json!({ "error": "Himalaya indisponible" })).into_response();
    }

    let limit = body.limit.unwrap_or(15).clamp(1, 25) as usize;
    let mailbox = body
        .mailbox
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Inbox");
    let account_opt = body
        .account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut targets: Vec<(String, Option<String>, String, String, String)> = Vec::new();
    if let Some(items) = &body.items {
        for it in items.iter().take(limit) {
            if it.id.trim().is_empty() {
                continue;
            }
            targets.push((
                it.id.clone(),
                it.account
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| account_opt.map(str::to_string)),
                it.mailbox
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| mailbox.to_string()),
                it.from.clone().unwrap_or_default(),
                it.subject.clone().unwrap_or_default(),
            ));
        }
    }

    if targets.is_empty() {
        match state
            .himalaya
            .list_envelopes(mailbox, 1, limit as u32, account_opt)
            .await
        {
            Ok(envs) => {
                for e in envs.into_iter().take(limit) {
                    targets.push((
                        e.id,
                        account_opt.map(str::to_string),
                        mailbox.to_string(),
                        e.from,
                        e.subject,
                    ));
                }
            }
            Err(e) => {
                return Json(json!({ "error": format!("Liste messages: {e}") })).into_response();
            }
        }
    }

    if targets.is_empty() {
        return Json(json!({ "ok": true, "items": [] })).into_response();
    }

    // Lecture des corps en parallèle (bornée par cli_limit)
    let mut join = JoinSet::new();
    for (id, acc, mbox, from, subject) in targets {
        let himalaya = state.himalaya.clone();
        let limit_sem = state.cli_limit.clone();
        join.spawn(async move {
            let _permit = limit_sem.acquire().await.ok();
            let acc_ref = acc.as_deref();
            let body_txt = match himalaya.read_message(&mbox, &id, acc_ref).await {
                Ok(msg) => {
                    let t = if !msg.body_text.trim().is_empty() {
                        msg.body_text
                    } else {
                        strip_html_approx(&msg.body_html)
                    };
                    truncate_chars(&t, 1200)
                }
                Err(_) => String::new(),
            };
            (id, acc, mbox, from, subject, body_txt)
        });
    }

    let mut snippets = Vec::new();
    while let Some(res) = join.join_next().await {
        if let Ok(row) = res {
            snippets.push(row);
        }
    }
    snippets.sort_by(|a, b| a.0.cmp(&b.0));

    let mut catalog = String::new();
    let mut accounts_in_batch: Vec<String> = Vec::new();
    for (i, (id, acc, _mbox, from, subject, body_txt)) in snippets.iter().enumerate() {
        if let Some(a) = acc.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            if !accounts_in_batch.iter().any(|x| x == a) {
                accounts_in_batch.push(a.to_string());
            }
        }
        catalog.push_str(&format!(
            "### MSG {i}\nid: {id}\nfrom: {from}\nsubject: {subject}\nbody:\n{body_txt}\n\n"
        ));
    }
    if accounts_in_batch.is_empty() {
        if let Some(a) = account_opt {
            accounts_in_batch.push(a.to_string());
        }
    }

    let now = Local::now().format("%Y-%m-%d %H:%M (%z)").to_string();
    let system = build_inbox_summary_system(&prefs, &accounts_in_batch);
    let user = format!("Now: {now}\n\nMessages:\n{catalog}");
    let text = match complete(&prefs, &system, &user, "inbox-summary").await {
        Ok(t) => t,
        Err(e) => return Json(json!({ "error": e })).into_response(),
    };
    let parsed = parse_inbox_summary_response(&text);
    let mut by_id: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut by_order: Vec<String> = Vec::new();
    for (id, summary) in parsed {
        let summary = summary.trim().to_string();
        if summary.is_empty() {
            continue;
        }
        by_order.push(summary.clone());
        if !id.is_empty() {
            by_id.insert(id, summary);
        }
    }
    // Ne jamais réutiliser un résumé par index si on a déjà des ids matchés
    // (évite d’attribuer le résumé de MSG N à MSG 0 quand le modèle fusionne les objets).
    let allow_order_fallback = by_id.is_empty();

    let items: Vec<Value> = snippets
        .into_iter()
        .enumerate()
        .map(|(idx, (id, acc, mbox, from, subject, body_txt))| {
            let summary = by_id
                .get(&id)
                .cloned()
                .or_else(|| {
                    if allow_order_fallback {
                        by_order.get(idx).cloned()
                    } else {
                        None
                    }
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    // Dernier recours : amorce du corps (pas le sujet seul, déjà affiché)
                    let snip = body_txt.trim();
                    if snip.is_empty() {
                        truncate_chars(&subject, 120)
                    } else {
                        truncate_chars(snip, 160)
                    }
                });
            json!({
                "id": id,
                "account": acc.unwrap_or_default(),
                "mailbox": mbox,
                "from": from,
                "subject": subject,
                "summary": summary,
            })
        })
        .collect();

    Json(json!({ "ok": true, "items": items })).into_response()
}

struct MailDraft {
    to: String,
    subject: String,
    body: String,
}

fn local_now_label(override_now: Option<&str>) -> String {
    override_now
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Local::now().format("%Y-%m-%d %H:%M (%z)").to_string())
}

fn build_system_preamble(prefs: &Prefs, account: Option<&str>, calendar: bool) -> String {
    let mut parts = Vec::new();
    let global = prefs.ai_ollama_preprompt.trim();
    if !global.is_empty() {
        parts.push(global.to_string());
    }
    if let Some(acc) = account.map(str::trim).filter(|s| !s.is_empty()) {
        let ap = prefs.account_ai_preprompt_for(acc);
        if !ap.is_empty() {
            parts.push(format!("Compte « {acc} » — présentation / style:\n{ap}"));
        }
    }
    if calendar {
        let cal = prefs.ai_calendar_preprompt.trim();
        if !cal.is_empty() {
            parts.push(cal.to_string());
        }
        let home = prefs.home_address.trim();
        if !home.is_empty() {
            parts.push(format!("Adresse de domicile (départ trajet): {home}"));
        }
    }
    parts.join("\n\n")
}

fn build_inbox_summary_system(prefs: &Prefs, accounts: &[String]) -> String {
    // Consignes de style / langue / tutoiement : uniquement via les préprompts prefs.
    let task = "Summarize each inbox message for the account owner (the recipient). \
Reply with strict JSON only — an array of SEPARATE objects (close each object with }), never merge keys into one object: \
{\"items\":[{\"id\":\"1\",\"summary\":\"…\"},{\"id\":\"2\",\"summary\":\"…\"}]} \
One entry per message, id must match the given id string, summary = one short sentence. No markdown, no text outside JSON.";

    let mut parts = Vec::new();
    let global = prefs.ai_ollama_preprompt.trim();
    if !global.is_empty() {
        parts.push(global.to_string());
    }
    for acc in accounts {
        let ap = prefs.account_ai_preprompt_for(acc);
        if !ap.is_empty() {
            parts.push(format!("Account `{acc}`:\n{ap}"));
        }
    }
    let inbox = prefs.ai_inbox_preprompt.trim();
    if !inbox.is_empty() {
        parts.push(inbox.to_string());
    }
    parts.push(task.to_string());
    parts.join("\n\n")
}

async fn generate_mail(
    prefs: &Prefs,
    prompt: &str,
    context: Option<&str>,
    previous: Option<&str>,
    kind: Option<&str>,
    account: Option<&str>,
    now: Option<&str>,
) -> Result<MailDraft, String> {
    let system_task = match kind {
        Some("reply") => {
            "Tu aides à rédiger / corriger un email (réponse). Applique la consigne (corriger, reformuler, compléter…). Réponds en JSON strict uniquement: {\"to\",\"subject\",\"body\"}. Le champ body doit contenir uniquement la nouvelle réponse ou correction, sans réinclure ni citer le message précédent (déjà fourni à part). Ne vide pas un champ déjà rempli sauf demande explicite. Pas de markdown."
        }
        _ => {
            "Tu aides à rédiger / corriger un email. Applique la consigne (corriger, reformuler, compléter…). Réponds en JSON strict uniquement: {\"to\",\"subject\",\"body\"}. Si un message précédent est fourni à part, ne l'inclus pas dans body — body = seulement le brouillon / la nouvelle réponse. Ne vide pas un champ déjà rempli sauf demande explicite. Pas de markdown."
        }
    };
    let preamble = build_system_preamble(prefs, account, false);
    let system = if preamble.is_empty() {
        system_task.to_string()
    } else {
        format!("{preamble}\n\n{system_task}")
    };

    let now_s = local_now_label(now);
    let mut user = format!("Maintenant (utilisateur): {now_s}\n\nConsigne:\n{prompt}\n");
    if let Some(prev) = previous.map(str::trim).filter(|s| !s.is_empty()) {
        user.push_str("\n--- Message précédent ---\n");
        user.push_str(prev);
        user.push('\n');
    }
    let ctx = context.unwrap_or("").trim();
    if !ctx.is_empty() {
        user.push_str("\n--- Brouillon actuel ---\n");
        user.push_str(ctx);
        user.push('\n');
    } else if previous.map(str::trim).filter(|s| !s.is_empty()).is_none() {
        user.push_str("\n--- Brouillon actuel ---\n(aucun)\n");
    }

    let text = complete(prefs, &system, &user, "mail").await?;
    let cleaned = strip_fences(&text);
    if let Ok(v) = serde_json::from_str::<Value>(cleaned) {
        Ok(MailDraft {
            to: v
                .get("to")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            subject: v
                .get("subject")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            body: v
                .get("body")
                .and_then(|x| x.as_str())
                .unwrap_or(cleaned)
                .to_string(),
        })
    } else {
        let (subject, body) = split_subject_body(cleaned);
        Ok(MailDraft {
            to: String::new(),
            subject,
            body,
        })
    }
}

async fn generate_event(
    prefs: &Prefs,
    prompt: &str,
    now: Option<&str>,
    selected_date: Option<&str>,
) -> Result<Value, String> {
    let system_task = "Propose un événement calendrier. Réponds en JSON strict uniquement: {\"summary\",\"date\",\"start_time\",\"end_date\",\"end_time\",\"description\",\"location\",\"rrule\"} où date/end_date = YYYY-MM-DD, start_time/end_time = HH:MM, rrule = none|daily|weekly|monthly|yearly. Pas de markdown.";
    let preamble = build_system_preamble(prefs, None, true);
    let system = if preamble.is_empty() {
        system_task.to_string()
    } else {
        format!("{preamble}\n\n{system_task}")
    };
    let now_s = local_now_label(now);
    let mut user = format!("Maintenant (utilisateur): {now_s}\n");
    if let Some(d) = selected_date.map(str::trim).filter(|s| !s.is_empty()) {
        user.push_str(&format!("Date / créneau sélectionné dans l’agenda: {d}\n"));
    }
    user.push_str("\nConsigne:\n");
    user.push_str(prompt);

    let text = complete(prefs, &system, &user, "event").await?;
    let cleaned = strip_fences(&text);
    serde_json::from_str::<Value>(cleaned).or_else(|_| {
        Ok(json!({
            "summary": cleaned.chars().take(80).collect::<String>(),
            "description": cleaned,
            "rrule": "none"
        }))
    })
}

fn strip_fences(text: &str) -> &str {
    text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

fn strip_think_blocks(text: &str) -> String {
    let mut out = text.to_string();
    while let Some(start) = out.find("<think>") {
        if let Some(rel_end) = out[start..].find("</think>") {
            let end = start + rel_end + "</think>".len();
            out.replace_range(start..end, "");
        } else {
            // Bloc de réflexion non fermé : tout jeter jusqu'au premier `{` JSON
            if let Some(brace) = out[start..].find('{') {
                out.replace_range(start..start + brace, "");
            } else {
                out.replace_range(start.., "");
            }
            break;
        }
    }
    out
}

fn extract_json_value(text: &str) -> Option<&str> {
    let cleaned = strip_fences(text);
    let brace = cleaned.find('{');
    let bracket = cleaned.find('[');
    let (start, open, close) = match (brace, bracket) {
        (Some(b), Some(a)) if a < b => (a, '[', ']'),
        (Some(b), _) => (b, '{', '}'),
        (None, Some(a)) => (a, '[', ']'),
        (None, None) => return None,
    };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, ch) in cleaned[start..].char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(&cleaned[start..start + i + 1]);
            }
        } else if ch == '"' {
            in_str = true;
        }
    }
    // JSON tronqué : préfixe pour scrape
    Some(&cleaned[start..])
}

fn extract_json_object(text: &str) -> Option<&str> {
    // Conservé pour extract_ollama_text / thinking
    let cleaned = strip_fences(text);
    let start = cleaned.find('{')?;
    extract_json_value(&cleaned[start..]).filter(|s| s.starts_with('{'))
}

fn json_stringish(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn items_from_json_value(v: &Value) -> Vec<(String, String)> {
    let arr = if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
        arr.as_slice()
    } else if let Some(arr) = v.as_array() {
        arr.as_slice()
    } else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for it in arr {
        let id = it.get("id").and_then(json_stringish).unwrap_or_default();
        let summary = it
            .get("summary")
            .and_then(json_stringish)
            .unwrap_or_default();
        if !summary.trim().is_empty() {
            out.push((id, summary));
        }
    }
    out
}

/// Parse la réponse inbox-summary : `{"items":[…]}` ou tableau nu `[…]`,
/// plus scrape des paires id/summary (JSON fusionné / tronqué / id numériques).
fn parse_inbox_summary_response(text: &str) -> Vec<(String, String)> {
    let without_think = strip_think_blocks(text);
    // Scrape sur le texte complet — pas seulement le 1er `{…}` (sinon tableau nu → 1 seul item)
    let scraped = scrape_inbox_summary_pairs(&without_think);

    let mut from_json = Vec::new();
    if let Some(frag) = extract_json_value(&without_think) {
        for candidate in [frag.to_string(), repair_truncated_json(frag)] {
            if let Ok(v) = serde_json::from_str::<Value>(&candidate) {
                let out = items_from_json_value(&v);
                if !out.is_empty() {
                    from_json = out;
                    break;
                }
            }
        }
    }

    // Prefer scrape when it recovers more pairs (flattened duplicate keys, etc.)
    if scraped.len() > from_json.len() {
        scraped
    } else if !from_json.is_empty() {
        from_json
    } else {
        scraped
    }
}

/// Tente de refermer un JSON coupé (ex. `{"items":[...]}` sans `}` final).
fn repair_truncated_json(frag: &str) -> String {
    let mut s = frag.trim_end().to_string();
    // Fermer une string ouverte
    let mut in_str = false;
    let mut escape = false;
    for ch in s.chars() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
        } else if ch == '"' {
            in_str = true;
        }
    }
    if in_str {
        s.push('"');
    }
    let opens = s.chars().filter(|c| *c == '{').count();
    let closes = s.chars().filter(|c| *c == '}').count();
    let open_arr = s.chars().filter(|c| *c == '[').count();
    let close_arr = s.chars().filter(|c| *c == ']').count();
    for _ in 0..open_arr.saturating_sub(close_arr) {
        s.push(']');
    }
    for _ in 0..opens.saturating_sub(closes) {
        s.push('}');
    }
    s
}

fn scrape_inbox_summary_pairs(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Cherche "id" : … puis "summary" : "…"
        let rest = &text[i..];
        let Some(id_key) = rest.find("\"id\"") else {
            break;
        };
        let after_id = i + id_key + 4;
        let Some(colon) = text[after_id..].find(':') else {
            break;
        };
        let mut p = after_id + colon + 1;
        while p < text.len() && text.as_bytes()[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= text.len() {
            break;
        }
        let id = if text.as_bytes()[p] == b'"' {
            p += 1;
            let start = p;
            while p < text.len() {
                let b = text.as_bytes()[p];
                if b == b'\\' {
                    p = (p + 2).min(text.len());
                    continue;
                }
                if b == b'"' {
                    break;
                }
                p += 1;
            }
            let s = text[start..p].to_string();
            if p < text.len() {
                p += 1;
            }
            s
        } else {
            let start = p;
            while p < text.len() {
                let b = text.as_bytes()[p];
                if b.is_ascii_digit() {
                    p += 1;
                } else {
                    break;
                }
            }
            text[start..p].to_string()
        };

        let Some(sum_rel) = text[p..].find("\"summary\"") else {
            i = p.max(i + 1);
            continue;
        };
        let after_sum = p + sum_rel + 9;
        let Some(colon2) = text[after_sum..].find(':') else {
            i = after_sum.max(i + 1);
            continue;
        };
        let mut q = after_sum + colon2 + 1;
        while q < text.len() && text.as_bytes()[q].is_ascii_whitespace() {
            q += 1;
        }
        if q >= text.len() || text.as_bytes()[q] != b'"' {
            i = q.max(i + 1);
            continue;
        }
        q += 1;
        let start = q;
        let mut incomplete = true;
        while q < text.len() {
            let b = text.as_bytes()[q];
            if b == b'\\' {
                q = (q + 2).min(text.len());
                continue;
            }
            if b == b'"' {
                incomplete = false;
                break;
            }
            q += 1;
        }
        let mut summary = text[start..q].to_string();
        // JSON coupé au milieu d'une string : garder la phrase partielle si utilisable
        if incomplete {
            summary = summary.trim_end().to_string();
            if let Some(cut) = summary.rfind(['.', '!', '?', ',', ';']) {
                summary = summary[..=cut].trim().to_string();
            }
        }
        if !summary.trim().is_empty() {
            out.push((id, summary));
        }
        i = if incomplete { text.len() } else { q + 1 };
    }
    out
}

fn split_subject_body(text: &str) -> (String, String) {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("Objet:") {
        if let Some((subj, body)) = rest.split_once('\n') {
            return (subj.trim().to_string(), body.trim().to_string());
        }
    }
    (String::new(), t.to_string())
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn strip_html_approx(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn append_ai_log(prefs: &Prefs, kind: &str, system: &str, user: &str, response: &str) {
    if !prefs.ai_log_enabled {
        return;
    }
    let Ok(path) = Prefs::ai_log_path() else {
        return;
    };
    let entry = json!({
        "ts": Local::now().to_rfc3339(),
        "kind": kind,
        "provider": prefs.ai_provider,
        "model": prefs.ai_model,
        "system": truncate_chars(system, 8000),
        "user": truncate_chars(user, 16000),
        "response": truncate_chars(response, 16000),
    });
    if let Ok(line) = serde_json::to_string(&entry) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

async fn complete(prefs: &Prefs, system: &str, user: &str, kind: &str) -> Result<String, String> {
    let result = match prefs.ai_provider.as_str() {
        "gemini" => gemini_chat(prefs, system, user).await,
        _ => match ollama_chat(prefs, system, user).await {
            Ok(t) => Ok(t),
            Err(local_err) => {
                if prefs.ai_remote_endpoint.trim().is_empty() {
                    Err(format!("Ollama: {local_err}"))
                } else {
                    remote_chat(
                        &prefs.ai_remote_endpoint,
                        &prefs.ai_model,
                        &prefs.ai_api_key,
                        system,
                        user,
                    )
                    .await
                    .map_err(|e| format!("Ollama: {local_err} · Distant: {e}"))
                }
            }
        },
    };
    match &result {
        Ok(text) => append_ai_log(prefs, kind, system, user, text),
        Err(e) => append_ai_log(prefs, kind, system, user, &format!("ERROR: {e}")),
    }
    result
}

async fn gemini_chat(prefs: &Prefs, system: &str, user: &str) -> Result<String, String> {
    let key = prefs.ai_api_key.trim();
    if key.is_empty() {
        return Err("Clé API Gemini manquante (Paramètres → IA)".into());
    }
    let mut model = prefs.ai_model.trim();
    if model.is_empty() || model.contains("llama") {
        model = "gemini-2.5-flash";
    }
    let model = model.trim_start_matches("models/");
    let is_gemma = model.to_ascii_lowercase().contains("gemma");
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={key}"
    );

    let body = if is_gemma {
        let combined = if system.trim().is_empty() {
            user.to_string()
        } else {
            format!("{system}\n\n---\n\n{user}")
        };
        json!({
            "contents": [{ "role": "user", "parts": [{ "text": combined }] }],
            "generationConfig": {
                "temperature": 1.0,
                "maxOutputTokens": 8192
            }
        })
    } else {
        json!({
            "systemInstruction": { "role": "system", "parts": [{ "text": system }] },
            "contents": [{ "role": "user", "parts": [{ "text": user }] }],
            "generationConfig": {
                "temperature": 0.4,
                "maxOutputTokens": 8192
            }
        })
    };

    let client = reqwest::Client::new();
    let mut last_err = String::new();
    let attempts = if is_gemma { 4 } else { 2 };
    for attempt in 1..=attempts {
        let res = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if res.status().is_success() {
            let v: Value = res.json().await.map_err(|e| e.to_string())?;
            if let Some(text) = extract_gemini_text(&v) {
                return Ok(text);
            }
            last_err = "réponse Gemini sans contenu".into();
        } else {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            last_err = format!("Gemini HTTP {status}: {err_body}");
            if !status.is_server_error() {
                break;
            }
        }
        if attempt < attempts {
            tokio::time::sleep(std::time::Duration::from_millis(400 * attempt as u64)).await;
        }
    }
    if is_gemma {
        Err(format!(
            "{last_err} — le modèle `{model}` est souvent instable côté Google. Réessayez, ou choisissez `gemini-2.5-flash` dans Paramètres → IA."
        ))
    } else {
        Err(last_err)
    }
}

fn extract_gemini_text(v: &Value) -> Option<String> {
    if let Some(parts) = v.pointer("/candidates/0/content/parts").and_then(|p| p.as_array()) {
        let mut texts = Vec::new();
        for part in parts {
            if part.get("thought").and_then(|t| t.as_bool()) == Some(true) {
                continue;
            }
            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                if !t.trim().is_empty() {
                    texts.push(t.to_string());
                }
            }
        }
        if !texts.is_empty() {
            return Some(texts.join("\n"));
        }
    }
    v.pointer("/candidates/0/content/parts/0/text")
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

async fn ollama_chat(prefs: &Prefs, system: &str, user: &str) -> Result<String, String> {
    // Le préprompt est déjà fusionné dans `system` via build_system_preamble.
    let url = format!("{}/api/chat", prefs.ai_endpoint.trim_end_matches('/'));
    let mut body = json!({
        "model": prefs.ai_model,
        "stream": false,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    });
    match prefs.ai_ollama_think.as_str() {
        "off" => {
            body["think"] = json!(false);
        }
        "low" | "medium" | "high" => {
            body["think"] = json!(prefs.ai_ollama_think.as_str());
        }
        // qwen3.5 « default » : thinking allumé → content vide, JSON perdu.
        // On coupe le think sauf demande explicite low/medium/high.
        _ => {
            body["think"] = json!(false);
        }
    }
    // num_predict assez haut : qwen/think coupe sinon le JSON inbox-summary
    let mut options = json!({ "num_predict": 2048 });
    if let Some(t) = prefs.ai_ollama_temperature {
        options["temperature"] = json!(t);
    }
    body["options"] = options;
    let client = reqwest::Client::new();
    let res = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    let v: Value = res.json().await.map_err(|e| e.to_string())?;
    extract_ollama_text(&v).ok_or_else(|| "réponse Ollama sans contenu".into())
}

fn extract_ollama_text(v: &Value) -> Option<String> {
    let content = v
        .pointer("/message/content")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if content.is_some() {
        return content;
    }
    // Certains modèles (qwen3.5) ne remplissent que `thinking`
    let thinking = v
        .pointer("/message/thinking")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    if let Some(json) = extract_json_object(thinking) {
        if json.contains("items") || json.contains("subject") || json.contains("summary") {
            return Some(json.to_string());
        }
    }
    Some(thinking.to_string())
}

async fn remote_chat(
    endpoint: &str,
    model: &str,
    api_key: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    });
    let client = reqwest::Client::new();
    let mut req = client.post(endpoint.trim()).json(&body);
    if !api_key.trim().is_empty() {
        req = req.bearer_auth(api_key.trim());
    }
    let res = req.send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    let v: Value = res.json().await.map_err(|e| e.to_string())?;
    v.pointer("/choices/0/message/content")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .ok_or_else(|| "réponse distante sans contenu".into())
}

async fn render_shell(state: &AppState, inner: AiTemplate) -> axum::response::Response {
    let content = match inner.render() {
        Ok(c) => c,
        Err(e) => format!("<pre>{e}</pre>"),
    };
    let (theme, layout) = state.theme_layout().await;
    let shell = ShellTemplate {
        title: "HimaWeb — IA".into(),
        active_tab: "ai".into(),
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

#[cfg(test)]
mod inbox_summary_parse_tests {
    use super::*;

    #[test]
    fn parses_string_ids() {
        let raw = r#"{"items":[{"id":"4652","summary":"Demande d'aide publication."}]}"#;
        let out = parse_inbox_summary_response(raw);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "4652");
        assert!(out[0].1.contains("publication"));
    }

    #[test]
    fn parses_numeric_ids_and_missing_brace() {
        // Cas réel qwen : id numériques + `}` racine manquant
        let raw = r#"{"items":[{"id":4652,"summary":"Beatrice mentionne le site."},{"id":4653,"summary":"Elodie evoque un festival."}]"#;
        let out = parse_inbox_summary_response(raw);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "4652");
        assert_eq!(out[1].0, "4653");
    }

    #[test]
    fn scrapes_truncated_mid_string() {
        let raw = r#"{"items":[{"id":4652,"summary":"Phrase complete."},{"id":4653,"summary":"Coupe au mil"#;
        let out = parse_inbox_summary_response(raw);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].1, "Phrase complete.");
        assert!(!out[1].1.is_empty());
    }

    #[test]
    fn scrapes_flattened_duplicate_keys_in_one_object() {
        // Cas réel qwen : un seul objet avec id/summary répétés
        let raw = r#"{"items":[{"id":"4652","summary":"Beatrice aide site.","id":"4653","summary":"Elodie programme.","id":"9827","summary":"Cecile meetup.","id":"9837","summary":"Jean-Claude photos."}]}"#;
        let out = parse_inbox_summary_response(raw);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].0, "4652");
        assert_eq!(out[1].0, "4653");
        assert_eq!(out[2].0, "9827");
        assert_eq!(out[3].0, "9837");
        assert!(out[0].1.contains("Beatrice"));
        assert!(out[3].1.contains("Jean-Claude"));
    }

    #[test]
    fn parses_bare_array_without_items_wrapper() {
        // Cas réel qwen : tableau nu sans {"items":…}
        let raw = r#"[{"id":"4652","summary":"Beatrice aide site."},{"id":"4653","summary":"Elodie programme."},{"id":"9827","summary":"Cecile meetup."},{"id":"9837","summary":"Jean-Claude photos."}]"#;
        let out = parse_inbox_summary_response(raw);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].0, "4652");
        assert_eq!(out[3].0, "9837");
        assert!(out[1].1.contains("Elodie"));
    }
}
