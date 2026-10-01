use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/attachments", get(download))
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    pub mailbox: String,
    pub message_id: String,
    pub attachment_id: String,
    pub account: Option<String>,
    /// `inline` pour prévisualisation navigateur ; sinon téléchargement.
    pub disposition: Option<String>,
}

async fn download(
    State(state): State<Arc<AppState>>,
    Query(q): Query<DownloadQuery>,
) -> Response {
    let _permit = state.cli_limit.acquire().await.ok();
    let tmp = match tempfile::tempdir() {
        Ok(t) => t,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let account = q
        .account
        .filter(|s| !s.is_empty())
        .or(state.account().await);

    // Les IDs Himalaya sont les positions MIME 1-based (`attachment list`).
    // Anciennes URLs pouvaient envoyer l'index 0-based du tableau `parts`.
    let (resolved_id, nice_name) = match state
        .himalaya
        .list_attachments(&q.mailbox, &q.message_id, account.as_deref())
        .await
    {
        Ok(list) if !list.is_empty() => {
            let exact = list.iter().find(|a| a.id == q.attachment_id);
            if let Some(a) = exact {
                (a.id.clone(), a.filename.clone())
            } else if let Ok(n) = q.attachment_id.parse::<u64>() {
                let as_one = (n + 1).to_string();
                if let Some(a) = list.iter().find(|a| a.id == as_one) {
                    (a.id.clone(), a.filename.clone())
                } else if let Some(a) = list.get(n as usize) {
                    (a.id.clone(), a.filename.clone())
                } else {
                    (q.attachment_id.clone(), q.attachment_id.clone())
                }
            } else {
                (q.attachment_id.clone(), q.attachment_id.clone())
            }
        }
        Ok(_) => (q.attachment_id.clone(), q.attachment_id.clone()),
        Err(_) => (q.attachment_id.clone(), q.attachment_id.clone()),
    };

    match state
        .himalaya
        .download_attachment(
            &q.mailbox,
            &q.message_id,
            &resolved_id,
            tmp.path().to_str().unwrap_or("."),
            account.as_deref(),
        )
        .await
    {
        Ok(bytes) => {
            let filename = nice_name.replace(['/', '\\'], "_");
            let mime = mime_guess::from_path(&filename)
                .first_or_octet_stream()
                .essence_str()
                .to_string();
            let inline = q
                .disposition
                .as_deref()
                .map(|d| d.eq_ignore_ascii_case("inline"))
                .unwrap_or(false);
            let disp = if inline {
                format!("inline; filename=\"{}\"", filename.replace('"', ""))
            } else {
                format!("attachment; filename=\"{}\"", filename.replace('"', ""))
            };
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CONTENT_DISPOSITION, disp),
                    (
                        header::CACHE_CONTROL,
                        "private, max-age=120".to_string(),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}
