use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};
use serde::Deserialize;

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/calendar", get(calendar_page))
        .route("/calendar/{id}/events", get(events_fragment))
}

#[derive(Deserialize)]
pub struct CalQuery {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub calendar: Option<String>,
    pub view: Option<String>,
    pub refresh: Option<String>,
}

#[derive(Template)]
#[template(path = "shell.html")]
struct ShellTemplate {
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

#[derive(Template)]
#[template(path = "calendar.html")]
struct CalendarTemplate {
    pub calendula_available: bool,
    pub calendars: Vec<CalRow>,
    pub account_hints: Vec<String>,
    pub current_id: String,
    pub current_id_enc: String,
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub view: String,
    pub title_label: String,
    pub month_name: String,
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
    pub from_cache: bool,
}

pub struct CalRow {
    pub id: String,
    pub name: String,
}

pub struct EventPreview {
    pub time: String,
    pub title: String,
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
    pub summary: String,
    pub date: String,
    pub when: String,
    pub time: String,
    pub description: String,
}

const MONTHS_FR: &[&str] = &[
    "",
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

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
            let _ = refresh_calendar_month(&bg, y, m).await;
        });
    }

    let selected = NaiveDate::from_ymd_opt(year, month, day).unwrap_or(now);
    let (prev, next, title_label) = nav_for_view(&view, selected);
    let month_name = MONTHS_FR
        .get(month as usize)
        .copied()
        .unwrap_or("")
        .to_string();

    let days = if view == "month" {
        build_month_grid(year, month, day, &events)
    } else {
        vec![]
    };
    let week_days = if view == "week" {
        build_week_cols(selected, &events)
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

    let current_id_enc = urlencoding::encode(&current_id).into_owned();
    let inner = CalendarTemplate {
        calendula_available: state.calendula_available,
        calendars,
        account_hints,
        current_id: current_id.clone(),
        current_id_enc,
        year,
        month,
        day,
        view: view.clone(),
        title_label,
        month_name,
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
        from_cache,
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
        calendula_available: state.calendula_available,
        cardamum_available: state.cardamum_available,
        theme,
        layout,
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

fn nav_for_view(view: &str, selected: NaiveDate) -> (NaiveDate, NaiveDate, String) {
    match view {
        "day" => {
            let label = format!(
                "{} {} {}",
                weekday_fr(selected.weekday()),
                selected.day(),
                MONTHS_FR[selected.month() as usize]
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
                "Semaine du {} {} — {} {}",
                monday.day(),
                MONTHS_FR[monday.month() as usize],
                sunday.day(),
                MONTHS_FR[sunday.month() as usize]
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
                MONTHS_FR[selected.month() as usize],
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

fn weekday_fr(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "lundi",
        Weekday::Tue => "mardi",
        Weekday::Wed => "mercredi",
        Weekday::Thu => "jeudi",
        Weekday::Fri => "vendredi",
        Weekday::Sat => "samedi",
        Weekday::Sun => "dimanche",
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
    Vec<String>,
    bool,
) {
    let account_hints = crate::calendar_import::list_calendula_accounts()
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.name)
        .collect::<Vec<_>>();

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
            let calendars: Vec<CalRow> = cached_cals
                .iter()
                .map(|(id, name, _)| CalRow {
                    id: id.clone(),
                    name: name.clone(),
                })
                .collect();
            let current_id = preferred
                .map(str::to_string)
                .filter(|p| p == "__all__" || calendars.iter().any(|c| &c.id == p))
                .unwrap_or_else(|| "__all__".into());
            let ids: Vec<String> = if current_id == "__all__" {
                calendars.iter().map(|c| c.id.clone()).collect()
            } else {
                vec![current_id.clone()]
            };
            let cached_ev = {
                let cache = state.cache.lock().await;
                cache
                    .load_events_in_month(&ids, year, month)
                    .unwrap_or_default()
            };
            if !cached_ev.is_empty() {
                let events: Vec<EventRow> = cached_ev
                    .into_iter()
                    .map(|(_cid, _id, summary, start, _end, desc)| {
                        to_event_row(summary, start, desc)
                    })
                    .collect();
                return (
                    calendars,
                    current_id,
                    events,
                    None,
                    account_hints,
                    true,
                );
            }
            // Calendriers en cache mais pas d'events pour ce mois → fetch CLI
        }
    }

    match fetch_and_cache_month(state, preferred, year, month).await {
        Ok((calendars, current_id, events)) => {
            (calendars, current_id, events, None, account_hints, false)
        }
        Err(e) => {
            // Fallback cache même si refresh a échoué
            let cached_cals = {
                let cache = state.cache.lock().await;
                cache.load_calendars().unwrap_or_default()
            };
            if !cached_cals.is_empty() {
                let calendars: Vec<CalRow> = cached_cals
                    .iter()
                    .map(|(id, name, _)| CalRow {
                        id: id.clone(),
                        name: name.clone(),
                    })
                    .collect();
                let current_id = preferred
                    .map(str::to_string)
                    .filter(|p| p == "__all__" || calendars.iter().any(|c| &c.id == p))
                    .unwrap_or_else(|| "__all__".into());
                let ids: Vec<String> = if current_id == "__all__" {
                    calendars.iter().map(|c| c.id.clone()).collect()
                } else {
                    vec![current_id.clone()]
                };
                let events: Vec<EventRow> = {
                    let cache = state.cache.lock().await;
                    cache
                        .load_events_in_month(&ids, year, month)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(_c, _i, s, start, _e, d)| to_event_row(s, start, d))
                        .collect()
                };
                return (
                    calendars,
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
) -> Result<(Vec<CalRow>, String, Vec<EventRow>), String> {
    let Some(client) = &state.calendula else {
        return Err("Calendula indisponible.".into());
    };
    let from = format!("{year:04}-{month:02}-01");
    let to = format!("{year:04}-{month:02}-{:02}", days_in_month(year, month));
    let _permit = state.cli_limit.acquire().await.map_err(|e| e.to_string())?;

    let list = client.list_calendars().await.map_err(|e| {
        format!("{e} — vérifiez l’URL CalDAV (home complète) et le mot de passe dans Paramètres.")
    })?;
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

    let calendars: Vec<CalRow> = list
        .iter()
        .map(|c| CalRow {
            id: c.id.clone(),
            name: c.name.clone(),
        })
        .collect();

    let current_id = preferred
        .map(str::to_string)
        .filter(|p| p == "__all__" || calendars.iter().any(|c| &c.id == p))
        .unwrap_or_else(|| "__all__".into());

    let targets: Vec<&CalRow> = if current_id == "__all__" {
        calendars.iter().collect()
    } else {
        calendars.iter().filter(|c| c.id == current_id).collect()
    };

    let mut all_events = Vec::new();
    let mut errs = Vec::new();
    for c in targets {
        match client.list_events(&c.id, Some(&from), Some(&to)).await {
            Ok(list) => {
                let rows: Vec<(String, String, String, String, String)> = list
                    .iter()
                    .map(|e| {
                        (
                            e.id.clone(),
                            e.summary.clone(),
                            e.date.clone(),
                            e.end.clone(),
                            e.description.clone(),
                        )
                    })
                    .collect();
                {
                    let cache = state.cache.lock().await;
                    let _ = cache.replace_calendar_events(&c.id, &rows);
                }
                all_events.extend(list.into_iter().map(|e| {
                    to_event_row(e.summary, e.date, e.description)
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

/// Warm / refresh mois courant (+ voisin) pour le cache.
pub async fn refresh_calendar_into_cache(state: &AppState) -> Result<usize, String> {
    let now = Local::now().date_naive();
    refresh_calendar_month(state, now.year(), now.month()).await
}

pub async fn refresh_calendar_month(
    state: &AppState,
    year: i32,
    month: u32,
) -> Result<usize, String> {
    let (cals, _, events) = fetch_and_cache_month(state, Some("__all__"), year, month).await?;
    Ok(cals.len().saturating_add(events.len()))
}

fn to_event_row(summary: String, date: String, description: String) -> EventRow {
    let when = format_event_when(&date);
    let time = format_event_time(&date);
    EventRow {
        summary,
        date,
        when,
        time,
        description,
    }
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

fn build_week_cols(selected: NaiveDate, events: &[EventRow]) -> Vec<WeekDayCol> {
    let monday = selected - Duration::days(selected.weekday().num_days_from_monday() as i64);
    let labels = ["Lun", "Mar", "Mer", "Jeu", "Ven", "Sam", "Dim"];
    (0..7)
        .map(|i| {
            let d = monday + Duration::days(i);
            let day_events: Vec<EventRow> = events
                .iter()
                .filter(|e| parse_full_date(&e.date) == Some(d))
                .cloned()
                .collect();
            WeekDayCol {
                label: format!("{} {}", labels[i as usize], d.day()),
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
