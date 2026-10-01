//! Helpers pour formulaires HTML à champs répétés.

use std::collections::HashMap;

use serde::de::{self, Deserializer, Visitor};
use std::fmt;

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

/// Accepte `accounts=foo` (une valeur) ou `accounts=a&accounts=b` (liste).
pub fn deserialize_string_or_seq<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrSeq;

    impl<'de> Visitor<'de> for StringOrSeq {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string or a sequence of strings")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            if v.is_empty() {
                Ok(vec![])
            } else {
                Ok(vec![v.to_string()])
            }
        }

        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            if v.is_empty() {
                Ok(vec![])
            } else {
                Ok(vec![v])
            }
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                if !s.is_empty() {
                    out.push(s);
                }
            }
            Ok(out)
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(vec![])
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(vec![])
        }
    }

    deserializer.deserialize_any(StringOrSeq)
}

