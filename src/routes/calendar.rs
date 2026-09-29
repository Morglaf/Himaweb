use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};
use serde::Deserialize;

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/calendar", get(calendar_page))
        .route("/calendar/{id}/events", get(events_fragment))
        .route("/calendar/create", post(create_event))
        .route("/calendar/update", post(update_event))
        .route("/calendar/delete", post(delete_event))
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
    pub account_hints: Vec<CalAccountHint>,
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
    pub flash: Option<String>,
    pub ai_enabled: bool,
    pub default_date: String,
    pub default_calendar: String,
}

pub struct CalRow {
    pub id: String,
    pub name: String,
}

pub struct CalAccountHint {
    pub name: String,
    pub label: String,
    pub icon: String,
    pub color: String,
}

pub struct EventPreview {
    pub time: String,
    pub title: String,
    pub ev_json: String,
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
    /// JSON compact pour data-ev (échappé HTML-safe)
    pub ev_json: String,
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

    let prefs = state.prefs.lock().await.clone();
    let current_id_enc = urlencoding::encode(&current_id).into_owned();
    let default_date = format!("{year:04}-{month:02}-{day:02}");
    let default_calendar = if current_id != "__all__" && !current_id.is_empty() {
        current_id.clone()
    } else {
        calendars
            .first()
            .map(|c| c.id.clone())
            .unwrap_or_default()
    };
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
        flash: q.msg,
        ai_enabled: prefs.ai_enabled,
        default_date,
        default_calendar,
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
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub view: String,
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
    let view = q.view.unwrap_or_else(|| "day".into());

    let (_cals, _cur, events, error, _hints, _) =
        load_calendar_data(&state, Some(&id), year, month, false).await;
    let day_events: Vec<EventRow> = events
        .into_iter()
        .filter(|e| parse_day(&e.date, year, month) == Some(day))
        .collect();

    let tpl = EventsFragment {
        events: day_events,
        error,
        year,
        month,
        day,
        view,
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
    Vec<CalAccountHint>,
    bool,
) {
    let prefs = state.prefs.lock().await.clone();
    let account_hints: Vec<CalAccountHint> = crate::calendar_import::list_calendula_accounts()
        .unwrap_or_default()
        .into_iter()
        .map(|a| CalAccountHint {
            label: prefs.cal_account_label(&a.name),
            icon: prefs.cal_account_icon(&a.name),
            color: prefs.cal_account_color(&a.name),
            name: a.name,
        })
        .collect();

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
                    .map(|(cid, id, summary, start, end, desc)| {
                        with_ev_json(to_event_row(id, cid, summary, start, end, desc))
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
                        .map(|(c, i, s, start, e, d)| {
                            with_ev_json(to_event_row(i, c, s, start, e, d))
                        })
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
                    with_ev_json(to_event_row(
                        e.id,
                        c.id.clone(),
                        e.summary,
                        e.date,
                        e.end,
                        e.description,
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

fn to_event_row(
    id: String,
    calendar_id: String,
    summary: String,
    date: String,
    end: String,
    description: String,
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
        location: String::new(),
        rrule: String::new(),
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
    })
    .to_string();
    e
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
    let ical = crate::cli::calendula::CalendulaClient::build_ical(
        form.summary.trim(),
        &start,
        &end,
        form.description.as_deref().unwrap_or(""),
        form.location.as_deref().unwrap_or(""),
        form.rrule.as_deref().unwrap_or("none"),
    );
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
            let _ = refresh_calendar_month(&state, y, m).await;
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
    let ical = crate::cli::calendula::CalendulaClient::build_ical(
        form.summary.trim(),
        &start,
        &end,
        form.description.as_deref().unwrap_or(""),
        form.location.as_deref().unwrap_or(""),
        form.rrule.as_deref().unwrap_or("none"),
    );
    let _permit = state.cli_limit.acquire().await.ok();
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
                let _ = refresh_calendar_month(&state, y, m).await;
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
                let _ = refresh_calendar_month(&state, y, m).await;
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
