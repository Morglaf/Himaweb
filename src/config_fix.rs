use std::path::PathBuf;

use crate::prefs;

/// Corrige les configs Cardamum générées avec d'anciennes clés (`home-uri` → `home`).
pub fn migrate_cardamum_config() -> Result<bool, String> {
    let path = prefs::cardamum_config_path();
    if !path.is_file() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut fixed = text
        .replace("carddav.home-uri", "carddav.home")
        .replace("carddav.server-uri", "carddav.server");

    // SOGo: home doit être …/Contacts/ (pas le carnet spécifique), sinon card list → 404
    let re = regex::Regex::new(r#"carddav\.home\s*=\s*"([^"]+)""#).unwrap();
    fixed = re
        .replace_all(&fixed, |caps: &regex::Captures| {
            let url = normalize_carddav_home(&caps[1]);
            format!(r#"carddav.home = "{url}""#)
        })
        .into_owned();

    if fixed == text {
        return Ok(false);
    }
    let bak = path.with_extension("toml.bak-migrate");
    let _ = std::fs::copy(&path, &bak);
    std::fs::write(&path, &fixed).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Remonte d'un segment si l'URL pointe directement sur un carnet.
pub fn normalize_carddav_home(url: &str) -> String {
    let mut u = url.trim().trim_end_matches('/').to_string();
    // .../Contacts/<id> → .../Contacts
    if let Some(idx) = u.rfind("/Contacts/") {
        let after = &u[idx + "/Contacts/".len()..];
        if !after.is_empty() && !after.contains('/') {
            u = u[..idx + "/Contacts".len()].to_string();
        }
    }
    // .../addressbooks/user/book → .../addressbooks/user
    if let Some(idx) = u.find("/addressbooks/") {
        let rest = &u[idx + "/addressbooks/".len()..];
        let parts: Vec<_> = rest.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 {
            u = format!(
                "{}addressbooks/{}",
                &u[..idx + 1],
                parts[0]
            );
        }
    }
    if !u.ends_with('/') {
        u.push('/');
    }
    u
}

/// Propose caldav.home si l'URL pointe déjà vers un calendrier / home-set.
pub fn normalize_caldav_url(url: &str) -> (String, &'static str) {
    let mut u = url.trim().trim_end_matches('/').to_string();

    // SOGo: …/Calendar/<id> → …/Calendar (sinon event list -k <id> → 404)
    if let Some(idx) = u.rfind("/Calendar/") {
        let after = &u[idx + "/Calendar/".len()..];
        if !after.is_empty() && !after.contains('/') {
            u = u[..idx + "/Calendar".len()].to_string();
        }
    }
    // …/calendars/<user>/<cal> → …/calendars/<user>
    if let Some(idx) = u.find("/calendars/") {
        let rest = &u[idx + "/calendars/".len()..];
        let parts: Vec<_> = rest.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 {
            u = format!("{}calendars/{}", &u[..idx + 1], parts[0]);
        }
    }

    if u.contains("/Calendar")
        || u.contains("/calendars/")
        || u.contains("/caldav/")
        || u.ends_with("/dav")
        || u.contains("/SOGo/dav/")
    {
        if !u.ends_with('/') {
            u.push('/');
        }
        (u, "caldav.home")
    } else {
        (u, "caldav.server")
    }
}

/// Corrige `caldav.server` trop précis → `caldav.home`, et normalise les homes.
pub fn migrate_calendula_config() -> Result<bool, String> {
    let path = prefs::calendula_config_path();
    if !path.is_file() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;

    let re_server = regex::Regex::new(r#"caldav\.server\s*=\s*"([^"]+)""#).unwrap();
    let re_home = regex::Regex::new(r#"caldav\.home\s*=\s*"([^"]+)""#).unwrap();

    let mut fixed = re_server
        .replace_all(&text, |caps: &regex::Captures| {
            let (url, key) = normalize_caldav_url(&caps[1]);
            format!(r#"{key} = "{url}""#)
        })
        .into_owned();

    fixed = re_home
        .replace_all(&fixed, |caps: &regex::Captures| {
            let (url, key) = normalize_caldav_url(&caps[1]);
            // Si après normalisation ce n'est plus un home, garder home quand même
            // (on a déjà un home explicite).
            let key = if key == "caldav.server" {
                "caldav.home"
            } else {
                key
            };
            format!(r#"{key} = "{url}""#)
        })
        .into_owned();

    if fixed == text {
        return Ok(false);
    }
    let bak = path.with_extension("toml.bak-migrate");
    let _ = std::fs::copy(&path, &bak);
    std::fs::write(&path, &fixed).map_err(|e| e.to_string())?;
    Ok(true)
}

pub fn delete_toml_account(config_path: &PathBuf, name: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(config_path).map_err(|e| e.to_string())?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let accounts = doc
        .get_mut("accounts")
        .and_then(|a| a.as_table_mut())
        .ok_or("section [accounts] absente")?;
    if accounts.remove(name).is_none() {
        return Err(format!("compte `{name}` introuvable"));
    }
    let bak = config_path.with_extension("toml.bak");
    let _ = std::fs::copy(config_path, &bak);
    std::fs::write(config_path, doc.to_string()).map_err(|e| e.to_string())
}

pub fn delete_himalaya_account(name: &str) -> Result<(), String> {
    delete_toml_account(&prefs::himalaya_config_path(), name)
}

pub fn delete_calendula_account(name: &str) -> Result<(), String> {
    delete_toml_account(&prefs::calendula_config_path(), name)
}

pub fn delete_cardamum_account(name: &str) -> Result<(), String> {
    delete_toml_account(&prefs::cardamum_config_path(), name)
}
