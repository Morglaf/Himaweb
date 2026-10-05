use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use chrono::{Datelike, Duration, Local, NaiveDate};
use serde::Deserialize;

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/calendar", get(calendar_page))
        .route("/calendar/{id}/events", get(events_fragment))
        .route("/calendar/create", post(create_event))
        .route("/calendar/update", post(update_event))
        .route("/calendar/delete", post(delete_event))
        .route("/calendar/todo/create", post(create_todo))
        .route("/calendar/todo/toggle", post(toggle_todo))
        .route("/calendar/todo/delete", post(delete_todo))
}

#[derive(Deserialize)]
pub struct CalQuery {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub calendar: Option<String>,
    pub view: Option<String>,
    pub refresh: Option<String>,
    pub msg: Option<String>,
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
#[template(path = "calendar.html")]
struct CalendarTemplate {
    pub calendula_available: bool,
    pub calendars: Vec<CalRow>,
    /// Tous les agendas (masqués inclus) pour créer des tâches
    pub task_calendars: Vec<CalRow>,
    pub account_hints: Vec<CalAccountHint>,
    pub current_id: String,
    pub current_id_enc: String,
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub view: String,
    pub title_label: String,
    pub month_name: String,
    pub dow: Vec<String>,
    pub prev_year: i32,
    pub prev_month: u32,
    pub prev_day: u32,
    pub next_year: i32,
    pub next_month: u32,
    pub next_day: u32,
    pub days: Vec<DayCell>,
    pub week_days: Vec<WeekDayCol>,
    pub events: Vec<EventRow>,
    pub error: Option<String>,
    pub flash: Option<String>,
    pub ai_enabled: bool,
    pub default_date: String,
    pub default_calendar: String,
    pub home_address_js: String,
    pub maps_provider: String,
    pub todos: Vec<TodoRow>,
}

pub struct TodoRow {
    pub id: String,
    pub calendar_id: String,
    /// Compte technique (filtre chips)
    pub account: String,
    /// Label Apparence des agendas (ex. EHESS)
    pub calendar_name: String,
    pub color: String,
    pub summary: String,
    pub due: String,
    pub due_label: String,
    pub completed: bool,
}

#[derive(Clone)]
pub struct CalRow {
    pub id: String,
    pub account: String,
    pub name: String,
    pub color: String,
    pub hidden: bool,
}

pub struct CalAccountHint {
    pub name: String,
    pub label: String,
    pub icon: String,
    pub color: String,
    /// `__acc__{name}` encodé pour les liens légende
    pub filter_id_enc: String,
    pub selected: bool,
}

/// Préfixe URL pour filtrer tous les agendas d’un compte CalDAV.
pub const ACC_FILTER_PREFIX: &str = "__acc__";

pub struct EventPreview {
    pub time: String,
    pub title: String,
    pub ev_json: String,
    pub color: String,
}

pub struct DayCell {
    pub day: u32,
    pub in_month: bool,
    pub has_events: bool,
    pub selected: bool,
    pub previews: Vec<EventPreview>,
    pub more: u32,
}

pub struct WeekDayCol {
    pub label: String,
    pub day: u32,
    pub month: u32,
    pub year: i32,
    pub selected: bool,
    pub events: Vec<EventRow>,
}

#[derive(Clone)]
pub struct EventRow {
    pub id: String,
    pub calendar_id: String,
    pub summary: String,
    pub date: String,
    pub date_iso: String,
    pub end_iso: String,
    pub start_time: String,
    pub end_time: String,
    pub when: String,
    pub time: String,
    pub description: String,
    pub location: String,
    pub rrule: String,
    /// URL trajet (vide si pas de lieu)
    pub maps_url: String,
    /// Couleur compte Calendula (`#rrggbb`)
    pub color: String,
    /// JSON compact pour data-ev (échappé HTML-safe)
    pub ev_json: String,
}

async fn calendar_page(
    State(state): State<Arc<AppState>>,
    Query(q): Query<CalQuery>,
) -> impl IntoResponse {
    let now = Local::now().date_naive();
    let year = q.year.unwrap_or(now.year());
    let month = q.month.unwrap_or(now.month());
    let day = q.day.unwrap_or(now.day()).clamp(1, days_in_month(year, month));
    let view = match q.view.as_deref() {
        Some("day") | Some("jour") => "day",
        Some("week") | Some("semaine") => "week",
        _ => "month",
    }
    .to_string();
    let force = q.refresh.as_deref() == Some("1");

    let (calendars, current_id, events, error, account_hints, from_cache) =
        load_calendar_data(&state, q.calendar.as_deref(), year, month, force).await;

    // Refresh bg si on a servi le cache
    if from_cache && !force {
        let bg = Arc::clone(&state);
        let y = year;
        let m = month;
        tokio::spawn(async move {
            let _ = refresh_calendar_month(&bg, y, m, true).await;
        });
    }

    let prefs = state.prefs.lock().await.clone();
    let loc = crate::i18n::normalize_locale(&prefs.locale);
    let selected = NaiveDate::from_ymd_opt(year, month, day).unwrap_or(now);
    let (prev, next, title_label) = nav_for_view(&view, selected, loc);
    let month_name = crate::i18n::month_name(loc, month);
    let dow: Vec<String> = (0..7).map(|i| crate::i18n::dow_short(loc, i)).collect();
    let events: Vec<EventRow> = events
        .into_iter()
        .map(|e| with_maps(e, &prefs.maps_provider, &prefs.home_address))
        .collect();

    let days = if view == "month" {
        build_month_grid(year, month, day, &events)
    } else {
        vec![]
    };
    let week_days = if view == "week" {
        build_week_cols(selected, &events, loc)
    } else {
        vec![]
    };

    let day_events: Vec<EventRow> = events
        .iter()
        .filter(|e| parse_day(&e.date, year, month) == Some(day))
        .cloned()
        .collect();

    let list_events = match view.as_str() {
        "day" => day_events,
        "week" => events
            .into_iter()
            .filter(|e| event_in_week(e, selected))
            .collect(),
        _ => day_events,
    };

    let todos = if state.calendula_available {
        // Tâches : tous les agendas (y compris masqués) — filtre dédié côté UI
        let all_for_todos: Vec<CalRow> = {
            let cache = state.cache.lock().await;
            let prefs_t = state.prefs.lock().await.clone();
            decorate_calendars(
                &prefs_t,
                cache
                    .load_calendars()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(id, name, _)| (id, name))
                    .collect(),
            )
        };
        let todos = load_todos(&state, &all_for_todos, "__all__").await;
        let task_calendars = select_task_calendars(&prefs, &all_for_todos, &todos);
        (todos, task_calendars)
    } else {
        (vec![], vec![])
    };
    let (todos, task_calendars) = todos;
    let current_id_enc = urlencoding::encode(&current_id).into_owned();
    let default_date = format!("{year:04}-{month:02}-{day:02}");
    let default_calendar = task_calendars
        .first()
        .or_else(|| calendars.first())
        .map(|c| c.id.clone())
        .unwrap_or_default();
    let account_hints = mark_account_hints_selected(account_hints, &current_id);
    let inner = CalendarTemplate {
        calendula_available: state.calendula_available,
        calendars,
        task_calendars,
        account_hints,
        current_id: current_id.clone(),
        current_id_enc,
        year,
        month,
        day,
        view: view.clone(),
        title_label,
        month_name,
        dow,
        prev_year: prev.year(),
        prev_month: prev.month(),
        prev_day: prev.day(),
        next_year: next.year(),
        next_month: next.month(),
        next_day: next.day(),
        days,
        week_days,
        events: list_events,
        error,
        flash: q.msg,
        ai_enabled: prefs.ai_enabled,
        default_date,
        default_calendar,
        home_address_js: serde_json::to_string(&prefs.home_address).unwrap_or_else(|_| "\"\"".into()),
        maps_provider: prefs.maps_provider.clone(),
        todos,
    };

    let content = match inner.render() {
        Ok(c) => c,
        Err(e) => format!("<pre>{e}</pre>"),
    };

    let (theme, layout) = state.theme_layout().await;
    let shell = ShellTemplate {
        title: "HimaWeb — Calendrier".into(),
        active_tab: "calendar".into(),
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

#[derive(Template)]
#[template(path = "calendar_events.html")]
struct EventsFragment {
    pub events: Vec<EventRow>,
    pub error: Option<String>,
}

async fn events_fragment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<CalQuery>,
) -> impl IntoResponse {
    let now = Local::now().date_naive();
    let year = q.year.unwrap_or(now.year());
    let month = q.month.unwrap_or(now.month());
    let day = q.day.unwrap_or(now.day());

    let (_cals, _cur, events, error, _hints, _) =
        load_calendar_data(&state, Some(&id), year, month, false).await;
    let day_events: Vec<EventRow> = events
        .into_iter()
        .filter(|e| parse_day(&e.date, year, month) == Some(day))
        .collect();

    let tpl = EventsFragment {
        events: day_events,
        error,
    };
    match tpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => Html(format!("<pre>{e}</pre>")).into_response(),
    }
}

fn nav_for_view(view: &str, selected: NaiveDate, locale: &str) -> (NaiveDate, NaiveDate, String) {
    match view {
        "day" => {
            let label = format!(
                "{} {} {}",
                crate::i18n::weekday_name(locale, selected.weekday()),
                selected.day(),
                crate::i18n::month_name(locale, selected.month())
            );
            (
                selected - Duration::days(1),
                selected + Duration::days(1),
                label,
            )
        }
        "week" => {
            let monday = selected
                - Duration::days(selected.weekday().num_days_from_monday() as i64);
            let sunday = monday + Duration::days(6);
            let label = format!(
                "{} {} {} — {} {}",
                crate::i18n::t(locale, "cal.week_of"),
                monday.day(),
                crate::i18n::month_name(locale, monday.month()),
                sunday.day(),
                crate::i18n::month_name(locale, sunday.month())
            );
            (
                monday - Duration::days(7),
                monday + Duration::days(7),
                label,
            )
        }
        _ => {
            let label = format!(
                "{} {}",
                crate::i18n::month_name(locale, selected.month()),
                selected.year()
            );
            let prev = if selected.month() == 1 {
                NaiveDate::from_ymd_opt(selected.year() - 1, 12, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(selected.year(), selected.month() - 1, 1).unwrap()
            };
            let next = if selected.month() == 12 {
                NaiveDate::from_ymd_opt(selected.year() + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(selected.year(), selected.month() + 1, 1).unwrap()
            };
            (prev, next, label)
        }
    }
}

fn event_in_week(e: &EventRow, selected: NaiveDate) -> bool {
    let monday = selected - Duration::days(selected.weekday().num_days_from_monday() as i64);
    let sunday = monday + Duration::days(6);
    let Some(d) = parse_full_date(&e.date) else {
        return false;
    };
    d >= monday && d <= sunday
}

async fn load_calendar_data(
    state: &AppState,
    preferred: Option<&str>,
    year: i32,
    month: u32,
    force_refresh: bool,
) -> (
    Vec<CalRow>,
    String,
    Vec<EventRow>,
    Option<String>,
    Vec<CalAccountHint>,
    bool,
) {
    let prefs = state.prefs.lock().await.clone();
    let account_hints: Vec<CalAccountHint> = build_account_hints(&prefs);

    if !state.calendula_available {
        return (
            vec![],
            String::new(),
            vec![],
            Some("Calendula est introuvable. Installez-le pour activer le calendrier.".into()),
            account_hints,
            false,
        );
    }

    // Cache-first
    if !force_refresh {
        let cached_cals = {
            let cache = state.cache.lock().await;
            cache.load_calendars().unwrap_or_default()
        };
        if !cached_cals.is_empty() {
            let calendars: Vec<CalRow> = decorate_calendars(
                &prefs,
                cached_cals
                    .iter()
                    .map(|(id, name, _)| (id.clone(), name.clone()))
                    .collect(),
            );
            let visible: Vec<CalRow> = calendars.iter().filter(|c| !c.hidden).cloned().collect();
            let current_id = normalize_cal_selection(preferred, &calendars);
            let ids = selection_calendar_ids(&current_id, &calendars, true);
            let cached_ev = {
                let cache = state.cache.lock().await;
                cache
                    .load_events_in_month(&ids, year, month)
                    .unwrap_or_default()
            };
            if !cached_ev.is_empty() {
                let events: Vec<EventRow> = cached_ev
                    .into_iter()
                    .map(|(cid, id, summary, start, end, desc, loc)| {
                        with_ev_json(decorate_event_color(
                            &prefs,
                            to_event_row(
                                id,
                                cid,
                                summary,
                                start,
                                end,
                                desc,
                                loc,
                                String::new(),
                            ),
                        ))
                    })
                    .collect();
                return (
                    visible,
                    current_id,
                    events,
                    None,
                    account_hints,
                    true,
                );
            }
            // Calendriers en cache mais pas d'events pour ce mois → fetch CLI
            // (retourner quand même la liste décorée pour l’UI)
            if !force_refresh && !visible.is_empty() {
                // continue to fetch below, but keep going
            }
        }
    }

    match fetch_and_cache_month(state, preferred, year, month, false).await {
        Ok((calendars, current_id, events)) => {
            let calendars = decorate_calendars(
                &prefs,
                calendars
                    .into_iter()
                    .map(|c| (c.id, c.name))
                    .collect(),
            );
            let visible: Vec<CalRow> = calendars.iter().filter(|c| !c.hidden).cloned().collect();
            let current_id = normalize_cal_selection(Some(&current_id), &calendars);
            let events: Vec<EventRow> = events
                .into_iter()
                .filter(|e| event_matches_selection(&current_id, &e.calendar_id, &prefs))
                .map(|e| with_ev_json(decorate_event_color(&prefs, e)))
                .collect();
            // Agenda unique hors liste visible → l’ajouter pour l’UI
            let ui_cals = if !is_all_selection(&current_id)
                && parse_account_filter(&current_id).is_none()
                && visible.iter().all(|c| c.id != current_id)
            {
                let mut v = visible;
                if let Some(c) = calendars.iter().find(|c| c.id == current_id) {
                    v.insert(0, c.clone());
                }
                v
            } else {
                visible
            };
            (ui_cals, current_id, events, None, account_hints, false)
        }
        Err(e) => {
            // Fallback cache même si refresh a échoué
            let cached_cals = {
                let cache = state.cache.lock().await;
                cache.load_calendars().unwrap_or_default()
            };
            if !cached_cals.is_empty() {
                let calendars: Vec<CalRow> = decorate_calendars(
                    &prefs,
                    cached_cals
                        .iter()
                        .map(|(id, name, _)| (id.clone(), name.clone()))
                        .collect(),
                );
                let visible: Vec<CalRow> =
                    calendars.iter().filter(|c| !c.hidden).cloned().collect();
                let current_id = normalize_cal_selection(preferred, &calendars);
                let ids = selection_calendar_ids(&current_id, &calendars, true);
                let events: Vec<EventRow> = {
                    let cache = state.cache.lock().await;
                    cache
                        .load_events_in_month(&ids, year, month)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(c, i, s, start, e, d, loc)| {
                            with_ev_json(decorate_event_color(
                                &prefs,
                                to_event_row(i, c, s, start, e, d, loc, String::new()),
                            ))
                        })
                        .collect()
                };
                return (
                    visible,
                    current_id,
                    events,
                    Some(format!("{e} — cache local.")),
                    account_hints,
                    true,
                );
            }
            (
                vec![],
                String::new(),
                vec![],
                Some(e),
                account_hints,
                false,
            )
        }
    }
}

async fn fetch_and_cache_month(
    state: &AppState,
    preferred: Option<&str>,
    year: i32,
    month: u32,
    background: bool,
) -> Result<(Vec<CalRow>, String, Vec<EventRow>), String> {
    let Some(client) = &state.calendula else {
        return Err("Calendula indisponible.".into());
    };
    let from = format!("{year:04}-{month:02}-01");
    let to = format!("{year:04}-{month:02}-{:02}", days_in_month(year, month));
    let pool = if background {
        state.cli_bg_limit.clone()
    } else {
        state.cli_limit.clone()
    };
    let list = {
        let _permit = pool.acquire().await.map_err(|e| e.to_string())?;
        client.list_calendars().await.map_err(|e| {
            format!("{e} — vérifiez l’URL CalDAV (home complète) et le mot de passe dans Paramètres.")
        })?
    };
    if list.is_empty() {
        return Err(
            "Aucun calendrier renvoyé. Ajoutez un compte CalDAV avec une URL complète.".into(),
        );
    }

    let cal_tuples: Vec<(String, String, String)> = list
        .iter()
        .map(|c| (c.id.clone(), c.name.clone(), c.account.clone()))
        .collect();
    {
        let cache = state.cache.lock().await;
        let _ = cache.save_calendars(&cal_tuples);
    }

    let prefs = state.prefs.lock().await.clone();
    let calendars: Vec<CalRow> = list
        .iter()
        .map(|c| CalRow {
            id: c.id.clone(),
            account: if c.account.is_empty() {
                crate::prefs::Prefs::cal_account_from_id(&c.id).to_string()
            } else {
                c.account.clone()
            },
            name: c.name.clone(),
            color: String::new(),
            hidden: prefs.is_calendar_hidden(&c.id),
        })
        .collect();

    let current_id = normalize_cal_selection(preferred, &calendars);
    let fetch_ids = selection_calendar_ids(&current_id, &calendars, true);

    let targets: Vec<&CalRow> = calendars
        .iter()
        .filter(|c| fetch_ids.iter().any(|id| id == &c.id))
        .collect();

    let mut all_events = Vec::new();
    let mut errs = Vec::new();
    for c in targets {
        let list_res = {
            let _permit = pool.acquire().await.map_err(|e| e.to_string())?;
            client.list_events(&c.id, Some(&from), Some(&to)).await
        };
        match list_res {
            Ok(list) => {
                let mut enriched = list;
                // Enrichir LOCATION / DESCRIPTION / RRULE via event read (parallèle),
                // uniquement sur le chemin interactif (pas le warm background).
                if !background && !enriched.is_empty() {
                    let mut join = tokio::task::JoinSet::new();
                    for ev in &enriched {
                        if !ev.location.is_empty() && !ev.description.is_empty() {
                            continue;
                        }
                        let client = client.clone();
                        let cal = c.id.clone();
                        let id = ev.id.clone();
                        let pool = pool.clone();
                        join.spawn(async move {
                            let _p = pool.acquire().await.ok();
                            let fields = client.enrich_event_from_ical(&cal, &id).await.ok();
                            (id, fields)
                        });
                    }
                    let mut by_id = std::collections::BTreeMap::new();
                    while let Some(res) = join.join_next().await {
                        if let Ok((id, Some((desc, loc, rr)))) = res {
                            by_id.insert(id, (desc, loc, rr));
                        }
                    }
                    for ev in &mut enriched {
                        if let Some((desc, loc, rr)) = by_id.remove(&ev.id) {
                            if ev.description.is_empty() && !desc.is_empty() {
                                ev.description = desc;
                            }
                            if ev.location.is_empty() && !loc.is_empty() {
                                ev.location = loc;
                            }
                            if ev.rrule.is_empty() && !rr.is_empty() {
                                ev.rrule = rr;
                            }
                        }
                    }
                }
                let rows: Vec<(String, String, String, String, String, String)> = enriched
                    .iter()
                    .map(|e| {
                        (
                            e.id.clone(),
                            e.summary.clone(),
                            e.date.clone(),
                            e.end.clone(),
                            e.description.clone(),
                            e.location.clone(),
                        )
                    })
                    .collect();
                {
                    let cache = state.cache.lock().await;
                    let _ = cache.replace_calendar_events_in_month(&c.id, year, month, &rows);
                }
                all_events.extend(enriched.into_iter().map(|e| {
                    with_ev_json(to_event_row(
                        e.id,
                        c.id.clone(),
                        e.summary,
                        e.date,
                        e.end,
                        e.description,
                        e.location,
                        normalize_rrule_display(&e.rrule),
                    ))
                }));
            }
            Err(e) => errs.push(format!("{}: {e}", c.name)),
        }
    }
    if all_events.is_empty() && !errs.is_empty() {
        return Err(errs.join(" · "));
    }
    Ok((calendars, current_id, all_events))
}

/// Warm / refresh mois courant + mois suivant pour le cache (widget agenda).
pub async fn refresh_calendar_into_cache(state: &AppState) -> Result<usize, String> {
    let now = Local::now().date_naive();
    let mut n = refresh_calendar_month(state, now.year(), now.month(), true).await?;
    if let Some(next) = now.checked_add_months(chrono::Months::new(1)) {
        match refresh_calendar_month(state, next.year(), next.month(), true).await {
            Ok(m) => n = n.saturating_add(m),
            Err(e) => tracing::warn!("warm calendar next month: {e}"),
        }
    }
    Ok(n)
}

/// `background` : utiliser le pool CLI de fond plutôt que le pool interactif.
pub async fn refresh_calendar_month(
    state: &AppState,
    year: i32,
    month: u32,
    background: bool,
) -> Result<usize, String> {
    let (cals, _, events) =
        fetch_and_cache_month(state, Some("__all__"), year, month, background).await?;
    Ok(cals.len().saturating_add(events.len()))
}

fn to_event_row(
    id: String,
    calendar_id: String,
    summary: String,
    date: String,
    end: String,
    description: String,
    location: String,
    rrule: String,
) -> EventRow {
    let when = format_event_when(&date);
    let time = format_event_time(&date);
    let date_iso = iso_date(&date);
    let end_iso = {
        let e = iso_date(&end);
        if e.is_empty() {
            date_iso.clone()
        } else {
            e
        }
    };
    let start_time = iso_time(&date);
    let end_time = {
        let t = iso_time(&end);
        if t.is_empty() {
            start_time.clone()
        } else {
            t
        }
    };
    EventRow {
        id,
        calendar_id,
        summary,
        date,
        date_iso,
        end_iso,
        start_time,
        end_time,
        when,
        time,
        description,
        location,
        rrule,
        maps_url: String::new(),
        color: String::new(),
        ev_json: String::new(),
    }
}

fn with_ev_json(mut e: EventRow) -> EventRow {
    e.ev_json = serde_json::json!({
        "id": e.id,
        "calendar": e.calendar_id,
        "summary": e.summary,
        "date": e.date_iso,
        "endDate": e.end_iso,
        "startTime": e.start_time,
        "endTime": e.end_time,
        "description": e.description,
        "location": e.location,
        "rrule": e.rrule,
        "color": e.color,
    })
    .to_string();
    e
}

pub fn decorate_calendars_pub(prefs: &crate::prefs::Prefs, rows: Vec<(String, String)>) -> Vec<CalRow> {
    decorate_calendars(prefs, rows)
}

fn is_all_selection(id: &str) -> bool {
    id.is_empty() || id == "__all__"
}

fn parse_account_filter(id: &str) -> Option<&str> {
    id.strip_prefix(ACC_FILTER_PREFIX)
        .filter(|s| !s.is_empty())
}

fn account_filter_id(account: &str) -> String {
    format!("{ACC_FILTER_PREFIX}{account}")
}

fn normalize_cal_selection(preferred: Option<&str>, calendars: &[CalRow]) -> String {
    let Some(p) = preferred.map(str::trim).filter(|s| !s.is_empty()) else {
        return "__all__".into();
    };
    if p == "__all__" {
        return "__all__".into();
    }
    if let Some(acc) = parse_account_filter(p) {
        if calendars
            .iter()
            .any(|c| crate::prefs::Prefs::cal_account_from_id(&c.id) == acc)
        {
            return account_filter_id(acc);
        }
        return "__all__".into();
    }
    if calendars.iter().any(|c| c.id == p) {
        return p.to_string();
    }
    "__all__".into()
}

fn selection_calendar_ids(selection: &str, calendars: &[CalRow], visible_only: bool) -> Vec<String> {
    if is_all_selection(selection) {
        calendars
            .iter()
            .filter(|c| !visible_only || !c.hidden)
            .map(|c| c.id.clone())
            .collect()
    } else if let Some(acc) = parse_account_filter(selection) {
        calendars
            .iter()
            .filter(|c| crate::prefs::Prefs::cal_account_from_id(&c.id) == acc)
            .filter(|c| !visible_only || !c.hidden)
            .map(|c| c.id.clone())
            .collect()
    } else {
        vec![selection.to_string()]
    }
}

fn event_matches_selection(selection: &str, calendar_id: &str, prefs: &crate::prefs::Prefs) -> bool {
    if is_all_selection(selection) {
        return !prefs.is_calendar_hidden(calendar_id);
    }
    if let Some(acc) = parse_account_filter(selection) {
        return crate::prefs::Prefs::cal_account_from_id(calendar_id) == acc
            && !prefs.is_calendar_hidden(calendar_id);
    }
    calendar_id == selection
}

fn build_account_hints(prefs: &crate::prefs::Prefs) -> Vec<CalAccountHint> {
    crate::calendar_import::list_calendula_accounts()
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            let filter_id = account_filter_id(&a.name);
            let filter_id_enc = urlencoding::encode(&filter_id).into_owned();
            CalAccountHint {
                label: prefs.cal_account_label(&a.name),
                icon: prefs.cal_account_icon(&a.name),
                color: prefs.cal_account_color(&a.name),
                name: a.name,
                filter_id_enc,
                selected: false,
            }
        })
        .collect()
}

fn mark_account_hints_selected(
    mut hints: Vec<CalAccountHint>,
    current_id: &str,
) -> Vec<CalAccountHint> {
    let selected_acc = parse_account_filter(current_id).map(str::to_string);
    for h in &mut hints {
        h.selected = selected_acc.as_deref() == Some(h.name.as_str());
    }
    hints
}

/// Agendas pour les tâches : liste manuelle (Paramètres) ou défaut 1 agenda / compte.
/// Labels = Apparence des agendas (un seul par compte → label compte).
pub(crate) fn select_task_calendars(
    prefs: &crate::prefs::Prefs,
    all: &[CalRow],
    _todos: &[TodoRow],
) -> Vec<CalRow> {
    let known: Vec<(String, String)> = all
        .iter()
        .map(|c| (c.id.clone(), c.name.clone()))
        .collect();
    let selected = prefs.effective_task_calendar_ids(&known);
    if selected.is_empty() {
        return vec![];
    }
    let mut out: Vec<CalRow> = Vec::new();
    for id in &selected {
        let Some(c) = all.iter().find(|c| &c.id == id) else {
            continue;
        };
        let account = crate::prefs::Prefs::cal_account_from_id(&c.id).to_string();
        let same_acc = selected
            .iter()
            .filter(|sid| crate::prefs::Prefs::cal_account_from_id(sid) == account)
            .count();
        let name = if same_acc <= 1 {
            prefs.cal_account_label(&account)
        } else {
            c.name.clone()
        };
        out.push(CalRow {
            id: c.id.clone(),
            account: account.clone(),
            name,
            color: c.color.clone(),
            hidden: c.hidden,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    out
}

fn decorate_calendars(prefs: &crate::prefs::Prefs, rows: Vec<(String, String)>) -> Vec<CalRow> {
    let mut out: Vec<CalRow> = rows
        .into_iter()
        .map(|(id, name)| {
            let color = prefs.calendar_color(&id);
            let hidden = prefs.is_calendar_hidden(&id);
            let account = crate::prefs::Prefs::cal_account_from_id(&id).to_string();
            let name = prefs.calendar_display_name(&id, &name);
            CalRow {
                id,
                account,
                name,
                color,
                hidden,
            }
        })
        .collect();
    // Ordre : comptes dans l’ordre des labels settings (alpha label), agendas non masqués d’abord
    out.sort_by(|a, b| {
        let aa = crate::prefs::Prefs::cal_account_from_id(&a.id);
        let bb = crate::prefs::Prefs::cal_account_from_id(&b.id);
        let la = prefs.cal_account_label(aa);
        let lb = prefs.cal_account_label(bb);
        la.cmp(&lb)
            .then_with(|| a.hidden.cmp(&b.hidden))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

fn decorate_event_color(prefs: &crate::prefs::Prefs, mut e: EventRow) -> EventRow {
    e.color = prefs.calendar_color(&e.calendar_id);
    e
}

fn with_maps(mut e: EventRow, provider: &str, home: &str) -> EventRow {
    e.maps_url = build_maps_url(provider, home, &e.location);
    e
}

pub(crate) fn build_maps_url(provider: &str, home: &str, location: &str) -> String {
    let dest = location.trim();
    if dest.is_empty() {
        return String::new();
    }
    let d = urlencoding::encode(dest);
    let o = home.trim();
    match provider {
        "osm" => {
            if o.is_empty() {
                format!("https://www.openstreetmap.org/search?query={d}")
            } else {
                format!(
                    "https://www.openstreetmap.org/directions?engine=fossgis_osrm_car&route={};{}",
                    urlencoding::encode(o),
                    d
                )
            }
        }
        "apple" => {
            if o.is_empty() {
                format!("https://maps.apple.com/?daddr={d}")
            } else {
                format!(
                    "https://maps.apple.com/?saddr={}&daddr={d}",
                    urlencoding::encode(o)
                )
            }
        }
        _ => {
            if o.is_empty() {
                format!("https://www.google.com/maps/dir/?api=1&destination={d}")
            } else {
                format!(
                    "https://www.google.com/maps/dir/?api=1&origin={}&destination={d}",
                    urlencoding::encode(o)
                )
            }
        }
    }
}

fn normalize_rrule_display(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    let upper = t.to_ascii_uppercase().replace("RRULE:", "");
    if upper.contains("FREQ=DAILY") {
        return "daily".into();
    }
    if upper.contains("FREQ=WEEKLY") {
        return "weekly".into();
    }
    if upper.contains("FREQ=MONTHLY") {
        return "monthly".into();
    }
    if upper.contains("FREQ=YEARLY") {
        return "yearly".into();
    }
    "none".into()
}

fn iso_date(raw: &str) -> String {
    if let Some(d) = parse_full_date(raw) {
        return d.format("%Y-%m-%d").to_string();
    }
    let t = raw.trim();
    if t.len() >= 10 && t.as_bytes().get(4) == Some(&b'-') {
        return t[..10].to_string();
    }
    String::new()
}

fn iso_time(raw: &str) -> String {
    let t = raw.trim().trim_matches('"');
    // ISO with T
    if let Some(pos) = t.find('T') {
        let rest = &t[pos + 1..];
        let digits: String = rest
            .chars()
            .filter(|c| c.is_ascii_digit())
            .take(4)
            .collect();
        if digits.len() == 4 {
            return format!("{}:{}", &digits[..2], &digits[2..]);
        }
    }
    // compact YYYYMMDDHHMM
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 12 {
        return format!("{}:{}", &digits[8..10], &digits[10..12]);
    }
    // already HH:MM
    if t.len() >= 5 && t.as_bytes().get(2) == Some(&b':') {
        return t[..5].to_string();
    }
    String::new()
}

fn parse_day(date: &str, year: i32, month: u32) -> Option<u32> {
    let d = parse_full_date(date)?;
    if d.year() == year && d.month() == month {
        Some(d.day())
    } else {
        None
    }
}

fn parse_full_date(date: &str) -> Option<NaiveDate> {
    let d = date.trim().trim_matches('"');
    if d.len() >= 10 && d.as_bytes().get(4) == Some(&b'-') {
        let y: i32 = d.get(0..4)?.parse().ok()?;
        let m: u32 = d.get(5..7)?.parse().ok()?;
        let day: u32 = d.get(8..10)?.parse().ok()?;
        return NaiveDate::from_ymd_opt(y, m, day);
    }
    let digits: String = d.chars().take(8).collect();
    if digits.len() == 8 && digits.chars().all(|c| c.is_ascii_digit()) {
        let y: i32 = digits.get(0..4)?.parse().ok()?;
        let m: u32 = digits.get(4..6)?.parse().ok()?;
        let day: u32 = digits.get(6..8)?.parse().ok()?;
        return NaiveDate::from_ymd_opt(y, m, day);
    }
    None
}

fn format_event_when(raw: &str) -> String {
    let d = raw.trim().trim_matches('"');
    if d.len() >= 15 && d.as_bytes().get(8) == Some(&b'T') {
        let day = &d[6..8];
        let month = &d[4..6];
        let year = &d[0..4];
        let hh = &d[9..11];
        let mm = &d[11..13];
        return format!("{day}/{month}/{year} {hh}:{mm}");
    }
    if d.len() >= 10 && d.as_bytes().get(4) == Some(&b'-') {
        return d.chars().take(16).collect();
    }
    d.to_string()
}

fn format_event_time(raw: &str) -> String {
    let d = raw.trim().trim_matches('"');
    if d.len() >= 15 && d.as_bytes().get(8) == Some(&b'T') {
        return format!("{}:{}", &d[9..11], &d[11..13]);
    }
    if d.len() >= 16 && d.as_bytes().get(10) == Some(&b'T') {
        return d[11..16].to_string();
    }
    String::new()
}

fn build_month_grid(year: i32, month: u32, selected_day: u32, events: &[EventRow]) -> Vec<DayCell> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_else(|| Local::now().date_naive());
    let dim = days_in_month(year, month);
    let weekday = first.weekday().num_days_from_monday();

    let mut by_day: std::collections::BTreeMap<u32, Vec<&EventRow>> = std::collections::BTreeMap::new();
    for e in events {
        if let Some(d) = parse_day(&e.date, year, month) {
            by_day.entry(d).or_default().push(e);
        }
    }

    let mut cells = Vec::new();
    for _ in 0..weekday {
        cells.push(DayCell {
            day: 0,
            in_month: false,
            has_events: false,
            selected: false,
            previews: vec![],
            more: 0,
        });
    }
    for d in 1..=dim {
        let list = by_day.get(&d).map(|v| v.as_slice()).unwrap_or(&[]);
        let mut previews = Vec::new();
        for e in list.iter().take(3) {
            previews.push(EventPreview {
                time: e.time.clone(),
                title: e.summary.clone(),
                ev_json: e.ev_json.clone(),
                color: e.color.clone(),
            });
        }
        let more = list.len().saturating_sub(3) as u32;
        cells.push(DayCell {
            day: d,
            in_month: true,
            has_events: !list.is_empty(),
            selected: d == selected_day,
            previews,
            more,
        });
    }
    while cells.len() % 7 != 0 {
        cells.push(DayCell {
            day: 0,
            in_month: false,
            has_events: false,
            selected: false,
            previews: vec![],
            more: 0,
        });
    }
    cells
}

fn build_week_cols(selected: NaiveDate, events: &[EventRow], locale: &str) -> Vec<WeekDayCol> {
    let monday = selected - Duration::days(selected.weekday().num_days_from_monday() as i64);
    (0..7)
        .map(|i| {
            let d = monday + Duration::days(i);
            let day_events: Vec<EventRow> = events
                .iter()
                .filter(|e| parse_full_date(&e.date) == Some(d))
                .cloned()
                .collect();
            WeekDayCol {
                label: format!("{} {}", crate::i18n::dow_short(locale, i as usize), d.day()),
                day: d.day(),
                month: d.month(),
                year: d.year(),
                selected: d == selected,
                events: day_events,
            }
        })
        .collect()
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .unwrap()
        .pred_opt()
        .unwrap()
        .day()
}

#[derive(Deserialize)]
pub struct CreateEventForm {
    pub calendar: String,
    pub summary: String,
    pub start_date: String,
    pub start_time: String,
    pub end_date: Option<String>,
    pub end_time: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub rrule: Option<String>,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub view: Option<String>,
    /// Si `1` : rester sur la page courante (panneau mail) au lieu d’aller au calendrier
    pub stay: Option<String>,
}

async fn create_event(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateEventForm>,
) -> impl IntoResponse {
    let stay =
        form.stay.as_deref() == Some("1") || form.stay.as_deref() == Some("true");
    let cal = form.calendar.trim();
    if cal.is_empty() || cal == "__all__" {
        if stay {
            return Html(
                r#"<script>alert('Choisissez un calendrier');</script>"#.to_string(),
            )
            .into_response();
        }
        return Redirect::to("/calendar?msg=Choisissez%20un%20calendrier").into_response();
    }
    let Some(client) = &state.calendula else {
        if stay {
            return Html(r#"<script>alert('Calendula absent');</script>"#.to_string())
                .into_response();
        }
        return Redirect::to("/calendar?msg=Calendula%20absent").into_response();
    };
    let start = combine_dt(&form.start_date, &form.start_time);
    let end = combine_dt(
        form.end_date.as_deref().unwrap_or(&form.start_date),
        form.end_time.as_deref().unwrap_or(""),
    );
    let fields = crate::cli::tcal::EventFields {
        summary: form.summary.trim().to_string(),
        start,
        end,
        description: form.description.as_deref().unwrap_or("").to_string(),
        location: form.location.as_deref().unwrap_or("").to_string(),
        rrule: form.rrule.as_deref().unwrap_or("none").to_string(),
    };
    let ical = match crate::cli::tcal::build_event(&fields) {
        Ok(v) => v,
        Err(e) => {
            if stay {
                let msg = serde_json::to_string(&format!("Erreur tcal: {e}"))
                    .unwrap_or_else(|_| "\"Erreur\"".into());
                return Html(format!(r#"<script>alert({msg});</script>"#)).into_response();
            }
            let err_s = format!("Erreur tcal: {e}");
            let msg = urlencoding::encode(&err_s);
            return Redirect::to(&format!("/calendar?msg={msg}")).into_response();
        }
    };
    let _permit = state.cli_limit.acquire().await.ok();
    let redirect = cal_redirect(
        cal,
        form.view.as_deref(),
        form.year,
        form.month,
        form.day,
    );
    match client.create_event(cal, ical.as_bytes()).await {
        Ok(_) => {
            let now = chrono::Local::now();
            let y = form.year.unwrap_or_else(|| now.year());
            let m = form.month.unwrap_or_else(|| now.month());
            let _ = refresh_calendar_month(&state, y, m, false).await;
            if stay {
                return Html(
                    r##"<script>
if (window.HimaWeb) {
  window.HimaWeb.onQuickEventCreated();
}
</script>"##
                        .to_string(),
                )
                .into_response();
            }
            Redirect::to(&format!("{redirect}&refresh=1&msg=Événement%20créé")).into_response()
        }
        Err(e) => {
            if stay {
                let msg = serde_json::to_string(&format!("Erreur: {e}"))
                    .unwrap_or_else(|_| "\"Erreur\"".into());
                return Html(format!(r#"<script>alert({msg});</script>"#)).into_response();
            }
            let err_s = format!("Erreur: {e}");
            let msg = urlencoding::encode(&err_s);
            Redirect::to(&format!("{redirect}&msg={msg}")).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateEventForm {
    pub id: String,
    pub calendar: String,
    pub summary: String,
    pub start_date: String,
    pub start_time: String,
    pub end_date: Option<String>,
    pub end_time: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub rrule: Option<String>,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub view: Option<String>,
}

async fn update_event(
    State(state): State<Arc<AppState>>,
    Form(form): Form<UpdateEventForm>,
) -> impl IntoResponse {
    let cal = form.calendar.trim();
    let id = form.id.trim();
    if cal.is_empty() || id.is_empty() {
        return Redirect::to("/calendar?msg=Événement%20incomplet").into_response();
    }
    let Some(client) = &state.calendula else {
        return Redirect::to("/calendar?msg=Calendula%20absent").into_response();
    };
    let start = combine_dt(&form.start_date, &form.start_time);
    let end = combine_dt(
        form.end_date.as_deref().unwrap_or(&form.start_date),
        form.end_time.as_deref().unwrap_or(""),
    );
    let fields = crate::cli::tcal::EventFields {
        summary: form.summary.trim().to_string(),
        start,
        end,
        description: form.description.as_deref().unwrap_or("").to_string(),
        location: form.location.as_deref().unwrap_or("").to_string(),
        rrule: form.rrule.as_deref().unwrap_or("none").to_string(),
    };
    let _permit = state.cli_limit.acquire().await.ok();
    // Fold-back sur l’iCal existant : conserve UID / alarmes / props non modélisées
    let ical = match client.read_event_ical(cal, id).await {
        Ok(existing) if !existing.trim().is_empty() => {
            crate::cli::tcal::apply_event_fields(&existing, &fields)
        }
        _ => {
            let uid = id.trim().trim_end_matches(".ics");
            crate::cli::tcal::build_event_with_uid(uid, &fields)
        }
    };
    let ical = match ical {
        Ok(v) => v,
        Err(e) => {
            let err_s = format!("Erreur tcal: {e}");
            let msg = urlencoding::encode(&err_s);
            return Redirect::to(&format!("/calendar?msg={msg}")).into_response();
        }
    };
    let redirect = cal_redirect(
        cal,
        form.view.as_deref(),
        form.year,
        form.month,
        form.day,
    );
    match client
        .update_event(cal, id, ical.as_bytes(), None)
        .await
    {
        Ok(()) => {
            if let (Some(y), Some(m)) = (form.year, form.month) {
                let _ = refresh_calendar_month(&state, y, m, false).await;
            }
            Redirect::to(&format!("{redirect}&refresh=1&msg=Événement%20modifié")).into_response()
        }
        Err(e) => {
            let err_s = format!("Erreur: {e}");
            let msg = urlencoding::encode(&err_s);
            Redirect::to(&format!("{redirect}&msg={msg}")).into_response()
        }
    }
}

fn combine_dt(date: &str, time: &str) -> String {
    let d = date.trim();
    let t = time.trim();
    if d.is_empty() {
        return String::new();
    }
    if t.is_empty() {
        return d.to_string();
    }
    format!("{d} {t}")
}

#[derive(Deserialize)]
pub struct DeleteEventForm {
    pub calendar: String,
    pub id: String,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub view: Option<String>,
}

async fn delete_event(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteEventForm>,
) -> impl IntoResponse {
    let Some(client) = &state.calendula else {
        return Redirect::to("/calendar?msg=Calendula%20absent").into_response();
    };
    if form.id.trim().is_empty() || form.calendar.trim().is_empty() {
        return Redirect::to("/calendar?msg=Événement%20incomplet").into_response();
    }
    let _permit = state.cli_limit.acquire().await.ok();
    let redirect = cal_redirect(
        form.calendar.trim(),
        form.view.as_deref(),
        form.year,
        form.month,
        form.day,
    );
    match client
        .delete_event(form.calendar.trim(), form.id.trim())
        .await
    {
        Ok(()) => {
            if let (Some(y), Some(m)) = (form.year, form.month) {
                let _ = refresh_calendar_month(&state, y, m, false).await;
            }
            Redirect::to(&format!("{redirect}&refresh=1&msg=Événement%20supprimé")).into_response()
        }
        Err(e) => {
            let err_s = format!("Erreur: {e}");
            let msg = urlencoding::encode(&err_s);
            Redirect::to(&format!("{redirect}&msg={msg}")).into_response()
        }
    }
}

fn cal_redirect(
    calendar: &str,
    view: Option<&str>,
    year: Option<i32>,
    month: Option<u32>,
    day: Option<u32>,
) -> String {
    let mut url = format!(
        "/calendar?calendar={}",
        urlencoding::encode(calendar)
    );
    if let Some(v) = view {
        url.push_str(&format!("&view={v}"));
    }
    if let Some(y) = year {
        url.push_str(&format!("&year={y}"));
    }
    if let Some(m) = month {
        url.push_str(&format!("&month={m}"));
    }
    if let Some(d) = day {
        url.push_str(&format!("&day={d}"));
    }
    url
}

pub(crate) async fn load_todos(
    state: &AppState,
    calendars: &[CalRow],
    current_id: &str,
) -> Vec<TodoRow> {
    let Some(client) = &state.calendula else {
        return vec![];
    };
    let prefs = state.prefs.lock().await.clone();
    let targets: Vec<&CalRow> = if current_id == "__all__" || current_id.is_empty() {
        calendars.iter().collect()
    } else if let Some(acc) = parse_account_filter(current_id) {
        calendars
            .iter()
            .filter(|c| crate::prefs::Prefs::cal_account_from_id(&c.id) == acc)
            .collect()
    } else {
        calendars.iter().filter(|c| c.id == current_id).collect()
    };
    let mut join = tokio::task::JoinSet::new();
    for c in targets {
        let client = client.clone();
        let cal = c.id.clone();
        let pool = state.cli_limit.clone();
        join.spawn(async move {
            let _p = pool.acquire().await.ok();
            client.list_todos(&cal, None, None).await.ok()
        });
    }
    let mut out = Vec::new();
    while let Some(res) = join.join_next().await {
        if let Ok(Some(list)) = res {
            for t in list {
                let completed = t.percent_complete >= 100
                    || t.status.eq_ignore_ascii_case("COMPLETED");
                let account = crate::prefs::Prefs::cal_account_from_id(&t.calendar_id).to_string();
                let calendar_name = prefs.cal_account_label(&account);
                let color = prefs.calendar_color(&t.calendar_id);
                out.push(TodoRow {
                    id: t.id,
                    calendar_id: t.calendar_id.clone(),
                    account,
                    calendar_name,
                    color,
                    summary: t.summary,
                    due: t.due.clone(),
                    due_label: format_todo_due(&t.due),
                    completed,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.completed
            .cmp(&b.completed)
            .then_with(|| a.due.cmp(&b.due))
            .then_with(|| a.summary.cmp(&b.summary))
    });
    out
}

fn format_todo_due(due: &str) -> String {
    let t = due.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.len() >= 8 && t.as_bytes().get(4) != Some(&b'-') {
        let d = &t[..8.min(t.len())];
        if d.chars().all(|c| c.is_ascii_digit()) && d.len() == 8 {
            return format!("{}/{}/{}", &d[6..8], &d[4..6], &d[0..4]);
        }
    }
    if t.len() >= 10 && t.as_bytes().get(4) == Some(&b'-') {
        let parts: Vec<_> = t[..10].split('-').collect();
        if parts.len() == 3 {
            return format!("{}/{}/{}", parts[2], parts[1], parts[0]);
        }
    }
    t.to_string()
}

fn todo_uid_from_id(id: &str) -> String {
    id.trim()
        .trim_end_matches(".ics")
        .trim()
        .to_string()
}

#[derive(Deserialize)]
pub struct CreateTodoForm {
    pub calendar: String,
    pub summary: String,
    pub due: Option<String>,
    pub stay: Option<String>,
}

async fn create_todo(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateTodoForm>,
) -> impl IntoResponse {
    let stay =
        form.stay.as_deref() == Some("1") || form.stay.as_deref() == Some("true");
    let cal = form.calendar.trim();
    if cal.is_empty() || cal == "__all__" {
        if stay {
            return Html(
                r#"<script>alert('Choisissez un calendrier');</script>"#.to_string(),
            )
            .into_response();
        }
        return Redirect::to("/calendar?msg=Choisissez%20un%20calendrier").into_response();
    }
    let Some(client) = &state.calendula else {
        return Redirect::to("/calendar?msg=Calendula%20absent").into_response();
    };
    let fields = crate::cli::tcal::TodoFields {
        summary: form.summary.trim().to_string(),
        due: form.due.as_deref().unwrap_or("").to_string(),
        completed: false,
        description: String::new(),
    };
    let ical = match crate::cli::tcal::build_todo(&fields) {
        Ok(v) => v,
        Err(e) => {
            let err_s = format!("Erreur tcal: {e}");
            let msg = urlencoding::encode(&err_s);
            return Redirect::to(&format!("/calendar?msg={msg}")).into_response();
        }
    };
    let _permit = state.cli_limit.acquire().await.ok();
    match client.create_todo(cal, ical.as_bytes()).await {
        Ok(_) => {
            if stay {
                return Html(
                    r##"<script>if(window.HimaWeb){window.HimaWeb.onQuickEventCreated();}</script>"##
                        .to_string(),
                )
                .into_response();
            }
            Redirect::to(&format!(
                "/calendar?calendar={}&msg=Tâche%20créée",
                urlencoding::encode(cal)
            ))
            .into_response()
        }
        Err(e) => {
            let err_s = e.to_string();
            let msg = urlencoding::encode(&err_s);
            Redirect::to(&format!("/calendar?msg={msg}")).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct ToggleTodoForm {
    pub calendar: String,
    pub id: String,
    pub summary: String,
    pub due: Option<String>,
    pub completed: Option<String>,
    pub stay: Option<String>,
}

async fn toggle_todo(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ToggleTodoForm>,
) -> impl IntoResponse {
    let stay =
        form.stay.as_deref() == Some("1") || form.stay.as_deref() == Some("true");
    let cal = form.calendar.trim();
    let Some(client) = &state.calendula else {
        return Redirect::to("/calendar?msg=Calendula%20absent").into_response();
    };
    let currently_done = form.completed.as_deref() == Some("1");
    let uid = todo_uid_from_id(&form.id);
    let fields = crate::cli::tcal::TodoFields {
        summary: form.summary.trim().to_string(),
        due: form.due.as_deref().unwrap_or("").to_string(),
        completed: !currently_done,
        description: String::new(),
    };
    // Réutilise l’iCal distant si lisible (même verbe event read / contents)
    let ical = match client.read_event_ical(cal, &form.id).await {
        Ok(existing) if !existing.trim().is_empty() => {
            crate::cli::tcal::apply_todo_fields(&existing, &fields)
        }
        _ => crate::cli::tcal::build_todo_with_uid(&uid, &fields),
    };
    let ical = match ical {
        Ok(v) => v,
        Err(e) => {
            let err_s = format!("Erreur tcal: {e}");
            let msg = urlencoding::encode(&err_s);
            return Redirect::to(&format!("/calendar?msg={msg}")).into_response();
        }
    };
    let _permit = state.cli_limit.acquire().await.ok();
    match client.update_todo(cal, &form.id, ical.as_bytes()).await {
        Ok(()) => {
            if stay {
                return Html(
                    r##"<script>if(window.HimaWeb){window.HimaWeb.onQuickEventCreated();}</script>"##
                        .to_string(),
                )
                .into_response();
            }
            Redirect::to(&format!(
                "/calendar?calendar={}&msg=Tâche%20mise%20à%20jour",
                urlencoding::encode(cal)
            ))
            .into_response()
        }
        Err(e) => {
            let err_s = e.to_string();
            let msg = urlencoding::encode(&err_s);
            Redirect::to(&format!("/calendar?msg={msg}")).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct DeleteTodoForm {
    pub calendar: String,
    pub id: String,
    pub stay: Option<String>,
}

async fn delete_todo(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteTodoForm>,
) -> impl IntoResponse {
    let stay =
        form.stay.as_deref() == Some("1") || form.stay.as_deref() == Some("true");
    let cal = form.calendar.trim();
    let Some(client) = &state.calendula else {
        return Redirect::to("/calendar?msg=Calendula%20absent").into_response();
    };
    let _permit = state.cli_limit.acquire().await.ok();
    match client.delete_todo(cal, &form.id).await {
        Ok(()) => {
            if stay {
                return Html(
                    r##"<script>if(window.HimaWeb){window.HimaWeb.onQuickEventCreated();}</script>"##
                        .to_string(),
                )
                .into_response();
            }
            Redirect::to(&format!(
                "/calendar?calendar={}&msg=Tâche%20supprimée",
                urlencoding::encode(cal)
            ))
            .into_response()
        }
        Err(e) => {
            let err_s = e.to_string();
            let msg = urlencoding::encode(&err_s);
            Redirect::to(&format!("/calendar?msg={msg}")).into_response()
        }
    }
}
