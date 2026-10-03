pub mod ai;
pub mod calendar;
pub mod contacts;
mod attachments;
mod compose;
mod extras;
mod mail;
mod pages;
mod search;
mod settings;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<std::sync::Arc<AppState>> {
    Router::new()
        .merge(pages::router())
        .merge(mail::router())
        .merge(search::router())
        .merge(compose::router())
        .merge(attachments::router())
        .merge(contacts::router())
        .merge(calendar::router())
        .merge(settings::router())
        .merge(ai::router())
        .merge(extras::router())
}
