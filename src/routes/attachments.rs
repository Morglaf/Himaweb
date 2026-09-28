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

    let account = state.account().await;
    match state
        .himalaya
        .download_attachment(
            &q.mailbox,
            &q.message_id,
            &q.attachment_id,
            tmp.path().to_str().unwrap_or("."),
            account.as_deref(),
        )
        .await
    {
        Ok(bytes) => {
            let filename = q.attachment_id.replace(['/', '\\'], "_");
            let mime = mime_guess::from_path(&filename)
                .first_or_octet_stream()
                .essence_str()
                .to_string();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{filename}\""),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}
