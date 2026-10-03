// Pas de fenêtre console au double-clic / démarrage Windows.
// Logs visibles : lancer avec HIMAWEB_CONSOLE=1.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod accounts_config;
mod account_colors;
mod assets;
mod attachments_class;
mod cache;
mod calendar_import;
mod cli;
mod config_backup;
mod config_fix;
mod contacts_import;
mod form_util;
mod i18n;
mod freshrss;
mod plugins;
mod prefs;
mod routes;
mod sanitize;
mod state;
mod thunderbird;
mod tray;
mod updates;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use state::AppState;

fn print_version() {
    println!("himaweb {}", env!("CARGO_PKG_VERSION"));
}

fn print_help() {
    println!(
        "HimaWeb {} — interface web locale pour l’écosystème Pimalaya\n\n\
         Usage: himaweb [OPTIONS]\n\n\
         Options:\n\
           -h, --help       Afficher cette aide\n\
           -V, --version    Afficher la version\n\n\
         Au démarrage, écoute http://127.0.0.1:8787 et ouvre le navigateur.\n\
         Variables utiles : HIMAWEB_CONSOLE=1 (logs Windows), HIMAWEB_*_BIN.\n",
        env!("CARGO_PKG_VERSION")
    );
}

/// Sous Windows (subsystem windows), rattache stdout au terminal parent
/// pour que `--help` / `--version` restent visibles depuis PowerShell / cmd.
#[cfg(windows)]
fn attach_parent_console() {
    unsafe {
        const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
        let _ = windows_sys::Win32::System::Console::AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn handle_cli_flags() -> bool {
    let mut version = false;
    let mut help = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-V" | "--version" => version = true,
            "-h" | "--help" => help = true,
            _ => {
                eprintln!("option inconnue: {arg} (voir --help)");
                return true;
            }
        }
    }
    if !version && !help {
        return false;
    }
    #[cfg(windows)]
    attach_parent_console();
    if help {
        print_help();
    } else {
        print_version();
    }
    true
}

#[tokio::main]
async fn main() {
    if handle_cli_flags() {
        return;
    }

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
                    Ok(n) => {
                        tracing::info!("warm contacts: {n}");
                        crate::routes::contacts::spawn_photo_sync(Arc::clone(&warm), Vec::new());
                    }
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

    tracing::info!(
        "static assets: embarqués dans le binaire (v{})",
        assets::version()
    );

    let open_browser = {
        let prefs = state.prefs.lock().await;
        prefs.open_browser_on_start
    };

    let app = routes::router()
        .merge(assets::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8787));
    let url = format!("http://{addr}");
    tracing::info!("HimaWeb écoute sur {url}");
    tray::spawn(url.clone());
    if open_browser {
        let _ = open::that(&url);
    } else {
        tracing::info!("ouverture navigateur désactivée (préférence)");
    }

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
