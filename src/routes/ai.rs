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
    for (i, (id, _acc, _mbox, from, subject, body_txt)) in snippets.iter().enumerate() {
        catalog.push_str(&format!(
            "### MSG {i}\nid: {id}\nfrom: {from}\nsubject: {subject}\nbody:\n{body_txt}\n\n"
        ));
    }

    let now = Local::now().format("%Y-%m-%d %H:%M (%z)").to_string();
    let system = "Tu résumes une boîte mail. Réponds en JSON strict uniquement: {\"items\":[{\"id\",\"summary\"}]} — une entrée par message (même id), summary = une phrase courte en français. Pas de markdown.";
    let user = format!("Maintenant: {now}\n\nMessages:\n{catalog}");
    let text = match complete(&prefs, system, &user, "inbox-summary").await {
        Ok(t) => t,
        Err(e) => return Json(json!({ "error": e })).into_response(),
    };
    let cleaned = strip_fences(&text);
    let mut by_id: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    if let Ok(v) = serde_json::from_str::<Value>(cleaned) {
        if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
            for it in arr {
                let id = it.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let summary = it
                    .get("summary")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if !id.is_empty() {
                    by_id.insert(id, summary);
                }
            }
        }
    }

    let items: Vec<Value> = snippets
        .into_iter()
        .map(|(id, acc, mbox, from, subject, _)| {
            let summary = by_id
                .get(&id)
                .cloned()
                .unwrap_or_else(|| truncate_chars(&subject, 120));
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
            "Tu aides à rédiger / corriger un email (réponse). Applique la consigne (corriger, reformuler, compléter…). Réponds en JSON strict uniquement: {\"to\",\"subject\",\"body\"}. Ne vide pas un champ déjà rempli sauf demande explicite. Pas de markdown."
        }
        _ => {
            "Tu aides à rédiger / corriger un email. Applique la consigne (corriger, reformuler, compléter…). Réponds en JSON strict uniquement: {\"to\",\"subject\",\"body\"}. Ne vide pas un champ déjà rempli sauf demande explicite. Pas de markdown."
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
        _ => {}
    }
    if let Some(t) = prefs.ai_ollama_temperature {
        body["options"] = json!({ "temperature": t });
    }
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
    v.pointer("/message/content")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .ok_or_else(|| "réponse Ollama sans contenu".into())
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
