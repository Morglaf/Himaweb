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
    /// Tous les agendas (y compris masqués) — création de tâches
    task_calendars: Vec<SideCalOpt>,
    todos: Vec<crate::routes::calendar::TodoRow>,
    default_date: String,
    default_calendar: String,
    ai_enabled: bool,
    home_address_js: String,
    maps_provider: String,
    show_calendar: bool,
    show_tasks: bool,
    show_contacts: bool,
    side_widget_events: u16,
    has_rss: bool,
    has_freshrss: bool,
    plugin_panels: Vec<SidePluginPanel>,
}

struct SidePluginPanel {
    title: String,
    html: String,
}

struct SideEventRow {
    summary: String,
    when: String,
    location: String,
    maps_url: String,
    account: String,
    calendar_id: String,
    calendar_name: String,
    color: String,
}

struct SideCalOpt {
    id: String,
    /// Compte technique pour filtre tâches
    account: String,
    name: String,
    color: String,
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
    let prefs = state.prefs.lock().await.clone();
    let display_limit = prefs.side_widget_events.max(1) as usize;
    // Charger plus que la limite affichée pour que le filtre multi-agenda
    // côté client ait encore assez d’événements.
    let fetch_limit = display_limit.saturating_mul(4).clamp(30, 80);
    // Toujours servir le cache tout de suite : le rafraîchissement Calendula
    // (deux mois × tous les agendas) ne doit pas bloquer le premier rendu.
    if state.calendula_available {
        let bg = Arc::clone(&state);
        tokio::spawn(async move {
            match crate::routes::calendar::refresh_calendar_into_cache(&bg).await {
                Ok(n) => tracing::debug!("side-widget calendar warm: {n}"),
                Err(e) => tracing::debug!("side-widget calendar warm: {e}"),
            }
        });
    }
    let (events, calendars) = {
        let cache = state.cache.lock().await;
        let cal_rows = cache.load_calendars().unwrap_or_default();
        let events = cache
            .load_upcoming_events(fetch_limit)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_id, _summary, _start, cal, _location)| !prefs.is_calendar_hidden(cal))
            .map(|(_id, summary, start, cal, location)| {
                let maps_url = crate::routes::calendar::build_maps_url(
                    &prefs.maps_provider,
                    &prefs.home_address,
                    &location,
                );
                let account = crate::prefs::Prefs::cal_account_from_id(&cal).to_string();
                // Nom d’apparence du compte (pas « Lea — Leaaa »)
                let calendar_name = prefs.cal_account_label(&account);
                let color = prefs.calendar_color(&cal);
                SideEventRow {
                    summary,
                    when: format_event_when(&start),
                    location,
                    maps_url,
                    account,
                    calendar_id: cal,
                    calendar_name,
                    color,
                }
            })
            .collect::<Vec<_>>();
        // Chips : un par compte (label d’apparence), pas un par agenda Google
        let calendars: Vec<SideCalOpt> = {
            let prefs_c = prefs.clone();
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for (id, _, _) in &cal_rows {
                if prefs_c.is_calendar_hidden(id) {
                    continue;
                }
                let account = crate::prefs::Prefs::cal_account_from_id(id).to_string();
                if !seen.insert(account.clone()) {
                    continue;
                }
                out.push(SideCalOpt {
                    id: account.clone(),
                    account: account.clone(),
                    name: prefs_c.cal_account_label(&account),
                    color: prefs_c.calendar_color(id),
                });
            }
            out.sort_by(|a, b| a.name.cmp(&b.name));
            out
        };
        (events, calendars)
    };
    let all_cal_rows: Vec<crate::routes::calendar::CalRow> = {
        let cache = state.cache.lock().await;
        let prefs_c = prefs.clone();
        crate::routes::calendar::decorate_calendars_pub(
            &prefs_c,
            cache
                .load_calendars()
                .unwrap_or_default()
                .into_iter()
                .map(|(id, name, _)| (id, name))
                .collect(),
        )
    };
    let todos = if prefs.side_show_tasks
        && state.calendula_available
        && !all_cal_rows.is_empty()
    {
        let mut list =
            crate::routes::calendar::load_todos(&state, &all_cal_rows, "__all__").await;
        // Sidebar : tâches ouvertes uniquement (filtre calendrier côté UI)
        list.retain(|t| !t.completed);
        list
    } else {
        vec![]
    };
    let task_calendars: Vec<SideCalOpt> =
        crate::routes::calendar::select_task_calendars(&prefs, &all_cal_rows, &todos)
            .into_iter()
            .map(|c| SideCalOpt {
                id: c.id,
                account: c.account,
                name: c.name,
                color: c.color,
            })
            .collect();
    let default_date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let default_calendar = task_calendars
        .first()
        .or_else(|| calendars.first())
        .map(|c| c.id.clone())
        .unwrap_or_default();
    let plugin_panels: Vec<SidePluginPanel> = crate::plugins::sidebar_panels()
        .into_iter()
        .map(|p| SidePluginPanel {
            title: p.title,
            html: p.html,
        })
        .collect();
    let freshrss_ok = prefs.plugin_freshrss
        && crate::freshrss::is_configured(
            &prefs.freshrss_url,
            &prefs.freshrss_user,
            &prefs.freshrss_api_password,
        );
    let rss_ok = prefs.plugin_rss && !prefs.rss_feeds.is_empty();
    let events = if prefs.side_show_calendar {
        events
    } else {
        vec![]
    };
    match (SideWidgetTemplate {
        events,
        calendars,
        task_calendars,
        todos,
        default_date,
        default_calendar,
        ai_enabled: prefs.ai_enabled,
        home_address_js: serde_json::to_string(&prefs.home_address)
            .unwrap_or_else(|_| "\"\"".into()),
        maps_provider: prefs.maps_provider.clone(),
        show_calendar: prefs.side_show_calendar,
        show_tasks: prefs.side_show_tasks,
        show_contacts: prefs.side_show_contacts,
        side_widget_events: prefs.side_widget_events.max(1),
        has_rss: freshrss_ok || rss_ok,
        has_freshrss: freshrss_ok,
        plugin_panels,
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
    let prefs_snap = state.prefs.lock().await.clone();
    let mailbox = q.mailbox.unwrap_or_else(|| {
        prefs_snap
            .selected_ntfy_key()
            .unwrap_or("Inbox")
            .to_string()
    });
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
    let list_icon = crate::routes::mail::mailbox_icon(&mailbox);
    let list_label = if prefs::Prefs::is_ntfy_key(&mailbox) {
        prefs_snap.account_label(&mailbox)
    } else {
        crate::routes::mail::mailbox_label(&mailbox)
    };
    let list_color = {
        if prefs::Prefs::is_ntfy_key(&mailbox) {
            prefs_snap.account_color(&mailbox)
        } else {
            let acc = q
                .account
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != prefs::ACCOUNT_ALL)
                .or_else(|| prefs_snap.selected_account());
            acc.map(|a| prefs_snap.account_color(a))
                .unwrap_or_else(|| "var(--accent)".into())
        }
    };
    let ai_summary_btn = if prefs_snap.ai_enabled {
        r#"<button type="button" class="icon-btn" id="inbox-ai-summary-btn" title="Résumer la boîte (IA)" data-i18n-title="mail.ai_summary"
                        onclick="window.HimaWeb && HimaWeb.openInboxSummary()">
                  <i data-lucide="sparkles"></i>
                </button>"#
    } else {
        ""
    };
    let (theme, layout) = state.theme_layout().await;
    let ui_style = state.ui_style().await;

    if !state.himalaya_available {
        let has_cached = {
            let cache = state.cache.lock().await;
            cache.has_any_mailboxes().unwrap_or(false)
        };
        if !has_cached {
            return render_shell(
                &state,
                ShellTemplate {
                    title: "HimaWeb".into(),
                    active_tab: "mail".into(),
                    offline: true,
                    himalaya_available: false,

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
          <div class="col-resizer" data-resize="rail" title="Redimensionner" data-i18n-title="mail.resize"></div>

          <section class="list-pane">
            <div class="list-toolbar">
              <div class="list-title" id="list-title" style="--list-title-color: {list_color}">
                <i data-lucide="{list_icon}" id="list-mailbox-icon"></i>
                <span id="list-mailbox-label">{list_label}</span>
              </div>
              <form class="mail-search" method="get" action="/search">
                <input class="input mail-search-input" type="search" name="q"
                       placeholder="Recherche dans toutes les boîtes…"
                       data-i18n-placeholder="mail.search_ph"
                       autocomplete="off" />
                <button class="icon-btn" type="submit" title="Recherche" data-i18n-title="mail.search">
                  <i data-lucide="search"></i>
                </button>
              </form>
              <div class="toolbar-actions">
                <label class="sr-only" for="mail-sort" data-i18n="mail.sort">Trier</label>
                <select id="mail-sort" name="sort" class="select sort-select"
                        title="Classement" data-i18n-title="mail.sort_by"
                        hx-get="/partials/envelopes"
                        hx-target="#envelope-list"
                        hx-swap="innerHTML"
                        hx-include="#mail-sort-ctx"
                        hx-vals='{{"page":"1"}}'
                        hx-on::before-request="HimaWeb.showListLoading()">
                  <option value="date_desc" selected data-i18n="mail.sort.date_desc">Date ↓</option>
                  <option value="date_asc" data-i18n="mail.sort.date_asc">Date ↑</option>
                  <option value="from_asc" data-i18n="mail.sort.from_asc">De A→Z</option>
                  <option value="from_desc" data-i18n="mail.sort.from_desc">De Z→A</option>
                  <option value="to_asc" data-i18n="mail.sort.to_asc">À A→Z</option>
                  <option value="to_desc" data-i18n="mail.sort.to_desc">À Z→A</option>
                </select>
                <div id="mail-sort-ctx" hidden>
                  <input type="hidden" name="mailbox" id="current-mailbox" value="{mailbox}" />
                  <input type="hidden" name="account" id="current-account" value="{account_val}" />
                </div>
                {ai_summary_btn}
                <button class="icon-btn" title="Rafraîchir" data-i18n-title="common.refresh"
                        hx-get="/partials/envelopes?mailbox={mailbox_q}&page={page}{account_q}"
                        hx-target="#envelope-list" hx-swap="innerHTML"
                        hx-include="#mail-sort"
                        hx-disabled-elt="this"
                        hx-on::before-request="HimaWeb.showListLoading()">
                  <i data-lucide="refresh-cw"></i>
                </button>
              </div>
            </div>
            <div id="envelope-list" class="envelope-list" aria-busy="true"
                 hx-get="/partials/envelopes?mailbox={mailbox_q}&page={page}{account_q}"
                 hx-trigger="load"
                 hx-include="#mail-sort"
                 hx-disinherit="hx-include"
                 hx-swap="innerHTML">
              <div class="hw-skeleton-list" aria-hidden="true">
                <div class="hw-skeleton-row"><div class="hw-skeleton hw-skeleton-avatar"></div><div class="hw-skeleton-lines"><div class="hw-skeleton hw-skeleton-line w-40"></div><div class="hw-skeleton hw-skeleton-line w-80"></div></div></div>
                <div class="hw-skeleton-row"><div class="hw-skeleton hw-skeleton-avatar"></div><div class="hw-skeleton-lines"><div class="hw-skeleton hw-skeleton-line w-40"></div><div class="hw-skeleton hw-skeleton-line w-60"></div></div></div>
                <div class="hw-skeleton-row"><div class="hw-skeleton hw-skeleton-avatar"></div><div class="hw-skeleton-lines"><div class="hw-skeleton hw-skeleton-line w-40"></div><div class="hw-skeleton hw-skeleton-line w-100"></div></div></div>
                <div class="hw-skeleton-row"><div class="hw-skeleton hw-skeleton-avatar"></div><div class="hw-skeleton-lines"><div class="hw-skeleton hw-skeleton-line w-40"></div><div class="hw-skeleton hw-skeleton-line w-80"></div></div></div>
                <div class="hw-skeleton-row"><div class="hw-skeleton hw-skeleton-avatar"></div><div class="hw-skeleton-lines"><div class="hw-skeleton hw-skeleton-line w-40"></div><div class="hw-skeleton hw-skeleton-line w-60"></div></div></div>
                <div class="hw-skeleton-row"><div class="hw-skeleton hw-skeleton-avatar"></div><div class="hw-skeleton-lines"><div class="hw-skeleton hw-skeleton-line w-40"></div><div class="hw-skeleton hw-skeleton-line w-100"></div></div></div>
                <div class="hw-skeleton-row"><div class="hw-skeleton hw-skeleton-avatar"></div><div class="hw-skeleton-lines"><div class="hw-skeleton hw-skeleton-line w-40"></div><div class="hw-skeleton hw-skeleton-line w-80"></div></div></div>
              </div>
            </div>
          </section>

          <div class="col-resizer" data-resize="list" title="Redimensionner" data-i18n-title="mail.resize"></div>

          <section class="read-pane" id="message-pane">
            <div class="empty-read">
              <i data-lucide="mail-open"></i>
              <p data-i18n="mail.select_message">Sélectionnez un message</p>
            </div>
          </section>
          {{SIDE_WIDGET}}
        </div>
        <div id="inbox-summary-host"></div>
        "##
    );

    let side_widget_html = if prefs_snap.side_widget {
        r#"<div class="col-resizer" data-resize="side" title="Redimensionner" data-i18n-title="mail.resize"></div>
          <aside class="side-widget" id="side-widget" data-open="1">
            <button type="button" class="side-widget-tab" title="Panneau latéral" data-i18n-title="side.panel" onclick="window.HimaWeb && HimaWeb.toggleSideWidget()">
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
