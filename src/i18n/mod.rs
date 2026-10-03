//! Chaînes UI FR / EN / ES / DE / IT. Les configs CLI Pimalaya ne sont pas traduites.

use std::collections::HashMap;
use std::sync::OnceLock;

mod de;
mod en;
mod es;
mod fr;
mod it;

pub fn normalize_locale(raw: &str) -> &'static str {
    let s = raw.trim().to_ascii_lowercase();
    if s.starts_with("en") {
        "en"
    } else if s.starts_with("es") || s.starts_with("spa") {
        "es"
    } else if s.starts_with("de") || s.starts_with("ger") || s.starts_with("deu") {
        "de"
    } else if s.starts_with("it") {
        "it"
    } else {
        "fr"
    }
}

/// Traduction d’une clé (fallback FR, puis clé brute).
pub fn t(locale: &str, key: &str) -> String {
    let loc = normalize_locale(locale);
    catalog()
        .get(loc)
        .and_then(|m| m.get(key).copied())
        .or_else(|| catalog().get("fr").and_then(|m| m.get(key).copied()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| key.to_string())
}

pub fn catalog_for(locale: &str) -> HashMap<String, String> {
    let loc = normalize_locale(locale);
    let mut out = HashMap::new();
    if let Some(base) = catalog().get("fr") {
        for (k, v) in base {
            out.insert((*k).to_string(), (*v).to_string());
        }
    }
    if loc != "fr" {
        if let Some(overlay) = catalog().get(loc) {
            for (k, v) in overlay {
                out.insert((*k).to_string(), (*v).to_string());
            }
        }
    }
    out
}

#[allow(dead_code)]
pub fn catalog_json(locale: &str) -> String {
    serde_json::to_string(&catalog_for(locale)).unwrap_or_else(|_| "{}".into())
}

fn catalog() -> &'static HashMap<&'static str, HashMap<&'static str, &'static str>> {
    static C: OnceLock<HashMap<&'static str, HashMap<&'static str, &'static str>>> = OnceLock::new();
    C.get_or_init(|| {
        let mut root = HashMap::new();
        root.insert("fr", fr::fr());
        root.insert("en", en::en());
        root.insert("es", es::es());
        root.insert("de", de::de());
        root.insert("it", it::it());
        root
    })
}

/// Nom de mois (1–12) selon la locale.
pub fn month_name(locale: &str, month: u32) -> String {
    t(locale, &format!("month.{month}"))
}

/// Jour de la semaine court (0=lun … 6=dim).
pub fn dow_short(locale: &str, i: usize) -> String {
    t(locale, &format!("dow.{i}"))
}

pub fn weekday_name(locale: &str, w: chrono::Weekday) -> String {
    use chrono::Weekday::*;
    let key = match w {
        Mon => "weekday.mon",
        Tue => "weekday.tue",
        Wed => "weekday.wed",
        Thu => "weekday.thu",
        Fri => "weekday.fri",
        Sat => "weekday.sat",
        Sun => "weekday.sun",
    };
    t(locale, key)
}
