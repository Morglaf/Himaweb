//! Pont iCalendar ↔ formulaire web via **tcal** (Pimalaya).
//!
//! Pas de second moteur iCal : projection TOML + fold-back, comme la CLI
//! `tcal template` / `tcal apply`.

use serde::Deserialize;
use tcal::template::TcalTemplate;
use toml_edit::{DocumentMut, Item, value};

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

/// Champs formulaire événement (UI calendrier).
#[derive(Debug, Clone, Default)]
pub struct EventFields {
    pub summary: String,
    pub start: String,
    pub end: String,
    pub description: String,
    pub location: String,
    /// `none` | `daily` | `weekly` | `monthly` | `yearly` | RRULE brut
    pub rrule: String,
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
    fields_from_event_toml(&tpl.project())
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
    let src = if ical.trim().is_empty() {
        seed_vevent(&uuid::Uuid::new_v4().to_string())
    } else {
        ical.to_string()
    };
    let tpl = template_for(&src, "event")?;
    let edited = overlay_event(&tpl.project(), fields)?;
    tpl.apply(&edited).map_err(map_tcal)
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
    }

    let form: Form = toml::from_str(toml_src).map_err(|e| TcalBridgeError::Toml(e.to_string()))?;
    Ok(EventFields {
        summary: form.summary.trim().to_string(),
        start: form.date_start.as_ui(),
        end: form.date_end.as_ui(),
        description: form.description,
        location: form.location,
        rrule: freq_to_ui(&form.recurrence.frequency),
    })
}

fn overlay_event(projected: &str, fields: &EventFields) -> Result<String> {
    let mut doc: DocumentMut = projected
        .parse()
        .map_err(|e: toml_edit::TomlError| TcalBridgeError::Toml(e.to_string()))?;

    doc["summary"] = value(fields.summary.trim());
    doc["description"] = value(fields.description.as_str());
    doc["location"] = value(fields.location.as_str());
    doc["date-start"] = value(ui_to_tcal_date(&fields.start));
    doc["date-end"] = value(ui_to_tcal_date(if fields.end.trim().is_empty() {
        &fields.start
    } else {
        &fields.end
    }));

    let freq = ui_to_freq(&fields.rrule);
    // Table recurrence (clés pointées du scaffold tcal)
    if let Some(Item::Table(t)) = doc.get_mut("recurrence") {
        t["frequency"] = value(freq.as_str());
        if freq.is_empty() {
            t["interval"] = value("");
            t["count"] = value("");
            t["until"] = value("");
        }
    } else {
        doc["recurrence"]["frequency"] = value(freq.as_str());
    }

    Ok(doc.to_string())
}

fn overlay_todo(projected: &str, fields: &TodoFields) -> Result<String> {
    let mut doc: DocumentMut = projected
        .parse()
        .map_err(|e: toml_edit::TomlError| TcalBridgeError::Toml(e.to_string()))?;

    doc["summary"] = value(fields.summary.trim());
    doc["description"] = value(fields.description.as_str());
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

/// UI `2026-10-03 14:00` / `2026-10-03` → forme tcal `2026-10-03T14:00:00`.
fn ui_to_tcal_date(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    let cleaned = t.replace(' ', "T");
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%dT%H:%M") {
        return dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%dT%H:%M:%S") {
        return dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return d.format("%Y-%m-%d").to_string();
    }
    // Compact iCal
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 14 {
        return format!(
            "{}-{}-{}T{}:{}:{}",
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

fn ui_to_freq(rrule: &str) -> String {
    let t = rrule.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") {
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
