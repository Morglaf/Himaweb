use std::path::PathBuf;

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::prefs;

#[derive(Debug, Clone, Default)]
pub struct AccountEdit {
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub imap_server: String,
    pub imap_user: String,
    pub smtp_server: String,
    pub smtp_user: String,
    pub is_default: bool,
    pub has_imap_password: bool,
    pub has_smtp_password: bool,
    /// `mailbox.alias.trash` — nom réel de la corbeille côté serveur
    pub trash_alias: String,
    pub sent_alias: String,
    pub drafts_alias: String,
}

pub fn list_editable_accounts() -> Result<Vec<AccountEdit>, String> {
    let path = prefs::himalaya_config_path();
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let Some(accounts) = doc.get("accounts").and_then(|a| a.as_table()) else {
        return Ok(vec![]);
    };
    let mut out = Vec::new();
    for (name, item) in accounts.iter() {
        let Some(t) = item.as_table() else { continue };
        out.push(AccountEdit {
            name: name.to_string(),
            email: get_str(t, &["email"]),
            display_name: get_str(t, &["display-name"]),
            imap_server: get_str(t, &["imap", "server"]),
            imap_user: get_str(t, &["imap", "sasl", "plain", "username"]),
            smtp_server: get_str(t, &["smtp", "server"]),
            smtp_user: get_str(t, &["smtp", "sasl", "plain", "username"]),
            is_default: t.get("default").and_then(|i| i.as_bool()).unwrap_or(false),
            has_imap_password: get_str(t, &["imap", "sasl", "plain", "password", "raw"]).len() > 0
                || path_exists(t, &["imap", "sasl", "plain", "password", "raw"]),
            has_smtp_password: path_exists(t, &["smtp", "sasl", "plain", "password", "raw"]),
            trash_alias: get_str(t, &["mailbox", "alias", "trash"]),
            sent_alias: get_str(t, &["mailbox", "alias", "sent"]),
            drafts_alias: get_str(t, &["mailbox", "alias", "drafts"]),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn update_account(
    name: &str,
    email: &str,
    display_name: &str,
    imap_server: &str,
    imap_user: &str,
    imap_password: Option<&str>,
    smtp_server: &str,
    smtp_user: &str,
    smtp_password: Option<&str>,
    make_default: bool,
    trash_alias: Option<&str>,
    sent_alias: Option<&str>,
    drafts_alias: Option<&str>,
) -> Result<(), String> {
    let path = prefs::himalaya_config_path();
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;

    {
        let accounts = doc
            .get_mut("accounts")
            .and_then(|a| a.as_table_mut())
            .ok_or("section [accounts] absente")?;

        if make_default {
            for (_, item) in accounts.iter_mut() {
                if let Some(t) = item.as_table_mut() {
                    t.remove("default");
                }
            }
        }

        let account = accounts
            .get_mut(name)
            .and_then(|a| a.as_table_mut())
            .ok_or_else(|| format!("compte `{name}` introuvable"))?;

        set_path(account, &["email"], email);
        set_path(account, &["display-name"], display_name);
        if !path_exists(account, &["mailbox", "alias", "inbox"]) {
            set_path(account, &["mailbox", "alias", "inbox"], "Inbox");
        }
        set_mailbox_alias(account, "trash", trash_alias);
        set_mailbox_alias(account, "sent", sent_alias);
        set_mailbox_alias(account, "drafts", drafts_alias);
        set_path(account, &["imap", "server"], imap_server);
        set_path(account, &["imap", "sasl", "plain", "username"], imap_user);
        if let Some(pw) = imap_password {
            if !pw.is_empty() {
                set_path(account, &["imap", "sasl", "plain", "password", "raw"], pw);
            }
        }
        if !smtp_server.trim().is_empty() {
            set_path(account, &["smtp", "server"], smtp_server);
            set_path(account, &["smtp", "sasl", "plain", "username"], smtp_user);
            if let Some(pw) = smtp_password {
                if !pw.is_empty() {
                    set_path(account, &["smtp", "sasl", "plain", "password", "raw"], pw);
                }
            }
        }
        if make_default {
            account.insert("default", Item::Value(Value::from(true)));
        }
    }

    let bak = PathBuf::from(format!("{}.bak", path.display()));
    let _ = std::fs::copy(&path, &bak);
    std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())
}

/// Lit `mailbox.alias.<key>` pour un compte (ex. trash).
pub fn get_mailbox_alias(account: &str, alias: &str) -> Option<String> {
    let path = prefs::himalaya_config_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let doc: DocumentMut = text.parse().ok()?;
    let t = doc
        .get("accounts")?
        .as_table()?
        .get(account)?
        .as_table()?;
    let v = get_str(t, &["mailbox", "alias", alias]);
    if v.trim().is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Écrit ou efface `mailbox.alias.<key>` pour un compte.
pub fn set_mailbox_alias_value(
    account: &str,
    alias: &str,
    value: Option<&str>,
) -> Result<(), String> {
    let path = prefs::himalaya_config_path();
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    {
        let accounts = doc
            .get_mut("accounts")
            .and_then(|a| a.as_table_mut())
            .ok_or("section [accounts] absente")?;
        let account_t = accounts
            .get_mut(account)
            .and_then(|a| a.as_table_mut())
            .ok_or_else(|| format!("compte `{account}` introuvable"))?;
        match value.map(str::trim).filter(|s| !s.is_empty()) {
            Some(v) => set_path(account_t, &["mailbox", "alias", alias], v),
            None => remove_path(account_t, &["mailbox", "alias", alias]),
        }
    }
    std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())
}

fn set_mailbox_alias(account: &mut Table, key: &str, value: Option<&str>) {
    match value.map(str::trim).filter(|s| !s.is_empty()) {
        Some(v) => set_path(account, &["mailbox", "alias", key], v),
        None => {
            // Ne pas effacer silenceusement si non fourni — l’appelant passe Some("") pour clear
            if value.is_some() {
                remove_path(account, &["mailbox", "alias", key]);
            }
        }
    }
}

fn remove_path(t: &mut Table, path: &[&str]) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        t.remove(path[0]);
        return;
    }
    let mut current = t;
    for part in &path[..path.len() - 1] {
        let Some(next) = current.get_mut(part).and_then(|i| i.as_table_mut()) else {
            return;
        };
        current = next;
    }
    current.remove(path[path.len() - 1]);
}

fn get_str(t: &Table, path: &[&str]) -> String {
    walk(t, path)
        .and_then(|i| i.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn path_exists(t: &Table, path: &[&str]) -> bool {
    walk(t, path).is_some()
}

fn walk<'a>(t: &'a Table, path: &[&str]) -> Option<&'a Value> {
    if path.is_empty() {
        return None;
    }
    let mut table = t;
    for (i, part) in path.iter().enumerate() {
        let item = table.get(part)?;
        if i + 1 == path.len() {
            return item.as_value();
        }
        table = item.as_table()?;
    }
    None
}

fn set_path(t: &mut Table, path: &[&str], value: &str) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        t.insert(path[0], Item::Value(Value::from(value)));
        return;
    }
    let mut current = t;
    for part in &path[..path.len() - 1] {
        if current.get(part).and_then(|i| i.as_table()).is_none() {
            current.insert(part, Item::Table(Table::new()));
        }
        current = current.get_mut(part).unwrap().as_table_mut().unwrap();
    }
    let last = path[path.len() - 1];
    current.insert(last, Item::Value(Value::from(value)));
}
