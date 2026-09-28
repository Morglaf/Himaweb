mod accounts_config;
mod account_colors;
mod cache;
mod calendar_import;
mod cli;
mod config_fix;
mod contacts_import;
mod prefs;
mod routes;
mod sanitize;
mod state;
mod thunderbird;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let state = match AppState::init().await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("échec initialisation: {e}");
            std::process::exit(1);
        }
    };

    // Warm caches en arrière-plan (contacts + calendrier)
    {
        let warm = Arc::clone(&state);
        tokio::spawn(async move {
            if warm.cardamum_available {
                match crate::routes::contacts::refresh_contacts_into_cache(&warm).await {
                    Ok(n) => tracing::info!("warm contacts: {n}"),
                    Err(e) => tracing::warn!("warm contacts: {e}"),
                }
            }
            if warm.calendula_available {
                match crate::routes::calendar::refresh_calendar_into_cache(&warm).await {
                    Ok(n) => tracing::info!("warm calendar events: {n}"),
                    Err(e) => tracing::warn!("warm calendar: {e}"),
                }
            }
        });
    }

    let app = Router::new()
        .merge(routes::router())
        .nest_service("/static", ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8787));
    tracing::info!("HimaWeb écoute sur http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind 127.0.0.1:8787");
    axum::serve(listener, app).await.expect("serveur");
}
