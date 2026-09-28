use std::sync::Arc;

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use crate::prefs;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct HomeQuery {
    pub mailbox: Option<String>,
    pub page: Option<u32>,
    pub account: Option<String>,
}

#[derive(Template)]
#[template(path = "shell.html")]
pub struct ShellTemplate {
    pub title: String,
    pub active_tab: String,
    pub offline: bool,
    pub himalaya_available: bool,
    pub calendula_available: bool,
    pub cardamum_available: bool,
    pub theme: String,
    pub layout: String,
    pub ui_style: String,
    pub error: Option<String>,
    pub content: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(home))
        .route("/health", get(health))
}

async fn health() -> impl IntoResponse {
    "ok"
}

pub async fn render_shell(_state: &AppState, tpl: ShellTemplate) -> axum::response::Response {
    match tpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => Html(format!("<pre>template error: {e}</pre>")).into_response(),
    }
}

async fn home(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HomeQuery>,
) -> impl IntoResponse {
    let mailbox = q.mailbox.unwrap_or_else(|| "Inbox".into());
    let page = q.page.unwrap_or(1).max(1);
    let mailbox_q = urlencoding::encode(&mailbox);
    let account_q = q
        .account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("&account={}", urlencoding::encode(s)))
        .unwrap_or_default();
    let account_val = q
        .account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .replace('"', "&quot;");
    let (theme, layout) = state.theme_layout().await;
    let ui_style = state.ui_style().await;

    if !state.himalaya_available {
        let cached = {
            let cache = state.cache.lock().await;
            cache.load_mailboxes().unwrap_or_default()
        };
        if cached.is_empty() {
            return render_shell(
                &state,
                ShellTemplate {
                    title: "HimaWeb".into(),
                    active_tab: "mail".into(),
                    offline: true,
                    himalaya_available: false,
                    calendula_available: state.calendula_available,
                    cardamum_available: state.cardamum_available,
                    theme,
                    layout,
                    ui_style: ui_style.clone(),
                    error: Some(
                        "Himalaya est introuvable. Installez-le ou définissez HIMAWEB_HIMALAYA_BIN."
                            .into(),
                    ),
                    content: String::new(),
                },
            )
            .await;
        }
    }

    if !prefs::himalaya_config_exists() {
        return render_shell(
            &state,
            ShellTemplate {
                title: "HimaWeb".into(),
                active_tab: "mail".into(),
                offline: false,
                himalaya_available: state.himalaya_available,
                calendula_available: state.calendula_available,
                cardamum_available: state.cardamum_available,
                theme: theme.clone(),
                layout: layout.clone(),
                ui_style: ui_style.clone(),
                error: Some(format!(
                    "Aucun compte Himalaya configuré ({}). Allez dans Paramètres.",
                    prefs::himalaya_config_path().display()
                )),
                content: r#"<p class="muted"><a class="btn" href="/settings">Ouvrir les paramètres</a></p>"#.into(),
            },
        )
        .await;
    }

    let content = format!(
        r##"
        <div class="gmail-shell layout-{layout}" id="mail-root">
          <aside class="rail" id="sidebar"
                 hx-get="/partials/sidebar?mailbox={mailbox_q}{account_q}"
                 hx-trigger="load"
                 hx-swap="innerHTML"></aside>
          <div class="col-resizer" data-resize="rail" title="Redimensionner"></div>

          <section class="list-pane">
            <div class="list-toolbar">
              <div class="list-title">
                <i data-lucide="mails"></i>
                <span id="list-mailbox-label">{mailbox}</span>
              </div>
              <form class="mail-search" method="get" action="/search">
                <input class="input mail-search-input" type="search" name="q"
                       placeholder="Recherche dans toutes les boîtes…"
                       autocomplete="off" />
                <button class="icon-btn" type="submit" title="Recherche">
                  <i data-lucide="search"></i>
                </button>
              </form>
              <label class="sr-only" for="mail-sort">Trier</label>
              <select id="mail-sort" name="sort" class="select sort-select"
                      hx-get="/partials/envelopes"
                      hx-target="#envelope-list"
                      hx-swap="innerHTML"
                      hx-include="#mail-sort-ctx"
                      hx-vals='{{"page":"1"}}'>
                <option value="date_desc" selected>Date ↓</option>
                <option value="date_asc">Date ↑</option>
                <option value="from_asc">Expéditeur A→Z</option>
                <option value="from_desc">Expéditeur Z→A</option>
                <option value="to_asc">Destinataire A→Z</option>
                <option value="to_desc">Destinataire Z→A</option>
              </select>
              <div id="mail-sort-ctx" hidden>
                <input type="hidden" name="mailbox" id="current-mailbox" value="{mailbox}" />
                <input type="hidden" name="account" id="current-account" value="{account_val}" />
              </div>
              <div class="toolbar-actions">
                <a class="btn primary" href="/compose?mailbox={mailbox_q}">
                  <i data-lucide="pen-square"></i> Nouveau
                </a>
                <button class="icon-btn" title="Rafraîchir"
                        hx-get="/partials/envelopes?mailbox={mailbox_q}&page={page}{account_q}"
                        hx-target="#envelope-list" hx-swap="innerHTML"
                        hx-include="#mail-sort"
                        onclick="setTimeout(()=>lucide.createIcons(),50)">
                  <i data-lucide="refresh-cw"></i>
                </button>
              </div>
            </div>
            <div id="envelope-list" class="envelope-list"
                 hx-get="/partials/envelopes?mailbox={mailbox_q}&page={page}{account_q}"
                 hx-trigger="load"
                 hx-include="#mail-sort"
                 hx-swap="innerHTML">
              <div class="loading">Chargement des messages…</div>
            </div>
          </section>

          <div class="col-resizer" data-resize="list" title="Redimensionner"></div>

          <section class="read-pane" id="message-pane">
            <div class="empty-read">
              <i data-lucide="mail-open"></i>
              <p>Sélectionnez un message</p>
            </div>
          </section>
        </div>
        "##
    );

    render_shell(
        &state,
        ShellTemplate {
            title: "HimaWeb".into(),
            active_tab: "mail".into(),
            offline: false,
            himalaya_available: state.himalaya_available,
            calendula_available: state.calendula_available,
            cardamum_available: state.cardamum_available,
            theme,
            layout,
            ui_style,
            error: None,
            content,
        },
    )
    .await
}
