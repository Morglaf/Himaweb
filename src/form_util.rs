//! Helpers pour formulaires HTML à champs répétés.

use std::collections::HashMap;

/// Collecte toutes les valeurs par clé (répétitions HTML correctement gérées).
pub fn parse_form_lists(raw: &[u8]) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (k, v) in form_urlencoded::parse(raw) {
        map.entry(k.into_owned()).or_default().push(v.into_owned());
    }
    map
}

pub fn form_values<'a>(map: &'a HashMap<String, Vec<String>>, key: &str) -> &'a [String] {
    map.get(key).map(|v| v.as_slice()).unwrap_or(&[])
}

pub fn is_safe_icon_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
