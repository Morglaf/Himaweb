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
    pub topbar_mode: String,
    pub ui_style: String,
    pub error: Option<String>,
    pub content: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(home))
        .route("/health", get(health))
        .route("/partials/side-widget", get(side_widget))
}

async fn health() -> impl IntoResponse {
    "ok"
}

#[derive(Template)]
#[template(path = "side_widget.html")]
struct SideWidgetTemplate {
    events: Vec<SideEventRow>,
    calendars: Vec<SideCalOpt>,
    default_date: String,
}

struct SideEventRow {
    summary: String,
    when: String,
}

struct SideCalOpt {
    id: String,
    name: String,
}

fn format_event_when(start: &str) -> String {
    // Formats courants: 20260930T100000Z, 20260930, 2026-09-30T10:00:00, 2026-09-30
    let s = start.trim();
    let (date, time) = if s.len() >= 15 && s.as_bytes().get(8) == Some(&b'T') {
        // YYYYMMDDTHHMMSS…
        let d = &s[0..8];
        let t = &s[9..];
        (
            format!("{}/{}/{}", &d[6..8], &d[4..6], &d[0..4]),
            if t.len() >= 4 {
                format!("{}:{}", &t[0..2], &t[2..4])
            } else {
                String::new()
            },
        )
    } else if s.len() >= 8 && s.as_bytes().get(4) != Some(&b'-') && s[..8].chars().all(|c| c.is_ascii_digit()) {
        let d = &s[0..8];
        (format!("{}/{}/{}", &d[6..8], &d[4..6], &d[0..4]), String::new())
    } else if s.len() >= 10 && s.as_bytes().get(4) == Some(&b'-') {
        let d = &s[0..10]; // YYYY-MM-DD
        let parts: Vec<_> = d.split('-').collect();
        let date = if parts.len() == 3 {
            format!("{}/{}/{}", parts[2], parts[1], parts[0])
        } else {
            d.to_string()
        };
        let time = if s.len() >= 16 && s.as_bytes().get(10) == Some(&b'T') {
            s[11..16].to_string()
        } else {
            String::new()
        };
        (date, time)
    } else {
        (s.to_string(), String::new())
    };
    if time.is_empty() {
        date
    } else {
        format!("{date} {time}")
    }
}

async fn side_widget(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let limit = state.prefs.lock().await.side_widget_events.max(1) as usize;
    // Toujours rafraîchir mois courant + suivant pour le panneau (sinon les RDV
    // du mois prochain manquent si le cache n'a vu que le mois affiché).
    if state.calendula_available {
        let _ = crate::routes::calendar::refresh_calendar_into_cache(&state).await;
    }
    let (events, calendars) = {
        let cache = state.cache.lock().await;
        let events = cache
            .load_upcoming_events(limit)
            .unwrap_or_default()
            .into_iter()
            .map(|(_id, summary, start, _cal)| SideEventRow {
                summary,
                when: format_event_when(&start),
            })
            .collect::<Vec<_>>();
        let calendars = cache
            .load_calendars()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, name, _)| SideCalOpt { id, name })
            .collect();
        (events, calendars)
    };
    let default_date = chrono::Local::now().format("%Y-%m-%d").to_string();
    match (SideWidgetTemplate {
        events,
        calendars,
        default_date,
    })
    .render()
    {
        Ok(html) => Html(html).into_response(),
        Err(e) => Html(format!("<p class=\"muted\">{e}</p>")).into_response(),
    }
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
    let prefs_snap = state.prefs.lock().await.clone();
    let list_icon = crate::routes::mail::mailbox_icon(&mailbox);
    let list_color = {
        let acc = q
            .account
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != prefs::ACCOUNT_ALL)
            .or_else(|| prefs_snap.selected_account());
        acc.map(|a| prefs_snap.account_color(a))
            .unwrap_or_else(|| "var(--accent)".into())
    };
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
                    topbar_mode: state.topbar_mode().await,
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
                topbar_mode: state.topbar_mode().await,
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
              <div class="list-title" id="list-title" style="--list-title-color: {list_color}">
                <i data-lucide="{list_icon}" id="list-mailbox-icon"></i>
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
              <div class="toolbar-actions">
                <label class="sr-only" for="mail-sort">Trier</label>
                <select id="mail-sort" name="sort" class="select sort-select"
                        title="Classement"
                        hx-get="/partials/envelopes"
                        hx-target="#envelope-list"
                        hx-swap="innerHTML"
                        hx-include="#mail-sort-ctx"
                        hx-vals='{{"page":"1"}}'>
                  <option value="date_desc" selected>Date ↓</option>
                  <option value="date_asc">Date ↑</option>
                  <option value="from_asc">De A→Z</option>
                  <option value="from_desc">De Z→A</option>
                  <option value="to_asc">À A→Z</option>
                  <option value="to_desc">À Z→A</option>
                </select>
                <div id="mail-sort-ctx" hidden>
                  <input type="hidden" name="mailbox" id="current-mailbox" value="{mailbox}" />
                  <input type="hidden" name="account" id="current-account" value="{account_val}" />
                </div>
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
          {{SIDE_WIDGET}}
        </div>
        "##
    );

    let side_widget_html = if prefs_snap.side_widget {
        r#"<div class="col-resizer" data-resize="side" title="Redimensionner"></div>
          <aside class="side-widget" id="side-widget" data-open="1">
            <button type="button" class="side-widget-tab" title="Aperçu" onclick="window.HimaWeb && HimaWeb.toggleSideWidget()">
              <i data-lucide="panel-right"></i>
            </button>
            <div class="side-widget-body"
                 hx-get="/partials/side-widget"
                 hx-trigger="load"
                 hx-swap="innerHTML"></div>
          </aside>"#
    } else {
        ""
    };
    let content = content.replace("{SIDE_WIDGET}", side_widget_html);

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
            topbar_mode: state.topbar_mode().await,
            ui_style,
            error: None,
            content,
        },
    )
    .await
}
