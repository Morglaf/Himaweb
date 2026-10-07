use std::path::{Path, PathBuf};

use regex::Regex;

use crate::prefs;
use crate::thunderbird::ThunderbirdAccount;

#[derive(Debug, Clone)]
pub struct ThunderbirdCalendar {
    pub name: String,
    pub cal_type: String,
    pub uri: String,
    pub username: String,
    pub disabled: bool,
}

pub fn parse_calendars(prefs_path: &Path) -> Result<Vec<ThunderbirdCalendar>, String> {
    let content = std::fs::read_to_string(prefs_path).map_err(|e| e.to_string())?;
    let re = Regex::new(r#"user_pref\("([^"]+)",\s*(.+)\);"#).map_err(|e| e.to_string())?;
    let mut prefs_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for cap in re.captures_iter(&content) {
        let key = cap[1].to_string();
        let mut val = cap[2].trim().to_string();
        if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
            val = val[1..val.len() - 1]
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
        }
        prefs_map.insert(key, val);
    }

    let mut ids = std::collections::BTreeSet::new();
    for key in prefs_map.keys() {
        if let Some(rest) = key.strip_prefix("calendar.registry.") {
            if let Some(id) = rest.split('.').next() {
                ids.insert(id.to_string());
            }
        }
    }

    let mut out = Vec::new();
    for id in ids {
        let prefix = format!("calendar.registry.{id}");
        out.push(ThunderbirdCalendar {
            name: prefs_map
                .get(&format!("{prefix}.name"))
                .cloned()
                .unwrap_or_else(|| id.clone()),
            cal_type: prefs_map
                .get(&format!("{prefix}.type"))
                .cloned()
                .unwrap_or_default(),
            uri: prefs_map
                .get(&format!("{prefix}.uri"))
                .cloned()
                .unwrap_or_default(),
            username: prefs_map
                .get(&format!("{prefix}.username"))
                .or_else(|| prefs_map.get(&format!("{prefix}.userName")))
                .cloned()
                .unwrap_or_default(),
            disabled: prefs_map
                .get(&format!("{prefix}.disabled"))
                .map(|v| v == "true")
                .unwrap_or(false),
        });
    }
    Ok(out)
}

pub fn calendula_toml_from_caldav(cals: &[ThunderbirdCalendar]) -> String {
    let caldav: Vec<_> = cals
        .iter()
        .filter(|c| {
            !c.disabled
                && (c.cal_type.eq_ignore_ascii_case("caldav")
                    || c.uri.starts_with("http://")
                    || c.uri.starts_with("https://"))
        })
        .collect();

    let mut out = String::from(
        "# Généré par HimaWeb depuis Thunderbird (CalDAV)\n\
         # Complétez password.raw puis testez: calendula calendar list\n\n",
    );
    if caldav.is_empty() {
        out.push_str(
            "# Aucun calendrier CalDAV trouvé dans Thunderbird.\n\
             # (souvent seul un agenda local « storage » est présent)\n\
             # Ajoutez un compte manuellement ci-dessous ou via Paramètres → Calendrier.\n",
        );
        return out;
    }

    for (i, c) in caldav.iter().enumerate() {
        let name = sanitize(&c.name);
        out.push_str(&format!("[accounts.{name}]\n"));
        if i == 0 {
            out.push_str("default = true\n");
        }
        if c.uri.contains("/calendars") || c.uri.contains("/caldav") {
            out.push_str(&format!("caldav.server = \"{}\"\n", toml_esc(&c.uri)));
        } else {
            out.push_str(&format!("caldav.home = \"{}\"\n", toml_esc(&c.uri)));
        }
        let user = if c.username.is_empty() {
            "USER".into()
        } else {
            c.username.clone()
        };
        out.push_str(&format!(
            "caldav.auth.basic.username = \"{}\"\n",
            toml_esc(&user)
        ));
        out.push_str("# caldav.auth.basic.password.raw = \"MOT_DE_PASSE\"\n\n");
    }
    out
}

pub fn calendula_toml_from_mail(accounts: &[ThunderbirdAccount]) -> String {
    let mut out = String::from(
        "# Proposition CalDAV à partir des comptes mail\n\
         # Adaptez l'URL CalDAV de votre hébergeur, puis password.raw\n\n",
    );
    for (i, a) in accounts.iter().enumerate() {
        let host = a.imap_host.trim();
        if host.is_empty() {
            continue;
        }
        if host.contains("gmail") {
            out.push_str(&format!(
                "# [{}] Gmail → préférez gcal + Ortie (OAuth), pas CalDAV simple\n\n",
                a.name
            ));
            continue;
        }
        let server = format!("https://{host}");
        out.push_str(&format!("[accounts.{}]\n", a.name));
        if i == 0 {
            out.push_str("default = true\n");
        }
        out.push_str(&format!("caldav.server = \"{server}\"\n"));
        out.push_str(&format!(
            "caldav.auth.basic.username = \"{}\"\n",
            toml_esc(&a.imap_user)
        ));
        out.push_str("# caldav.auth.basic.password.raw = \"MOT_DE_PASSE\"\n\n");
    }
    out
}

pub fn write_calendula_config(toml: &str, overwrite: bool) -> Result<PathBuf, String> {
    let path = prefs::calendula_config_path();
    if path.exists() && !overwrite {
        return Err(format!(
            "Le fichier {} existe déjà — cochez écraser ou copiez manuellement.",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        let bak = path.with_extension("toml.bak-himaweb");
        let _ = std::fs::copy(&path, &bak);
    }
    std::fs::write(&path, toml).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn upsert_caldav_account(
    name: &str,
    server: &str,
    username: &str,
    password: &str,
    make_default: bool,
) -> Result<PathBuf, String> {
    let path = prefs::calendula_config_path();
    let mut body = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::from("# Calendula config — HimaWeb\n\n")
    };

    if make_default {
        let re = Regex::new(r"(?m)^default\s*=\s*true\s*\n?").unwrap();
        body = re.replace_all(&body, "").into_owned();
    }

    let section = {
        let (url, key) = crate::config_fix::normalize_caldav_url(server);
        format!(
            "[accounts.{name}]\n{default}{key} = \"{url}\"\ncaldav.auth.basic.username = \"{user}\"\ncaldav.auth.basic.password.raw = \"{pass}\"\n\n",
            name = sanitize(name),
            default = if make_default {
                "default = true\n"
            } else {
                ""
            },
            url = toml_esc(&url),
            user = toml_esc(username),
            pass = toml_esc(password),
        )
    };
    let header = format!("[accounts.{}]", sanitize(name));
    if let Some(start) = body.find(&header) {
        let rest = &body[start + header.len()..];
        let end = rest
            .find("\n[accounts.")
            .map(|i| start + header.len() + i)
            .unwrap_or(body.len());
        body.replace_range(start..end, &section);
    } else {
        if !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&section);
    }
    write_calendula_config(&body, true)
}

pub fn set_caldav_password(name: &str, password: &str) -> Result<(), String> {
    let path = prefs::calendula_config_path();
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let account = doc
        .get_mut("accounts")
        .and_then(|a| a.as_table_mut())
        .and_then(|t| t.get_mut(name))
        .and_then(|a| a.as_table_mut())
        .ok_or_else(|| format!("compte `{name}` introuvable"))?;

    set_nested(
        account,
        &["caldav", "auth", "basic", "password", "raw"],
        password,
    );
    let bak = path.with_extension("toml.bak");
    let _ = std::fs::copy(&path, &bak);
    std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())
}

fn set_nested(t: &mut toml_edit::Table, path: &[&str], value: &str) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        t.insert(path[0], toml_edit::value(value));
        return;
    }
    let mut current = t;
    for part in &path[..path.len() - 1] {
        if current.get(part).and_then(|i| i.as_table()).is_none() {
            current.insert(part, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        current = current.get_mut(part).unwrap().as_table_mut().unwrap();
    }
    current.insert(path[path.len() - 1], toml_edit::value(value));
}

#[derive(Debug, Clone)]
pub struct CalendulaAccountEdit {
    pub name: String,
    pub server: String,
    pub username: String,
    pub is_default: bool,
    pub has_password: bool,
    /// Inclus dans le backup / export (renseigné par settings)
    pub backup_selected: bool,
}

#[derive(Debug, Clone)]
pub struct CaldavBasicAuth {
    pub username: String,
    pub password: String,
    /// `caldav.home` si présent (collection parent des agendas)
    pub home: Option<String>,
}

/// Identifiants Basic + home/server pour un compte Calendula.
pub fn caldav_basic_auth(account: &str) -> Result<CaldavBasicAuth, String> {
    let path = prefs::calendula_config_path();
    if !path.is_file() {
        return Err("Config Calendula introuvable.".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let t = doc
        .get("accounts")
        .and_then(|a| a.as_table())
        .and_then(|a| a.get(account))
        .and_then(|i| i.as_table())
        .ok_or_else(|| format!("Compte Calendula « {account} » introuvable."))?;
    let username = get_nested_str(t, &["caldav", "auth", "basic", "username"])
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Username CalDAV manquant pour « {account} »."))?;
    let password = get_nested_str(t, &["caldav", "auth", "basic", "password", "raw"])
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Mot de passe CalDAV manquant pour « {account} »."))?;
    let home = get_nested_str(t, &["caldav", "home"]).filter(|s| !s.is_empty());
    let _server = get_nested_str(t, &["caldav", "server"]).filter(|s| !s.is_empty());
    Ok(CaldavBasicAuth {
        username,
        password,
        home,
    })
}

pub fn list_calendula_accounts() -> Result<Vec<CalendulaAccountEdit>, String> {
    let path = prefs::calendula_config_path();
    if !path.is_file() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let Some(accounts) = doc.get("accounts").and_then(|a| a.as_table()) else {
        return Ok(vec![]);
    };
    let mut out = Vec::new();
    for (name, item) in accounts.iter() {
        let Some(t) = item.as_table() else { continue };
        let server = get_nested_str(t, &["caldav", "server"])
            .or_else(|| get_nested_str(t, &["caldav", "home"]))
            .unwrap_or_default();
        let username =
            get_nested_str(t, &["caldav", "auth", "basic", "username"]).unwrap_or_default();
        let has_password = get_nested_str(t, &["caldav", "auth", "basic", "password", "raw"])
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        out.push(CalendulaAccountEdit {
            name: name.to_string(),
            server,
            username,
            is_default: t.get("default").and_then(|i| i.as_bool()).unwrap_or(false),
            has_password,
            backup_selected: true,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn get_nested_str(t: &toml_edit::Table, path: &[&str]) -> Option<String> {
    let mut table = t;
    for (i, part) in path.iter().enumerate() {
        let item = table.get(part)?;
        if i + 1 == path.len() {
            return item.as_str().map(str::to_string);
        }
        table = item.as_table()?;
    }
    None
}


fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn toml_esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
