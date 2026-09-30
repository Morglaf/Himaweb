// Pas de fenêtre console au double-clic / démarrage Windows.
// Logs visibles : lancer avec HIMAWEB_CONSOLE=1.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod accounts_config;
mod account_colors;
mod cache;
mod calendar_import;
mod cli;
mod config_fix;
mod contacts_import;
mod form_util;
mod plugins;
mod prefs;
mod routes;
mod sanitize;
mod state;
mod thunderbird;
mod tray;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use state::AppState;

/// Répertoire `static/` indépendant du CWD (ex. démarrage Windows → System32).
fn resolve_static_dir() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("static"));
            // cargo run : target/debug ou target/release
            candidates.push(dir.join("../static"));
            candidates.push(dir.join("../../static"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("static"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static"));

    for c in &candidates {
        if c.join("app.css").is_file() && c.join("app.js").is_file() {
            return c
                .canonicalize()
                .unwrap_or_else(|_| normalize_path(c));
        }
    }

    tracing::error!(
        "dossier static/ introuvable (CWD={:?}, exe={:?})",
        std::env::current_dir().ok(),
        std::env::current_exe().ok()
    );
    PathBuf::from("static")
}

fn normalize_path(p: &Path) -> PathBuf {
    // Évite les `..` inutiles si canonicalize échoue (chemin pas encore existant).
    p.components().collect()
}

#[tokio::main]
async fn main() {
    #[cfg(windows)]
    {
        if std::env::var_os("HIMAWEB_CONSOLE").is_some() {
            unsafe {
                let _ = windows_sys::Win32::System::Console::AllocConsole();
            }
        }
    }

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

    let static_dir = resolve_static_dir();
    tracing::info!("static assets: {}", static_dir.display());

    let app = Router::new()
        .merge(routes::router())
        .nest_service("/static", ServeDir::new(static_dir))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8787));
    let url = format!("http://{addr}");
    tracing::info!("HimaWeb écoute sur {url}");
    tray::spawn(url.clone());
    let _ = open::that(&url);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind 127.0.0.1:8787");

    let server = axum::serve(listener, app);
    tokio::select! {
        res = server => {
            if let Err(e) = res {
                tracing::error!("serveur: {e}");
            }
        }
        _ = async {
            loop {
                if tray::should_quit() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        } => {
            tracing::info!("arrêt demandé depuis le tray");
        }
    }
}
