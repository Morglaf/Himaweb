use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;
use rusqlite::Connection;

use crate::prefs;

#[derive(Debug, Clone)]
pub struct ThunderbirdAddressBook {
    pub name: String,
    pub carddav_url: String,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct LocalContact {
    pub name: String,
    pub email: String,
}

pub fn parse_address_books(prefs_path: &Path) -> Result<Vec<ThunderbirdAddressBook>, String> {
    let content = std::fs::read_to_string(prefs_path).map_err(|e| e.to_string())?;
    let re = Regex::new(r#"user_pref\("([^"]+)",\s*(.+)\);"#).map_err(|e| e.to_string())?;
    let mut prefs: HashMap<String, String> = HashMap::new();
    for cap in re.captures_iter(&content) {
        let key = cap[1].to_string();
        let mut val = cap[2].trim().to_string();
        if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
            val = val[1..val.len() - 1]
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
        }
        prefs.insert(key, val);
    }

    let mut ids = std::collections::BTreeSet::new();
    for key in prefs.keys() {
        if let Some(rest) = key.strip_prefix("ldap_2.servers.") {
            if let Some(id) = rest.split('.').next() {
                ids.insert(id.to_string());
            }
        }
    }

    let mut out = Vec::new();
    for id in ids {
        if id == "history" {
            continue;
        }
        let prefix = format!("ldap_2.servers.{id}");
        let name = prefs
            .get(&format!("{prefix}.description"))
            .cloned()
            .unwrap_or_else(|| id.clone());
        let carddav_url = prefs
            .get(&format!("{prefix}.carddav.url"))
            .cloned()
            .unwrap_or_default();
        let username = prefs
            .get(&format!("{prefix}.carddav.username"))
            .cloned()
            .unwrap_or_default();
        out.push(ThunderbirdAddressBook {
            name,
            carddav_url,
            username,
        });
    }
    Ok(out)
}

pub fn cardamum_toml_from_books(books: &[ThunderbirdAddressBook]) -> String {
    let carddav: Vec<_> = books
        .iter()
        .filter(|b| !b.carddav_url.is_empty())
        .collect();

    let mut out = String::from(
        "# Généré par HimaWeb depuis Thunderbird (CardDAV)\n\
         # Renseignez les mots de passe dans Paramètres → Contacts (GUI),\n\
         # puis testez: cardamum addressbook list\n\n",
    );
    if carddav.is_empty() {
        out.push_str(
            "# Aucun carnet CardDAV trouvé.\n\
             # Importez les contacts locaux (abook.sqlite) vers le cache HimaWeb,\n\
             # ou ajoutez un compte CardDAV manuellement.\n",
        );
        return out;
    }

    for (i, b) in carddav.iter().enumerate() {
        let name = sanitize(&b.name);
        out.push_str(&format!("[accounts.{name}]\n"));
        if i == 0 {
            out.push_str("default = true\n");
        }
        // Prefer home when URL looks like an addressbook collection
        if b.carddav_url.contains("/addressbooks/")
            || b.carddav_url.contains("/Contacts/")
            || b.carddav_url.contains("/dav.php/")
        {
            let home = crate::config_fix::normalize_carddav_home(&b.carddav_url);
            out.push_str(&format!("carddav.home = \"{}\"\n", toml_esc(&home)));
        } else {
            out.push_str(&format!(
                "carddav.server = \"{}\"\n",
                toml_esc(&b.carddav_url)
            ));
        }
        let user = if b.username.is_empty() {
            "USER".into()
        } else {
            b.username.clone()
        };
        out.push_str(&format!(
            "carddav.auth.basic.username = \"{}\"\n",
            toml_esc(&user)
        ));
        out.push_str("# carddav.auth.basic.password.raw = \"MOT_DE_PASSE\"\n\n");
    }
    out
}

pub fn write_cardamum_config(toml: &str, overwrite: bool) -> Result<PathBuf, String> {
    let path = prefs::cardamum_config_path();
    if path.exists() && !overwrite {
        return Err(format!(
            "Le fichier {} existe déjà — cochez écraser.",
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

pub fn upsert_carddav_account(
    name: &str,
    uri: &str,
    username: &str,
    password: &str,
    make_default: bool,
) -> Result<PathBuf, String> {
    let path = prefs::cardamum_config_path();
    let mut body = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::from("# Cardamum config — HimaWeb\n\n")
    };

    if make_default {
        let re = Regex::new(r"(?m)^default\s*=\s*true\s*\n?").unwrap();
        body = re.replace_all(&body, "").into_owned();
    }

    let home_or_server = if uri.contains("/addressbooks/")
        || uri.contains("/Contacts/")
        || uri.contains("/dav.php/")
    {
        "carddav.home"
    } else {
        "carddav.server"
    };
    let uri = if home_or_server == "carddav.home" {
        crate::config_fix::normalize_carddav_home(uri)
    } else {
        uri.to_string()
    };

    let section = format!(
        "[accounts.{name}]\n{default}{home_or_server} = \"{uri}\"\ncarddav.auth.basic.username = \"{user}\"\ncarddav.auth.basic.password.raw = \"{pass}\"\n\n",
        name = sanitize(name),
        default = if make_default {
            "default = true\n"
        } else {
            ""
        },
        uri = toml_esc(&uri),
        user = toml_esc(username),
        pass = toml_esc(password),
    );
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
    write_cardamum_config(&body, true)
}

pub fn set_carddav_password(name: &str, password: &str) -> Result<(), String> {
    let path = prefs::cardamum_config_path();
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

    // nested: carddav.auth.basic.password.raw
    set_nested(
        account,
        &["carddav", "auth", "basic", "password", "raw"],
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
pub struct CardamumAccountEdit {
    pub name: String,
    pub uri: String,
    pub username: String,
    pub is_default: bool,
    pub has_password: bool,
}

pub fn list_cardamum_accounts() -> Result<Vec<CardamumAccountEdit>, String> {
    let path = prefs::cardamum_config_path();
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
        let uri = get_nested_str(t, &["carddav", "home"])
            .or_else(|| get_nested_str(t, &["carddav", "server"]))
            .or_else(|| get_nested_str(t, &["carddav", "discover", "host"]))
            .unwrap_or_default();
        let username =
            get_nested_str(t, &["carddav", "auth", "basic", "username"]).unwrap_or_default();
        let has_password = get_nested_str(t, &["carddav", "auth", "basic", "password", "raw"])
            .map(|s| !s.is_empty())
            .unwrap_or(false)
            || walk_exists(t, &["carddav", "auth", "basic", "password", "raw"]);
        out.push(CardamumAccountEdit {
            name: name.to_string(),
            uri,
            username,
            is_default: t.get("default").and_then(|i| i.as_bool()).unwrap_or(false),
            has_password,
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

fn walk_exists(t: &toml_edit::Table, path: &[&str]) -> bool {
    let mut table = t;
    for (i, part) in path.iter().enumerate() {
        let Some(item) = table.get(part) else {
            return false;
        };
        if i + 1 == path.len() {
            return item.as_value().is_some();
        }
        let Some(next) = item.as_table() else {
            return false;
        };
        table = next;
    }
    false
}

/// Importe contacts locaux depuis abook*.sqlite du profil (dossier parent de prefs.js).
pub fn import_local_contacts(prefs_path: &Path) -> Result<Vec<LocalContact>, String> {
    let profile_dir = prefs_path
        .parent()
        .ok_or("chemin profil invalide")?;
    let mut contacts = Vec::new();
    let pattern = profile_dir.join("abook*.sqlite");
    let glob_str = pattern.to_string_lossy().to_string();
    // manual glob without dependency
    if let Ok(entries) = std::fs::read_dir(profile_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("abook") && name.ends_with(".sqlite") {
                match read_abook_sqlite(&entry.path()) {
                    Ok(mut list) => contacts.append(&mut list),
                    Err(e) => tracing::warn!("abook {}: {e}", entry.path().display()),
                }
            }
        }
    }
    let _ = glob_str;
    // dedupe by email
    let mut seen = std::collections::HashSet::new();
    contacts.retain(|c| {
        let key = c.email.to_ascii_lowercase();
        if key.is_empty() || seen.contains(&key) {
            false
        } else {
            seen.insert(key);
            true
        }
    });
    Ok(contacts)
}

fn read_abook_sqlite(path: &Path) -> Result<Vec<LocalContact>, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    // Prefer structured properties; fall back to _vCard blob
    let mut by_card: HashMap<String, (String, String)> = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT card, name, value FROM properties")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows.flatten() {
        let (card, name, value) = row;
        let entry = by_card.entry(card).or_default();
        match name.as_str() {
            "DisplayName" | "FN" => {
                if entry.0.is_empty() {
                    entry.0 = value;
                }
            }
            "PrimaryEmail" | "SecondEmail" | "Email" | "EMAIL" => {
                if entry.1.is_empty() && value.contains('@') {
                    entry.1 = value;
                }
            }
            "_vCard" => {
                let (fn_, email) = parse_vcard_fn_email(&value);
                if entry.0.is_empty() {
                    entry.0 = fn_;
                }
                if entry.1.is_empty() {
                    entry.1 = email;
                }
            }
            _ => {}
        }
    }

    Ok(by_card
        .into_values()
        .filter(|(_, email)| !email.is_empty())
        .map(|(name, email)| LocalContact { name, email })
        .collect())
}

fn parse_vcard_fn_email(vcard: &str) -> (String, String) {
    let mut fn_ = String::new();
    let mut email = String::new();
    for line in vcard.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with("FN:") || line.starts_with("FN;") {
            if let Some(v) = line.split_once(':').map(|(_, v)| v) {
                if fn_.is_empty() {
                    fn_ = v.to_string();
                }
            }
        } else if line.starts_with("EMAIL") {
            if let Some(v) = line.split_once(':').map(|(_, v)| v) {
                if email.is_empty() && v.contains('@') {
                    email = v.to_string();
                }
            }
        }
    }
    (fn_, email)
}

fn sanitize(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() {
        "contacts".into()
    } else {
        s
    }
}

fn toml_esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
