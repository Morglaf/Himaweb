//! Pont vCard ↔ formulaire web via **tcard** (Pimalaya).
//!
//! Pas de second moteur vCard : projection TOML + fold-back, comme la CLI
//! `tcard template` / `tcard edit`.

use serde::{Deserialize, Serialize};
use tcard::template::TcardTemplate;
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};
use vcard::version::VcardVersion;

use super::cardamum::VcardFields;

#[derive(Debug, thiserror::Error)]
pub enum TcardBridgeError {
    #[error("tcard: {0}")]
    Tcard(String),
    #[error("toml: {0}")]
    Toml(String),
}

type Result<T> = std::result::Result<T, TcardBridgeError>;

fn map_tcard<E: std::fmt::Display>(e: E) -> TcardBridgeError {
    TcardBridgeError::Tcard(e.to_string())
}

/// Carte vide avec UID (même seed que la CLI tcard).
fn seed_vcard(uid: &str) -> String {
    format!(
        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:{}\r\nEND:VCARD\r\n",
        uid.trim().trim_start_matches("urn:uuid:")
    )
}

/// Lit les champs UI depuis un vCard via la projection tcard.
pub fn parse_fields(vcard: &str) -> Result<VcardFields> {
    let tpl = TcardTemplate::parse(vcard, VcardVersion::V4_0).map_err(map_tcard)?;
    let projected = tpl.project();
    let mut fields = fields_from_toml(&projected)?;
    fields.has_photo = vcard_has_photo(vcard);
    Ok(fields)
}

pub fn build_vcard_with_photo(
    uid: &str,
    fields: &VcardFields,
    photo: PhotoEdit,
) -> Result<String> {
    let seed = seed_vcard(uid);
    apply_fields_with_photo(&seed, fields, photo)
}

/// Édition de la photo via le champ tcard `[[photo]].value` (URI / data URI).
#[derive(Debug, Clone, Default)]
pub enum PhotoEdit {
    /// Ne pas toucher au bloc photo projeté (préserve PHOTO binaire).
    #[default]
    Keep,
    /// Supprimer la photo modélisée.
    Clear,
    /// Remplacer par une data URI (`data:image/jpeg;base64,…`).
    Set(String),
}

pub fn apply_fields_with_photo(
    vcard: &str,
    fields: &VcardFields,
    photo: PhotoEdit,
) -> Result<String> {
    let src = if vcard.trim().is_empty() {
        seed_vcard(&uuid::Uuid::new_v4().to_string())
    } else {
        vcard.to_string()
    };
    let tpl = TcardTemplate::parse(&src, VcardVersion::V4_0).map_err(map_tcard)?;
    let projected = tpl.project();
    let edited = overlay_fields(&projected, fields, photo)?;
    tpl.apply(&edited).map_err(map_tcard)
}

/// Construit une data URI pour PHOTO vCard 4.
pub fn photo_data_uri(mime: &str, bytes: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{b64}")
}

fn vcard_has_photo(vcard: &str) -> bool {
    vcard.lines().any(|l| {
        let u = l.trim_start().to_ascii_uppercase();
        u.starts_with("PHOTO:") || u.starts_with("PHOTO;")
    })
}

fn fields_from_toml(toml_src: &str) -> Result<VcardFields> {
    #[derive(Default, Deserialize)]
    struct Form {
        #[serde(rename = "full-name", default)]
        full_name: String,
        #[serde(default)]
        nickname: VecOrString,
        #[serde(default)]
        organization: VecOrString,
        #[serde(default)]
        title: String,
        #[serde(default)]
        note: String,
        #[serde(default)]
        email: Vec<TypedValue>,
        #[serde(default)]
        phone: Vec<TypedValue>,
        #[serde(default)]
        address: Vec<Addr>,
        #[serde(default)]
        url: Vec<TypedValue>,
        #[serde(default)]
        name: NameParts,
    }

    #[derive(Default, Deserialize)]
    struct NameParts {
        #[serde(default)]
        family: VecOrString,
        #[serde(default)]
        given: VecOrString,
    }

    #[derive(Default, Deserialize)]
    struct TypedValue {
        #[serde(default)]
        value: String,
    }

    #[derive(Default, Deserialize)]
    struct Addr {
        #[serde(default)]
        street: VecOrString,
        #[serde(default)]
        locality: String,
        #[serde(default)]
        region: String,
        #[serde(default)]
        code: String,
        #[serde(default)]
        country: String,
    }

    let form: Form = toml::from_str(toml_src).map_err(|e| TcardBridgeError::Toml(e.to_string()))?;
    let emails: Vec<String> = form
        .email
        .into_iter()
        .map(|e| e.value.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let tels: Vec<String> = form
        .phone
        .into_iter()
        .map(|e| e.value.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let url = form
        .url
        .into_iter()
        .map(|u| u.value.trim().to_string())
        .find(|s| !s.is_empty())
        .unwrap_or_default();

    let mut street = String::new();
    let mut city = String::new();
    let mut region = String::new();
    let mut postal = String::new();
    let mut country = String::new();
    if let Some(a) = form.address.into_iter().next() {
        street = a.street.join(", ");
        city = a.locality;
        region = a.region;
        postal = a.code;
        country = a.country;
    }

    let mut fn_name = form.full_name.trim().to_string();
    if fn_name.is_empty() {
        let given = form.name.given.join(" ");
        let family = form.name.family.join(" ");
        fn_name = [given, family]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }

    let mut f = VcardFields {
        fn_name,
        nickname: form.nickname.join(", "),
        email: emails.first().cloned().unwrap_or_default(),
        emails,
        tel: tels.first().cloned().unwrap_or_default(),
        tels,
        org: form.organization.join(", "),
        title: form.title.trim().to_string(),
        note: form.note.trim().to_string(),
        url,
        street: street.trim().to_string(),
        city: city.trim().to_string(),
        region: region.trim().to_string(),
        postal: postal.trim().to_string(),
        country: country.trim().to_string(),
        address: String::new(),
        has_photo: false,
    };
    f.address = format_address(&f);
    Ok(f)
}

/// TOML projeté + champs formulaire → document à plier via tcard.
fn overlay_fields(projected: &str, fields: &VcardFields, photo: PhotoEdit) -> Result<String> {
    let mut doc: DocumentMut = projected
        .parse()
        .map_err(|e: toml_edit::TomlError| TcardBridgeError::Toml(e.to_string()))?;

    doc["full-name"] = value(fields.fn_name.trim());

    let mut nick = Array::new();
    let nick_s = fields.nickname.trim();
    if !nick_s.is_empty() {
        nick.push(nick_s);
    }
    doc["nickname"] = Item::Value(nick.into());

    let mut org = Array::new();
    let org_s = fields.org.trim();
    if !org_s.is_empty() {
        org.push(org_s);
    }
    doc["organization"] = Item::Value(org.into());

    doc["title"] = value(fields.title.trim());
    doc["note"] = value(fields.note.trim());

    // N dérivé du FN si pas déjà rempli utilement
    if doc.get("name").is_some() {
        let parts: Vec<&str> = fields.fn_name.split_whitespace().collect();
        if parts.len() >= 2 {
            let family = parts.last().copied().unwrap_or("");
            let given = parts[..parts.len() - 1].join(" ");
            let mut fam = Array::new();
            if !family.is_empty() {
                fam.push(family);
            }
            let mut giv = Array::new();
            if !given.is_empty() {
                giv.push(given.as_str());
            }
            if let Some(Item::Table(t)) = doc.get_mut("name") {
                t["family"] = Item::Value(fam.into());
                t["given"] = Item::Value(giv.into());
            }
        }
    }

    set_typed_values(&mut doc, "email", &emails_of(fields), "");
    set_typed_values(&mut doc, "phone", &tels_of(fields), "cell");
    set_typed_values(
        &mut doc,
        "url",
        &if fields.url.trim().is_empty() {
            vec![]
        } else {
            vec![fields.url.trim().to_string()]
        },
        "",
    );
    set_address(&mut doc, fields);

    match photo {
        PhotoEdit::Keep => {}
        PhotoEdit::Clear => set_photo_value(&mut doc, ""),
        PhotoEdit::Set(uri) => set_photo_value(&mut doc, &uri),
    }

    Ok(doc.to_string())
}

fn set_photo_value(doc: &mut DocumentMut, uri: &str) {
    let mut aot = ArrayOfTables::new();
    let mut t = Table::new();
    t["value"] = value(uri);
    aot.push(t);
    doc["photo"] = Item::ArrayOfTables(aot);
}

fn emails_of(f: &VcardFields) -> Vec<String> {
    if !f.emails.is_empty() {
        f.emails
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else if !f.email.trim().is_empty() {
        vec![f.email.trim().to_string()]
    } else {
        vec![]
    }
}

fn tels_of(f: &VcardFields) -> Vec<String> {
    if !f.tels.is_empty() {
        f.tels
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else if !f.tel.trim().is_empty() {
        vec![f.tel.trim().to_string()]
    } else {
        vec![]
    }
}

fn set_typed_values(doc: &mut DocumentMut, key: &str, values: &[String], default_type: &str) {
    let mut aot = ArrayOfTables::new();
    if values.is_empty() {
        // Une entrée vide : tcard ignore les blocs vides / absents
        let mut t = Table::new();
        t["type"] = value(default_type);
        t["value"] = value("");
        aot.push(t);
    } else {
        for v in values {
            let mut t = Table::new();
            t["type"] = value(default_type);
            t["value"] = value(v.as_str());
            aot.push(t);
        }
    }
    doc[key] = Item::ArrayOfTables(aot);
}

fn set_address(doc: &mut DocumentMut, fields: &VcardFields) {
    let street = if !fields.street.trim().is_empty() {
        fields.street.trim().to_string()
    } else if !fields.address.trim().is_empty() {
        fields.address.trim().replace('\n', ", ")
    } else {
        String::new()
    };
    let has = [
        street.as_str(),
        fields.city.trim(),
        fields.region.trim(),
        fields.postal.trim(),
        fields.country.trim(),
    ]
    .iter()
    .any(|s| !s.is_empty());

    let mut aot = ArrayOfTables::new();
    let mut t = Table::new();
    t["type"] = value(if has { "home" } else { "" });
    let mut streets = Array::new();
    if !street.is_empty() {
        streets.push(street.as_str());
    }
    t["street"] = Item::Value(streets.into());
    t["locality"] = value(fields.city.trim());
    t["region"] = value(fields.region.trim());
    t["code"] = value(fields.postal.trim());
    t["country"] = value(fields.country.trim());
    aot.push(t);
    doc["address"] = Item::ArrayOfTables(aot);
}

fn format_address(f: &VcardFields) -> String {
    let mut lines = Vec::new();
    if !f.street.trim().is_empty() {
        lines.push(f.street.trim().to_string());
    }
    let mut city_line = String::new();
    if !f.postal.trim().is_empty() {
        city_line.push_str(f.postal.trim());
    }
    if !f.city.trim().is_empty() {
        if !city_line.is_empty() {
            city_line.push(' ');
        }
        city_line.push_str(f.city.trim());
    }
    if !f.region.trim().is_empty() {
        if !city_line.is_empty() {
            city_line.push_str(", ");
        }
        city_line.push_str(f.region.trim());
    }
    if !city_line.is_empty() {
        lines.push(city_line);
    }
    if !f.country.trim().is_empty() {
        lines.push(f.country.trim().to_string());
    }
    lines.join("\n")
}

/// Accepte `nickname = "x"` ou `nickname = ["x"]` selon les projections.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(untagged)]
enum VecOrString {
    #[default]
    Empty,
    One(String),
    Many(Vec<String>),
}

impl VecOrString {
    fn join(&self, sep: &str) -> String {
        match self {
            Self::Empty => String::new(),
            Self::One(s) => s.trim().to_string(),
            Self::Many(v) => v
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(sep),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_photo_line() {
        let src = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:test\r\nFN:Jane Doe\r\nEMAIL:jane@example.com\r\nPHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQ\r\nEND:VCARD\r\n";
        let mut fields = parse_fields(src).unwrap();
        fields.fn_name = "Jane Smith".into();
        fields.note = "hi".into();
        let out = apply_fields_with_photo(src, &fields, PhotoEdit::Keep).unwrap();
        assert!(out.contains("FN:Jane Smith") || out.contains("FN:Jane Smith\r"));
        assert!(out.contains("PHOTO"));
        assert!(out.contains("jane@example.com"));
    }

    #[test]
    fn set_photo_data_uri() {
        let src = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:test\r\nFN:Jane\r\nEMAIL:jane@example.com\r\nEND:VCARD\r\n";
        let fields = parse_fields(src).unwrap();
        let uri = photo_data_uri("image/jpeg", b"\xff\xd8\xff");
        let out = apply_fields_with_photo(src, &fields, PhotoEdit::Set(uri)).unwrap();
        assert!(out.to_ascii_uppercase().contains("PHOTO"), "missing PHOTO in:\n{out}");
        let parsed = crate::cli::cardamum::CardamumClient::parse_vcard_photo(&out);
        assert!(parsed.is_some(), "parse_vcard_photo failed on:\n{out}");
    }
}
