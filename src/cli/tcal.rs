//! Pont iCalendar ↔ formulaire web via **tcal** (Pimalaya).
//!
//! Pas de second moteur iCal : projection TOML + fold-back, comme la CLI
//! `tcal template` / `tcal apply`.

use chrono::{Datelike, TimeZone};
use serde::Deserialize;
use tcal::template::TcalTemplate;
use toml_edit::{Array, DocumentMut, Item, value};

#[derive(Debug, thiserror::Error)]
pub enum TcalBridgeError {
    #[error("tcal: {0}")]
    Tcal(String),
    #[error("toml: {0}")]
    Toml(String),
}

type Result<T> = std::result::Result<T, TcalBridgeError>;

fn map_tcal<E: std::fmt::Display>(e: E) -> TcalBridgeError {
    TcalBridgeError::Tcal(e.to_string())
}

/// Projection UI d’une récurrence (select + panneau custom).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecurrenceUi {
    /// `none` | `daily` | `weekly` | `monthly` | `yearly` | `custom`
    pub mode: String,
    /// Fréquence réelle si `mode == "custom"`.
    pub frequency: String,
    pub interval: Option<u32>,
    pub count: Option<u32>,
    /// `YYYY-MM-DD`
    pub until: String,
    /// Ex. `tu`, `2tu`, `4tu`.
    pub byday: Vec<String>,
    /// Occurrences exclues (`YYYY-MM-DD HH:MM`) lues depuis EXDATE.
    pub exdates: Vec<String>,
}

impl RecurrenceUi {
    pub fn none() -> Self {
        Self {
            mode: "none".into(),
            ..Default::default()
        }
    }

    pub fn is_active(&self) -> bool {
        let m = self.mode.trim().to_ascii_lowercase();
        !m.is_empty() && m != "none"
    }

    pub fn from_event_fields(f: &EventFields) -> Self {
        Self {
            mode: f.rrule.clone(),
            frequency: f.rrule_frequency.clone(),
            interval: f.rrule_interval,
            count: f.rrule_count,
            until: f.rrule_until.clone(),
            byday: f.rrule_byday.clone(),
            exdates: f.exdates.clone(),
        }
    }

    pub fn apply_to(&self, fields: &mut EventFields) {
        fields.rrule = self.mode.clone();
        fields.rrule_frequency = self.frequency.clone();
        fields.rrule_interval = self.interval;
        fields.rrule_count = self.count;
        fields.rrule_until = self.until.clone();
        fields.rrule_byday = self.byday.clone();
    }

    /// Ligne `RRULE:…` (sans préfixe) pour expansion d’affichage.
    pub fn to_rrule_value(&self) -> Option<String> {
        let freq = if self.mode.eq_ignore_ascii_case("custom") {
            self.frequency.trim().to_ascii_lowercase()
        } else {
            self.mode.trim().to_ascii_lowercase()
        };
        if freq.is_empty() || freq == "none" {
            return None;
        }
        let mut parts = vec![format!("FREQ={}", freq.to_ascii_uppercase())];
        if let Some(n) = self.interval.filter(|n| *n > 1) {
            parts.push(format!("INTERVAL={n}"));
        }
        if let Some(n) = self.count.filter(|n| *n > 0) {
            parts.push(format!("COUNT={n}"));
        }
        let until = self.until.trim();
        if !until.is_empty() {
            // Fin de journée UTC-ish compact (suffisant pour bornage d’affichage).
            let digits: String = until.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 8 {
                parts.push(format!("UNTIL={}T235959Z", &digits[..8]));
            }
        }
        if !self.byday.is_empty() {
            let days: Vec<String> = self
                .byday
                .iter()
                .map(|d| d.trim().to_ascii_uppercase())
                .filter(|d| !d.is_empty())
                .collect();
            if !days.is_empty() {
                parts.push(format!("BYDAY={}", days.join(",")));
            }
        }
        Some(parts.join(";"))
    }
}

/// Occurrences dans `[from, to]` (dates inclusives), pour la grille UI,
/// avec n° d’occurrence 1-based (EXDATE comptés pour garder des trous stables).
///
/// Utilise la crate `rrule` en lecture seule — l’écriture ICS reste tcal.
pub fn expand_occurrence_starts_numbered(
    start_ui: &str,
    recurrence: &RecurrenceUi,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Vec<(chrono::NaiveDateTime, u32)> {
    let Some(rule) = recurrence.to_rrule_value() else {
        return Vec::new();
    };
    let Some(start_dt) = parse_ui_naive_dt(start_ui) else {
        return Vec::new();
    };
    let dtstart = start_dt.format("%Y%m%dT%H%M%SZ").to_string();
    let blob = format!("DTSTART:{dtstart}\nRRULE:{rule}");
    let Ok(set) = blob.parse::<rrule::RRuleSet>() else {
        return Vec::new();
    };
    // Toute la série depuis DTSTART (pour numérotation stable après EXDATE).
    let series_after = rrule::Tz::UTC
        .with_ymd_and_hms(
            start_dt.year(),
            start_dt.month(),
            start_dt.day(),
            0,
            0,
            0,
        )
        .single();
    let series_before = {
        let until_d = recurrence
            .until
            .trim()
            .get(..10)
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .unwrap_or_else(|| to.max(start_dt.date()) + chrono::Duration::days(365 * 2));
        let end = until_d.max(to);
        rrule::Tz::UTC
            .with_ymd_and_hms(end.year(), end.month(), end.day(), 23, 59, 59)
            .single()
    };
    let (Some(series_after), Some(series_before)) = (series_after, series_before) else {
        return Vec::new();
    };
    let series_after = series_after - chrono::Duration::seconds(1);
    let set = set.after(series_after).before(series_before);
    let result = set.all(400);
    let exset: std::collections::HashSet<String> = recurrence
        .exdates
        .iter()
        .filter_map(|s| parse_ui_naive_dt(s))
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .collect();
    let mut numbered = Vec::new();
    let mut n: u32 = 0;
    for dt in result.dates.into_iter().map(|d| d.naive_utc()) {
        // Numéroter toute la série RRULE (y compris EXDATE) pour garder
        // des trous stables : si on détache #3, le suivant reste #4.
        n = n.saturating_add(1);
        let key = dt.format("%Y-%m-%d %H:%M").to_string();
        if exset.contains(&key) {
            continue;
        }
        let d = dt.date();
        if d >= from && d <= to {
            numbered.push((dt, n));
        }
    }
    numbered
}

/// Index 1-based d’une occurrence dans la série RRULE (EXDATE comptés pour la position).
pub fn occurrence_index(
    start_ui: &str,
    recurrence: &RecurrenceUi,
    occurrence_ui: &str,
) -> Option<u32> {
    let Some(target) = parse_ui_naive_dt(occurrence_ui) else {
        return None;
    };
    let target_key = target.format("%Y-%m-%d %H:%M").to_string();
    let from = parse_ui_naive_dt(start_ui)?.date();
    let to = recurrence
        .until
        .trim()
        .get(..10)
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .unwrap_or(from + chrono::Duration::days(365 * 3));
    expand_occurrence_starts_numbered(start_ui, recurrence, from, to)
        .into_iter()
        .find(|(dt, _)| dt.format("%Y-%m-%d %H:%M").to_string() == target_key)
        .map(|(_, n)| n)
        .or_else(|| {
            // Cible peut être EXDATÉe : recalculer en ignorant le filtre d’affichage.
            let rec_no_ex = RecurrenceUi {
                exdates: vec![],
                ..recurrence.clone()
            };
            expand_occurrence_starts_numbered(start_ui, &rec_no_ex, from, to)
                .into_iter()
                .find(|(dt, _)| dt.format("%Y-%m-%d %H:%M").to_string() == target_key)
                .map(|(_, n)| n)
        })
}

/// Préfixe `#n · ` pour une occurrence détachée / affichage.
pub fn with_occ_prefix(summary: &str, occ_n: u32) -> String {
    let base = strip_occ_prefix(summary);
    if occ_n == 0 {
        base
    } else {
        format!("#{occ_n} · {base}")
    }
}

/// Retire un préfixe `#n · ` éventuel.
pub fn strip_occ_prefix(summary: &str) -> String {
    let s = summary.trim();
    let re = regex::Regex::new(r"^#\d+\s*[·•\-–—]\s*").ok();
    if let Some(re) = re {
        re.replace(s, "").into_owned()
    } else {
        s.to_string()
    }
}

/// Extrait `#n` depuis un titre préfixé.
pub fn parse_occ_prefix(summary: &str) -> Option<u32> {
    let re = regex::Regex::new(r"^#(\d+)\s*[·•\-–—]\s*").ok()?;
    let caps = re.captures(summary.trim())?;
    caps.get(1)?.as_str().parse().ok()
}

/// Lit les `EXDATE` d’un VEVENT (`YYYY-MM-DD HH:MM`).
pub fn parse_exdates_from_ical(ical: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in ical.lines() {
        let line = raw_line.trim();
        let upper = line.to_ascii_uppercase();
        if !upper.starts_with("EXDATE") {
            continue;
        }
        let Some(val) = line.split(':').next_back() else {
            continue;
        };
        for part in val.split(',') {
            let digits: String = part.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 8 {
                let date = format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8]);
                let time = if digits.len() >= 12 {
                    format!("{}:{}", &digits[8..10], &digits[10..12])
                } else {
                    "00:00".into()
                };
                out.push(format!("{date} {time}"));
            }
        }
    }
    out
}

/// Ajoute `EXDATE` pour exclure une occurrence (suppression / détachement « cette fois »).
///
/// Insertion chirurgicale dans le VEVENT — pas un second moteur iCal.
pub fn add_exdate(ical: &str, occurrence_ui: &str) -> Result<String> {
    let Some(dt) = parse_ui_naive_dt(occurrence_ui) else {
        return Err(TcalBridgeError::Toml(format!(
            "date d’occurrence invalide: {occurrence_ui}"
        )));
    };
    let ex = dt.format("%Y%m%dT%H%M%S").to_string();
    let upper = ical.to_ascii_uppercase();
    if upper.contains(&format!("EXDATE:{ex}"))
        || upper.contains(&format!("EXDATE;VALUE=DATE-TIME:{ex}"))
    {
        return Ok(ical.to_string());
    }
    let line = format!("EXDATE:{ex}");
    // Insérer avant la première fin de VEVENT.
    let markers = ["\r\nEND:VEVENT", "\nEND:VEVENT", "\r\nend:vevent", "\nend:vevent"];
    for m in markers {
        if let Some(idx) = ical.find(m) {
            let mut out = String::with_capacity(ical.len() + line.len() + 4);
            out.push_str(&ical[..idx]);
            if ical[..idx].ends_with("\r\n") {
                out.push_str(&line);
                out.push_str("\r\n");
            } else if ical[..idx].ends_with('\n') {
                out.push_str(&line);
                out.push('\n');
            } else {
                out.push_str("\r\n");
                out.push_str(&line);
                out.push_str("\r\n");
            }
            out.push_str(&ical[idx..]);
            return Ok(out);
        }
    }
    Err(TcalBridgeError::Tcal(
        "END:VEVENT introuvable pour EXDATE".into(),
    ))
}

fn parse_ui_naive_dt(raw: &str) -> Option<chrono::NaiveDateTime> {
    let cleaned = raw.trim().replace('T', " ");
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%d %H:%M:%S") {
        return Some(dt);
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%d %H:%M") {
        return Some(dt);
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(cleaned.split_whitespace().next()?, "%Y-%m-%d")
    {
        return d.and_hms_opt(0, 0, 0);
    }
    None
}

/// Champs formulaire événement (UI calendrier).
#[derive(Debug, Clone, Default)]
pub struct EventFields {
    pub summary: String,
    pub start: String,
    pub end: String,
    pub description: String,
    pub location: String,
    /// `none` | `daily` | `weekly` | `monthly` | `yearly` | `custom`
    pub rrule: String,
    /// Fréquence réelle quand `rrule == "custom"` (`daily`…`yearly`).
    pub rrule_frequency: String,
    pub rrule_interval: Option<u32>,
    pub rrule_count: Option<u32>,
    /// Date UI `YYYY-MM-DD` (borne UNTIL).
    pub rrule_until: String,
    /// Ex. `tu`, `2tu`, `4tu`, `-1fr`.
    pub rrule_byday: Vec<String>,
    /// EXDATE projetés (`YYYY-MM-DD HH:MM`).
    pub exdates: Vec<String>,
}

/// Champs formulaire tâche (VTODO).
#[derive(Debug, Clone, Default)]
pub struct TodoFields {
    pub summary: String,
    pub due: String,
    pub completed: bool,
    pub description: String,
}

fn seed_vevent(uid: &str) -> String {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//HimaWeb//tcal//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:{}\r\n\
         DTSTAMP:{stamp}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n",
        uid.trim()
    )
}

fn seed_vtodo(uid: &str) -> String {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//HimaWeb//tcal//EN\r\n\
         BEGIN:VTODO\r\n\
         UID:{}\r\n\
         DTSTAMP:{stamp}\r\n\
         END:VTODO\r\n\
         END:VCALENDAR\r\n",
        uid.trim()
    )
}

fn template_for<'a>(src: &'a str, kind: &str) -> Result<TcalTemplate<'a>> {
    TcalTemplate::parse(src)
        .map_err(map_tcal)?
        .with_types(&[kind.to_string()])
        .map_err(map_tcal)
}

/// Lit les champs UI depuis un iCal VEVENT via tcal.
pub fn parse_event_fields(ical: &str) -> Result<EventFields> {
    let tpl = template_for(ical, "event")?;
    let mut f = fields_from_event_toml(&tpl.project())?;
    f.exdates = parse_exdates_from_ical(ical);
    Ok(f)
}

/// Projette une RRULE brute (ou token UI) vers les champs du formulaire.
pub fn project_rrule(raw: &str) -> RecurrenceUi {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") {
        return RecurrenceUi::none();
    }
    let lower = t.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "daily" | "weekly" | "monthly" | "yearly" | "custom"
    ) {
        return RecurrenceUi {
            mode: lower,
            ..Default::default()
        };
    }
    let rule = t
        .trim_start_matches("RRULE:")
        .trim_start_matches("rrule:")
        .trim();
    let ical = format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//HimaWeb//tcal//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:rrule-project\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         DTSTART:20260101T120000\r\n\
         RRULE:{rule}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n"
    );
    match parse_event_fields(&ical) {
        Ok(f) => RecurrenceUi::from_event_fields(&f),
        Err(_) => {
            let freq = ui_to_freq(t);
            if freq.is_empty() {
                RecurrenceUi::none()
            } else {
                RecurrenceUi {
                    mode: freq,
                    ..Default::default()
                }
            }
        }
    }
}

/// Crée un VEVENT (UID frais).
pub fn build_event(fields: &EventFields) -> Result<String> {
    build_event_with_uid(&uuid::Uuid::new_v4().to_string(), fields)
}

pub fn build_event_with_uid(uid: &str, fields: &EventFields) -> Result<String> {
    apply_event_fields(&seed_vevent(uid), fields)
}

/// Met à jour un iCal existant (préserve UID et propriétés non modélisées).
pub fn apply_event_fields(ical: &str, fields: &EventFields) -> Result<String> {
    let mut fields = fields.clone();
    fields.normalize_recurring_span();
    let src = if ical.trim().is_empty() {
        seed_vevent(&uuid::Uuid::new_v4().to_string())
    } else {
        ical.to_string()
    };
    let tpl = template_for(&src, "event")?;
    let edited = overlay_event(&tpl.project(), &fields)?;
    tpl.apply(&edited).map_err(map_tcal)
}

impl EventFields {
    /// Pour une série récurrente, « Fin » = durée d’**une** occurrence.
    /// Si la date de fin est > start+1 jour (confusion fréquente avec UNTIL),
    /// on reclaque sur le jour de début en gardant l’heure de fin — sinon
    /// Thunderbird affiche l’événement **chaque jour** de la plage.
    pub fn normalize_recurring_span(&mut self) {
        let mode = self.rrule.trim().to_ascii_lowercase();
        if mode.is_empty() || mode == "none" {
            return;
        }
        let Some(start_d) = parse_ui_date_only(&self.start) else {
            return;
        };
        let end_raw = if self.end.trim().is_empty() {
            self.start.as_str()
        } else {
            self.end.as_str()
        };
        let Some(end_d) = parse_ui_date_only(end_raw) else {
            return;
        };
        if end_d <= start_d + chrono::Duration::days(1) {
            return;
        }
        let end_time = parse_ui_time_only(end_raw)
            .or_else(|| parse_ui_time_only(&self.start))
            .unwrap_or_else(|| "23:59".into());
        self.end = format!("{} {end_time}", start_d.format("%Y-%m-%d"));
    }
}

fn parse_ui_date_only(raw: &str) -> Option<chrono::NaiveDate> {
    let cleaned = raw.trim().replace('T', " ");
    let date_part = cleaned.split_whitespace().next().unwrap_or("");
    chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

fn parse_ui_time_only(raw: &str) -> Option<String> {
    let cleaned = raw.trim().replace('T', " ");
    let mut parts = cleaned.split_whitespace();
    let _date = parts.next()?;
    let time = parts.next()?;
    if time.len() >= 5 {
        Some(time[..5].to_string())
    } else {
        None
    }
}

pub fn build_todo(fields: &TodoFields) -> Result<String> {
    build_todo_with_uid(&uuid::Uuid::new_v4().to_string(), fields)
}

pub fn build_todo_with_uid(uid: &str, fields: &TodoFields) -> Result<String> {
    apply_todo_fields(&seed_vtodo(uid), fields)
}

pub fn apply_todo_fields(ical: &str, fields: &TodoFields) -> Result<String> {
    let src = if ical.trim().is_empty() {
        seed_vtodo(&uuid::Uuid::new_v4().to_string())
    } else {
        ical.to_string()
    };
    let tpl = template_for(&src, "todo")?;
    let edited = overlay_todo(&tpl.project(), fields)?;
    tpl.apply(&edited).map_err(map_tcal)
}

fn fields_from_event_toml(toml_src: &str) -> Result<EventFields> {
    #[derive(Default, Deserialize)]
    struct Form {
        #[serde(default)]
        summary: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        location: String,
        #[serde(rename = "date-start", default)]
        date_start: TomlDate,
        #[serde(rename = "date-end", default)]
        date_end: TomlDate,
        #[serde(default)]
        recurrence: Recurrence,
    }

    #[derive(Default, Deserialize)]
    struct Recurrence {
        #[serde(default)]
        frequency: String,
        #[serde(default)]
        interval: TomlInt,
        #[serde(default)]
        count: TomlInt,
        #[serde(default)]
        until: TomlDate,
        #[serde(rename = "by-day", default)]
        by_day: Vec<String>,
    }

    let form: Form = toml::from_str(toml_src).map_err(|e| TcalBridgeError::Toml(e.to_string()))?;
    let freq = form.recurrence.frequency.trim().to_ascii_lowercase();
    let interval = form.recurrence.interval.as_u32();
    let count = form.recurrence.count.as_u32();
    let until = form.recurrence.until.as_date_ui();
    let byday: Vec<String> = form
        .recurrence
        .by_day
        .into_iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let exotic = is_exotic_recurrence(interval, count, &until, &byday);
    let (rrule, rrule_frequency) = if freq.is_empty() {
        ("none".into(), String::new())
    } else if exotic {
        ("custom".into(), freq)
    } else {
        (freq_to_ui(&freq), String::new())
    };

    Ok(EventFields {
        summary: form.summary.trim().to_string(),
        start: form.date_start.as_ui(),
        end: form.date_end.as_ui(),
        description: ical_unescape_text(&form.description),
        location: ical_unescape_text(&form.location),
        rrule,
        rrule_frequency,
        rrule_interval: interval.filter(|&n| n > 1),
        rrule_count: count,
        rrule_until: until,
        rrule_byday: byday,
        exdates: vec![],
    })
}

fn is_exotic_recurrence(
    interval: Option<u32>,
    count: Option<u32>,
    until: &str,
    byday: &[String],
) -> bool {
    interval.is_some_and(|n| n > 1)
        || count.is_some_and(|n| n > 0)
        || !until.trim().is_empty()
        || !byday.is_empty()
}

fn overlay_event(projected: &str, fields: &EventFields) -> Result<String> {
    let mut doc: DocumentMut = projected
        .parse()
        .map_err(|e: toml_edit::TomlError| TcalBridgeError::Toml(e.to_string()))?;

    doc["summary"] = value(fields.summary.trim());
    // tcal / iCal : les retours ligne doivent être des `\n` littéraux, pas de vrais NL
    // (sinon seule la 1ʳᵉ ligne survit dans DESCRIPTION).
    doc["description"] = value(ical_escape_text(&fields.description));
    doc["location"] = value(ical_escape_text(&fields.location));
    doc["date-start"] = value(ui_to_tcal_date(&fields.start));
    doc["date-end"] = value(ui_to_tcal_date(if fields.end.trim().is_empty() {
        &fields.start
    } else {
        &fields.end
    }));

    let freq = effective_freq(fields);
    ensure_recurrence_table(&mut doc);
    if let Some(Item::Table(t)) = doc.get_mut("recurrence") {
        t["frequency"] = value(freq.as_str());
        if freq.is_empty() {
            t["interval"] = value("");
            t["count"] = value("");
            t["until"] = value("");
            t["by-day"] = Item::Value(Array::new().into());
        } else {
            match fields.rrule_interval {
                Some(n) if n > 0 => t["interval"] = value(i64::from(n)),
                _ => t["interval"] = value(""),
            }
            match fields.rrule_count {
                Some(n) if n > 0 => t["count"] = value(i64::from(n)),
                _ => t["count"] = value(""),
            }
            let until = fields.rrule_until.trim();
            if until.is_empty() {
                t["until"] = value("");
            } else {
                t["until"] = value(ui_to_tcal_until(until));
            }
            let mut days = Array::new();
            for d in &fields.rrule_byday {
                let s = d.trim().to_ascii_lowercase();
                if !s.is_empty() {
                    days.push(s.as_str());
                }
            }
            t["by-day"] = Item::Value(days.into());
        }
    }

    Ok(doc.to_string())
}

fn ensure_recurrence_table(doc: &mut DocumentMut) {
    if !matches!(doc.get("recurrence"), Some(Item::Table(_))) {
        doc["recurrence"] = Item::Table(toml_edit::Table::new());
    }
}

/// Échappe texte iCal (`\` → `\\`, newline → `\n`).
fn ical_escape_text(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\r', "")
        .replace('\n', "\\n")
}

/// Inverse de [`ical_escape_text`] pour l’UI.
fn ical_unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('N') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn overlay_todo(projected: &str, fields: &TodoFields) -> Result<String> {
    let mut doc: DocumentMut = projected
        .parse()
        .map_err(|e: toml_edit::TomlError| TcalBridgeError::Toml(e.to_string()))?;

    doc["summary"] = value(fields.summary.trim());
    doc["description"] = value(ical_escape_text(&fields.description));
    doc["date-due"] = value(ui_to_tcal_date(&fields.due));
    if fields.completed {
        doc["status"] = value("completed");
        doc["percent"] = value("100");
        let stamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        doc["date-completed"] = value(stamp);
    } else {
        doc["status"] = value("needs-action");
        doc["percent"] = value("0");
        doc["date-completed"] = value("");
    }

    Ok(doc.to_string())
}

/// Accepte datetime TOML natif ou chaîne.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(untagged)]
enum TomlDate {
    #[default]
    Empty,
    Str(String),
    Datetime(toml::value::Datetime),
}

impl TomlDate {
    fn as_ui(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Str(s) => tcal_date_to_ui(s),
            Self::Datetime(dt) => tcal_date_to_ui(&dt.to_string()),
        }
    }

    /// Date seule `YYYY-MM-DD` pour le champ UNTIL de l’UI.
    fn as_date_ui(&self) -> String {
        let full = self.as_ui();
        if full.len() >= 10 {
            full[..10].to_string()
        } else {
            full
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(untagged)]
enum TomlInt {
    #[default]
    Empty,
    Int(i64),
    Str(String),
}

impl TomlInt {
    fn as_u32(&self) -> Option<u32> {
        match self {
            Self::Empty => None,
            Self::Int(n) if *n > 0 => Some(*n as u32),
            Self::Int(_) => None,
            Self::Str(s) => s.trim().parse().ok().filter(|n: &u32| *n > 0),
        }
    }
}

/// `2026-10-03T14:00:00` / `20261003T140000` → `2026-10-03 14:00` (UI).
fn tcal_date_to_ui(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    // Déjà format UI
    if t.contains(' ') && t.len() >= 16 {
        return t[..16].to_string();
    }
    let compact: String = t
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if compact.len() >= 8 {
        let y = &compact[0..4];
        let mo = &compact[4..6];
        let d = &compact[6..8];
        if compact.len() >= 13 {
            let h = &compact[9..11];
            let mi = &compact[11..13];
            return format!("{y}-{mo}-{d} {h}:{mi}");
        }
        return format!("{y}-{mo}-{d}");
    }
    t.replace('T', " ").chars().take(16).collect()
}

/// UI `2026-10-03 14:00` / `2026-10-03` → forme « friendly » tcal (`YYYY-MM-DD HH:MM:SS`).
///
/// Important : tcal `parse_friendly_date` attend un **espace** entre date et heure
/// (pas un `T`). Avec un `T`, la valeur est recopiée telle quelle dans le iCal
/// (`DTSTART:2026-10-15T18:00:00`), ce que Thunderbird / CalDAV mal-interprètent
/// (récurrence fantôme, affichage cassé).
fn ui_to_tcal_date(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    let cleaned = t.replace('T', " ");
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%d %H:%M") {
        return dt.format("%Y-%m-%d %H:%M:%S").to_string();
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%d %H:%M:%S") {
        return dt.format("%Y-%m-%d %H:%M:%S").to_string();
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return d.format("%Y-%m-%d").to_string();
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(cleaned.split_whitespace().next().unwrap_or(""), "%Y-%m-%d") {
        return d.format("%Y-%m-%d").to_string();
    }
    // Compact iCal → friendly
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 14 {
        return format!(
            "{}-{}-{} {}:{}:{}",
            &digits[0..4],
            &digits[4..6],
            &digits[6..8],
            &digits[8..10],
            &digits[10..12],
            &digits[12..14]
        );
    }
    if digits.len() >= 8 {
        return format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8]);
    }
    cleaned
}

/// UNTIL UI `YYYY-MM-DD` → fin de journée friendly pour tcal.
fn ui_to_tcal_until(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return format!("{} 23:59:00", d.format("%Y-%m-%d"));
    }
    ui_to_tcal_date(t)
}

fn effective_freq(fields: &EventFields) -> String {
    let mode = fields.rrule.trim().to_ascii_lowercase();
    if mode == "custom" {
        ui_to_freq(&fields.rrule_frequency)
    } else {
        ui_to_freq(&fields.rrule)
    }
}

fn ui_to_freq(rrule: &str) -> String {
    let t = rrule.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") || t.eq_ignore_ascii_case("custom") {
        return String::new();
    }
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "daily" | "quotidien" => "daily".into(),
        "weekly" | "hebdo" | "hebdomadaire" => "weekly".into(),
        "monthly" | "mensuel" => "monthly".into(),
        "yearly" | "annuel" => "yearly".into(),
        _ => {
            let upper = t.to_ascii_uppercase();
            let rule = upper.trim_start_matches("RRULE:");
            if let Some(rest) = rule.strip_prefix("FREQ=") {
                let freq = rest.split(';').next().unwrap_or(rest);
                freq.to_ascii_lowercase()
            } else {
                lower
            }
        }
    }
}

fn freq_to_ui(freq: &str) -> String {
    let f = freq.trim().to_ascii_lowercase();
    if f.is_empty() {
        "none".into()
    } else {
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_event_preserves_uid() {
        let src = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\nBEGIN:VEVENT\r\nUID:keep-me\r\nDTSTAMP:20260101T120000Z\r\nDTSTART:20261003T140000\r\nDTEND:20261003T150000\r\nSUMMARY:Old\r\nLOCATION:Paris\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let fields = EventFields {
            summary: "New title".into(),
            start: "2026-10-03 16:00".into(),
            end: "2026-10-03 17:00".into(),
            description: "Notes".into(),
            location: "Lyon".into(),
            rrule: "weekly".into(),
            ..Default::default()
        };
        let out = apply_event_fields(src, &fields).unwrap();
        assert!(out.contains("UID:keep-me"), "uid lost:\n{out}");
        assert!(out.contains("SUMMARY:New title") || out.contains("SUMMARY:New title\r"));
        assert!(out.contains("Lyon"));
        assert!(out.contains("RRULE:") || out.to_ascii_uppercase().contains("WEEKLY"));
        let parsed = parse_event_fields(&out).unwrap();
        assert_eq!(parsed.summary, "New title");
        assert!(parsed.rrule.contains("week") || parsed.rrule == "weekly");
    }

    #[test]
    fn build_event_none_rrule_has_no_rrule() {
        let fields = EventFields {
            summary: "Apéro Synaps".into(),
            start: "2026-10-15 18:00".into(),
            end: "2026-10-15 20:00".into(),
            description: String::new(),
            location: String::new(),
            rrule: "none".into(),
            ..Default::default()
        };
        let out = build_event(&fields).unwrap();
        let upper = out.to_ascii_uppercase();
        assert!(
            !upper.contains("RRULE"),
            "unexpected RRULE for none:\n{out}"
        );
        assert!(
            !upper.contains("FREQ=DAILY"),
            "unexpected daily recurrence:\n{out}"
        );
        // Format iCal compact (pas ISO avec tirets) — sinon TB/CalDAV cassent.
        assert!(
            out.contains("DTSTART:20261015T180000")
                || (out.contains("DTSTART;TZID=") && out.contains(":20261015T180000")),
            "DTSTART must be compact iCal:\n{out}"
        );
        assert!(
            out.contains("DTEND:20261015T200000")
                || (out.contains("DTEND;TZID=") && out.contains(":20261015T200000")),
            "DTEND must be compact iCal:\n{out}"
        );
        assert!(
            !out.contains("DTSTART:2026-10-15"),
            "ISO-extended DTSTART must not be written:\n{out}"
        );
    }

    #[test]
    fn build_event_custom_byday_until_round_trip() {
        let fields = EventFields {
            summary: "Cours".into(),
            start: "2026-11-10 13:00".into(),
            end: "2026-11-10 15:00".into(),
            rrule: "custom".into(),
            rrule_frequency: "monthly".into(),
            rrule_until: "2027-06-08".into(),
            rrule_byday: vec!["2tu".into(), "4tu".into()],
            ..Default::default()
        };
        let out = build_event(&fields).unwrap();
        let upper = out.to_ascii_uppercase();
        assert!(
            upper.contains("RRULE:") && upper.contains("FREQ=MONTHLY"),
            "missing monthly RRULE:\n{out}"
        );
        assert!(
            upper.contains("BYDAY=") && upper.contains("2TU") && upper.contains("4TU"),
            "missing BYDAY 2TU,4TU:\n{out}"
        );
        assert!(
            upper.contains("UNTIL="),
            "missing UNTIL:\n{out}"
        );
        let parsed = parse_event_fields(&out).unwrap();
        assert_eq!(parsed.rrule, "custom");
        assert_eq!(parsed.rrule_frequency, "monthly");
        assert_eq!(parsed.rrule_until, "2027-06-08");
        let days: Vec<_> = parsed
            .rrule_byday
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        assert!(days.iter().any(|d| d == "2tu"), "days={days:?}");
        assert!(days.iter().any(|d| d == "4tu"), "days={days:?}");
    }

    #[test]
    fn numbered_series_keeps_holes_after_exdate() {
        let rec = RecurrenceUi {
            mode: "custom".into(),
            frequency: "monthly".into(),
            until: "2027-06-08".into(),
            byday: vec!["2tu".into(), "4tu".into()],
            exdates: vec!["2026-11-24 13:00".into()],
            ..Default::default()
        };
        let from = chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let occs = expand_occurrence_starts_numbered("2026-10-14 13:00", &rec, from, to);
        let nums: Vec<u32> = occs.iter().map(|(_, n)| *n).collect();
        // 14/10=#1, 28/10=#2, 24/11 EXDATE, 09/12 doit rester #4 (pas #3)
        assert!(
            nums.contains(&1) && nums.contains(&2) && nums.contains(&4) && !nums.contains(&3),
            "expected holes after EXDATE, got {nums:?} from {occs:?}"
        );
    }

    #[test]
    fn multiline_description_survives_round_trip() {
        let fields = EventFields {
            summary: "Multi".into(),
            start: "2026-10-08 10:00".into(),
            end: "2026-10-08 11:00".into(),
            description: "Ligne 1\nLigne 2\nLigne 3".into(),
            ..Default::default()
        };
        let out = build_event(&fields).unwrap();
        let desc_line = out
            .lines()
            .find(|l| l.to_ascii_uppercase().starts_with("DESCRIPTION"))
            .unwrap_or("");
        assert!(
            desc_line.contains("\\n") || desc_line.contains("Ligne 2"),
            "DESCRIPTION must keep all lines (escaped or folded):\n{out}"
        );
        assert!(
            !desc_line.contains('\n') || desc_line.starts_with("DESCRIPTION"),
            "raw newline inside DESCRIPTION value breaks iCal:\n{desc_line:?}"
        );
        let parsed = parse_event_fields(&out).unwrap();
        assert!(
            parsed.description.contains("Ligne 1") && parsed.description.contains("Ligne 2"),
            "round-trip lost lines: {:?}",
            parsed.description
        );
    }

    #[test]
    fn add_exdate_inserts_before_end_vevent() {
        let src = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART:20261110T130000\r\nRRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let out = add_exdate(src, "2026-11-17 13:00").unwrap();
        assert!(
            out.to_ascii_uppercase().contains("EXDATE:20261117T130000"),
            "missing EXDATE:\n{out}"
        );
        assert!(out.contains("END:VEVENT"));
    }

    #[test]
    fn expand_2tu_4tu_november_2026() {
        let rec = RecurrenceUi {
            mode: "custom".into(),
            frequency: "monthly".into(),
            until: "2027-06-08".into(),
            byday: vec!["2tu".into(), "4tu".into()],
            ..Default::default()
        };
        let from = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let occs = expand_occurrence_starts_numbered("2026-11-10 13:00", &rec, from, to);
        let days: Vec<u32> = occs.iter().map(|(d, _)| d.date().day()).collect();
        assert!(
            days.contains(&10) && days.contains(&24),
            "expected 2nd+4th Tue (10,24), got {days:?} from {occs:?}"
        );
        assert!(
            !days.contains(&3) && !days.contains(&17),
            "must not include 1st/3rd Tue, got {days:?}"
        );
    }

    #[test]
    fn recurring_clamps_multi_month_end_to_occurrence() {
        // Confusion UNTIL / date de fin → sinon TB affiche l’événement chaque jour.
        let fields = EventFields {
            summary: "Cours".into(),
            start: "2026-11-10 13:00".into(),
            end: "2027-06-08 15:00".into(),
            rrule: "custom".into(),
            rrule_frequency: "monthly".into(),
            rrule_until: "2027-06-08".into(),
            rrule_byday: vec!["2tu".into(), "4tu".into()],
            ..Default::default()
        };
        let out = build_event(&fields).unwrap();
        let upper = out.to_ascii_uppercase();
        assert!(
            upper.contains("DTEND:20261110T150000")
                || (upper.contains("DTEND;TZID=") && upper.contains(":20261110T150000")),
            "DTEND must stay on occurrence day:\n{out}"
        );
        assert!(
            !upper.contains("DTEND:20270608"),
            "must not span until UNTIL date:\n{out}"
        );
        assert!(upper.contains("UNTIL="), "UNTIL must remain:\n{out}");
    }

    #[test]
    fn build_todo_completed() {
        let fields = TodoFields {
            summary: "Buy milk".into(),
            due: "2026-10-05".into(),
            completed: true,
            description: String::new(),
        };
        let out = build_todo(&fields).unwrap();
        assert!(out.contains("BEGIN:VTODO"));
        assert!(out.contains("Buy milk"));
        assert!(
            out.to_ascii_uppercase().contains("COMPLETED")
                || out.to_ascii_uppercase().contains("PERCENT")
        );
    }
}
