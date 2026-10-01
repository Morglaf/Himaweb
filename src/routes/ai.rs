use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::prefs::Prefs;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ai", get(ai_page))
        .route("/ai/draft", post(ai_draft))
        .route("/ai/event", post(ai_event_draft))
        .route("/ai/api/mail", post(ai_api_mail))
        .route("/ai/api/event", post(ai_api_event))
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
    match generate_mail(&prefs, form.prompt.trim(), form.context.as_deref(), form.kind.as_deref())
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
    match generate_event(&prefs, form.prompt.trim()).await {
        Ok(v) => Json(json!({ "ok": true, "event": v })).into_response(),
        Err(e) => Json(json!({ "error": e })).into_response(),
    }
}

#[derive(Deserialize)]
pub struct MailApiBody {
    pub prompt: String,
    pub context: Option<String>,
    pub kind: Option<String>,
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
        body.kind.as_deref(),
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
}

async fn ai_api_event(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<EventApiBody>,
) -> impl IntoResponse {
    let prefs = state.prefs.lock().await.clone();
    if !prefs.ai_enabled {
        return Json(json!({ "error": "IA désactivée" })).into_response();
    }
    match generate_event(&prefs, body.prompt.trim()).await {
        Ok(v) => Json(json!({ "ok": true, "event": v })).into_response(),
        Err(e) => Json(json!({ "error": e })).into_response(),
    }
}

struct MailDraft {
    to: String,
    subject: String,
    body: String,
}

async fn generate_mail(
    prefs: &Prefs,
    prompt: &str,
    context: Option<&str>,
    kind: Option<&str>,
) -> Result<MailDraft, String> {
    let system = match kind {
        Some("reply") => {
            "Tu aides à rédiger / corriger un email (réponse). Le contexte contient le brouillon actuel (À, Cc, Cci, Reply-To, Sujet, Corps). Applique la consigne (corriger, reformuler, compléter…). Réponds en JSON strict uniquement: {\"to\",\"subject\",\"body\"}. Ne vide pas un champ déjà rempli sauf demande explicite. Pas de markdown."
        }
        _ => {
            "Tu aides à rédiger / corriger un email. Le contexte contient le brouillon actuel (À, Cc, Cci, Reply-To, Sujet, Corps). Applique la consigne (corriger, reformuler, compléter…). Réponds en JSON strict uniquement: {\"to\",\"subject\",\"body\"}. Ne vide pas un champ déjà rempli sauf demande explicite. Pas de markdown."
        }
    };
    let user = format!(
        "{}\n\nContexte:\n{}",
        prompt,
        context.unwrap_or("(aucun)")
    );
    let text = complete(prefs, system, &user).await?;
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
        // Fallback texte libre
        let (subject, body) = split_subject_body(cleaned);
        Ok(MailDraft {
            to: String::new(),
            subject,
            body,
        })
    }
}

async fn generate_event(prefs: &Prefs, prompt: &str) -> Result<Value, String> {
    let system = "Propose un événement calendrier. Réponds en JSON strict uniquement: {\"summary\",\"date\",\"start_time\",\"end_date\",\"end_time\",\"description\",\"location\",\"rrule\"} où date/end_date = YYYY-MM-DD, start_time/end_time = HH:MM, rrule = none|daily|weekly|monthly|yearly. Pas de markdown.";
    let text = complete(prefs, system, prompt).await?;
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

async fn complete(prefs: &Prefs, system: &str, user: &str) -> Result<String, String> {
    match prefs.ai_provider.as_str() {
        "gemini" => gemini_chat(prefs, system, user).await,
        _ => {
            // Ollama d'abord, fallback distant optionnel
            match ollama_chat(prefs, system, user).await {
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
            }
        }
    }
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

    // Gemma : pas de systemInstruction ; maxOutputTokens obligatoire (sinon 500 / coupe thinking).
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
    // Gemma renvoie parfois HTTP 500 côté Google — quelques retries aident.
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
            // Retry uniquement sur 5xx
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
    // Prefers non-thought parts when present
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
    let url = format!("{}/api/chat", prefs.ai_endpoint.trim_end_matches('/'));
    let system = {
        let pre = prefs.ai_ollama_preprompt.trim();
        if pre.is_empty() {
            system.to_string()
        } else {
            format!("{pre}\n\n{system}")
        }
    };
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
