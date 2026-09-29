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
            imap_user: {
                let plain = get_str(t, &["imap", "sasl", "plain", "username"]);
                if plain.is_empty() {
                    get_str(t, &["imap", "sasl", "oauthbearer", "username"])
                } else {
                    plain
                }
            },
            smtp_server: get_str(t, &["smtp", "server"]),
            smtp_user: {
                let plain = get_str(t, &["smtp", "sasl", "plain", "username"]);
                if plain.is_empty() {
                    get_str(t, &["smtp", "sasl", "oauthbearer", "username"])
                } else {
                    plain
                }
            },
            is_default: t.get("default").and_then(|i| i.as_bool()).unwrap_or(false),
            has_imap_password: get_str(t, &["imap", "sasl", "plain", "password", "raw"]).len() > 0
                || path_exists(t, &["imap", "sasl", "plain", "password", "raw"])
                || path_exists(t, &["imap", "sasl", "oauthbearer", "token", "command"]),
            has_smtp_password: path_exists(t, &["smtp", "sasl", "plain", "password", "raw"])
                || path_exists(t, &["smtp", "sasl", "oauthbearer", "token", "command"]),
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

/// Supprime toute variante SASL (plain / oauthbearer / …) pour imap ou smtp.
fn clear_sasl(account: &mut Table, transport: &str) {
    if let Some(tr) = account.get_mut(transport).and_then(|i| i.as_table_mut()) {
        tr.remove("sasl");
    }
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

/// Écrit un chemin en clés pointées Himalaya (`imap.server = …`), pas en tables `[….imap]`.
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
            let mut child = Table::new();
            child.set_dotted(true);
            current.insert(part, Item::Table(child));
        } else if let Some(child) = current.get_mut(part).and_then(|i| i.as_table_mut()) {
            child.set_dotted(true);
        }
        current = current.get_mut(part).unwrap().as_table_mut().unwrap();
    }
    current.insert(path[path.len() - 1], Item::Value(Value::from(value)));
}

fn set_path_str_array(t: &mut Table, path: &[&str], values: &[&str]) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        let mut arr = toml_edit::Array::new();
        for v in values {
            arr.push(*v);
        }
        t.insert(path[0], Item::Value(Value::Array(arr)));
        return;
    }
    let mut current = t;
    for part in &path[..path.len() - 1] {
        if current.get(part).and_then(|i| i.as_table()).is_none() {
            let mut child = Table::new();
            child.set_dotted(true);
            current.insert(part, Item::Table(child));
        } else if let Some(child) = current.get_mut(part).and_then(|i| i.as_table_mut()) {
            child.set_dotted(true);
        }
        current = current.get_mut(part).unwrap().as_table_mut().unwrap();
    }
    let mut arr = toml_edit::Array::new();
    for v in values {
        arr.push(*v);
    }
    current.insert(path[path.len() - 1], Item::Value(Value::Array(arr)));
}

fn mark_dotted_recursive(t: &mut Table) {
    t.set_dotted(true);
    let keys: Vec<String> = t
        .iter()
        .filter(|(_, i)| i.is_table())
        .map(|(k, _)| k.to_string())
        .collect();
    for k in keys {
        if let Some(sub) = t.get_mut(&k).and_then(|i| i.as_table_mut()) {
            mark_dotted_recursive(sub);
        }
    }
}

fn is_gmail_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h.contains("gmail.com") || h.contains("googlemail.com")
}

/// Ajoute ou met à jour des comptes Thunderbird dans le config Himalaya existant (merge, sans écraser les autres).
/// Si `use_ortie` et serveur Gmail : SASL oauthbearer + `ortie token show`.
pub fn merge_thunderbird_accounts(
    accounts: &[crate::thunderbird::ThunderbirdAccount],
    use_ortie: bool,
) -> Result<String, String> {
    let path = prefs::himalaya_config_path();
    let text = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let mut doc: DocumentMut = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?
    };

    if doc.get("accounts").and_then(|a| a.as_table()).is_none() {
        doc.insert("accounts", Item::Table(Table::new()));
    }

    let mut added = 0usize;
    let mut updated = 0usize;
    let mut ortie_accounts: Vec<(String, String)> = Vec::new();

    {
        let accounts_t = doc
            .get_mut("accounts")
            .and_then(|a| a.as_table_mut())
            .ok_or("section [accounts] absente")?;

        // Implicit tables for [accounts.foo] — ensure dotted-key friendly
        accounts_t.set_implicit(true);

        let has_default = accounts_t.iter().any(|(_, item)| {
            item.as_table()
                .and_then(|t| t.get("default"))
                .and_then(|i| i.as_bool())
                .unwrap_or(false)
        });

        for acc in accounts {
            let existed = accounts_t.get(&acc.name).is_some();
            if !existed {
                let mut fresh = Table::new();
                fresh.set_implicit(false);
                accounts_t.insert(&acc.name, Item::Table(fresh));
                added += 1;
            } else {
                updated += 1;
            }
            let account = accounts_t
                .get_mut(&acc.name)
                .and_then(|a| a.as_table_mut())
                .ok_or_else(|| format!("compte `{}` introuvable après insert", acc.name))?;

            // Forcer le mode dotted sur d’éventuelles tables imbriquées déjà présentes
            for nest in ["imap", "smtp", "mailbox"] {
                if let Some(sub) = account.get_mut(nest).and_then(|i| i.as_table_mut()) {
                    mark_dotted_recursive(sub);
                }
            }

            set_path(account, &["email"], &acc.email);
            if !acc.display_name.is_empty() {
                set_path(account, &["display-name"], &acc.display_name);
            }
            if !path_exists(account, &["mailbox", "alias", "inbox"]) {
                set_path(account, &["mailbox", "alias", "inbox"], "Inbox");
            }
            if !path_exists(account, &["mailbox", "alias", "trash"]) {
                set_path(account, &["mailbox", "alias", "trash"], "Trash");
            }
            if !path_exists(account, &["mailbox", "alias", "sent"]) {
                set_path(account, &["mailbox", "alias", "sent"], "Sent");
            }
            if !path_exists(account, &["mailbox", "alias", "drafts"]) {
                set_path(account, &["mailbox", "alias", "drafts"], "Drafts");
            }

            let imap_url = if acc.imap_port == 993 {
                format!("imaps://{}:{}", acc.imap_host, acc.imap_port)
            } else {
                format!("imap://{}:{}", acc.imap_host, acc.imap_port)
            };
            set_path(account, &["imap", "server"], &imap_url);
            if acc.imap_port != 993 {
                if let Some(imap) = account.get_mut("imap").and_then(|i| i.as_table_mut()) {
                    imap.set_dotted(true);
                    imap.insert("starttls", Item::Value(Value::from(true)));
                }
            }

            let smtp_url = if !acc.smtp_host.is_empty() {
                Some(if acc.smtp_port == 465 {
                    format!("smtps://{}:{}", acc.smtp_host, acc.smtp_port)
                } else {
                    format!("smtp://{}:{}", acc.smtp_host, acc.smtp_port)
                })
            } else {
                None
            };
            if let Some(ref url) = smtp_url {
                set_path(account, &["smtp", "server"], url);
                if acc.smtp_port != 465 {
                    if let Some(smtp) = account.get_mut("smtp").and_then(|i| i.as_table_mut()) {
                        smtp.set_dotted(true);
                        smtp.insert("starttls", Item::Value(Value::from(true)));
                    }
                }
            }

            let gmail = use_ortie && (is_gmail_host(&acc.imap_host) || is_gmail_host(&acc.email));
            if gmail {
                clear_sasl(account, "imap");
                clear_sasl(account, "smtp");
                set_path(account, &["imap", "sasl", "oauthbearer", "username"], &acc.email);
                set_path_str_array(
                    account,
                    &["imap", "sasl", "oauthbearer", "token", "command"],
                    &["ortie", "token", "show", "-a", &acc.name],
                );
                if smtp_url.is_some() {
                    set_path(account, &["smtp", "sasl", "oauthbearer", "username"], &acc.email);
                    set_path_str_array(
                        account,
                        &["smtp", "sasl", "oauthbearer", "token", "command"],
                        &["ortie", "token", "show", "-a", &acc.name],
                    );
                }
                ortie_accounts.push((acc.name.clone(), acc.email.clone()));
            } else {
                // Ne pas toucher un oauthbearer déjà en place
                let has_oauth = path_exists(account, &["imap", "sasl", "oauthbearer", "username"])
                    || path_exists(account, &["imap", "sasl", "oauthbearer", "token", "command"]);
                if !has_oauth {
                    set_path(account, &["imap", "sasl", "plain", "username"], &acc.imap_user);
                    if smtp_url.is_some() {
                        set_path(
                            account,
                            &["smtp", "sasl", "plain", "username"],
                            if acc.smtp_user.is_empty() {
                                &acc.imap_user
                            } else {
                                &acc.smtp_user
                            },
                        );
                    }
                }
            }

            if !has_default && !existed && added == 1 && updated == 0 {
                account.insert("default", Item::Value(Value::from(true)));
            }
        }
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if path.exists() {
        let bak = PathBuf::from(format!("{}.bak", path.display()));
        let _ = std::fs::copy(&path, &bak);
    }
    std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())?;

    let mut msg = format!(
        "Config Himalaya mis à jour ({added} ajouté(s), {updated} mis à jour) — autres comptes préservés."
    );
    if !ortie_accounts.is_empty() {
        match ensure_ortie_gmail_accounts(&ortie_accounts) {
            Ok(ortie_msg) => {
                msg.push(' ');
                msg.push_str(&ortie_msg);
            }
            Err(e) => {
                msg.push_str(&format!(" Ortie config: {e}"));
            }
        }
        msg.push_str(" Lancez ensuite OAuth : Paramètres → Sync → Ortie (compte = nom Himalaya).");
    } else if added + updated > 0 {
        msg.push_str(" Renseignez les mots de passe via « Modifier un compte » si besoin.");
    }
    Ok(msg)
}

/// Crée/complète les comptes Ortie Gmail (client Thunderbird public + stockage fichier).
pub fn ensure_ortie_gmail_accounts(accounts: &[(String, String)]) -> Result<String, String> {
    let path = ortie_config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tokens_dir = path
        .parent()
        .map(|p| p.join("tokens"))
        .unwrap_or_else(|| PathBuf::from("ortie-tokens"));
    let _ = std::fs::create_dir_all(&tokens_dir);

    let text = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let mut doc: DocumentMut = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?
    };
    if doc.get("accounts").and_then(|a| a.as_table()).is_none() {
        doc.insert("accounts", Item::Table(Table::new()));
    }

    let mut created = 0usize;
    {
        let accounts_t = doc
            .get_mut("accounts")
            .and_then(|a| a.as_table_mut())
            .ok_or("section [accounts] Ortie absente")?;
        for (name, email) in accounts {
            if accounts_t.get(name).is_none() {
                accounts_t.insert(name, Item::Table(Table::new()));
                created += 1;
            }
            let account = accounts_t
                .get_mut(name)
                .and_then(|a| a.as_table_mut())
                .ok_or_else(|| format!("compte Ortie `{name}` manquant"))?;

            // Client public Thunderbird (vérifié Google) — voir README Ortie
            set_path(
                account,
                &["client-id"],
                "406964657835-aq8lmia8j95dhl1a2bvharmfk3t1hgqj.apps.googleusercontent.com",
            );
            set_path(account, &["client-secret", "raw"], "kSmqreRr0qwBWJgbf5Y-PjSU");
            set_path(
                account,
                &["endpoints", "authorization"],
                "https://accounts.google.com/o/oauth2/v2/auth",
            );
            set_path(
                account,
                &["endpoints", "token"],
                "https://oauth2.googleapis.com/token",
            );
            set_path(account, &["endpoints", "redirection"], "http://localhost");
            set_path_str_array(
                account,
                &["scopes"],
                &["https://mail.google.com/"],
            );
            set_path(account, &["extras", "access_type"], "offline");
            set_path(account, &["extras", "prompt"], "consent");
            set_path(account, &["extras", "login_hint"], email);
            account.insert("auto-refresh", Item::Value(Value::from(true)));

            let scripts_dir = path.parent().unwrap_or(tokens_dir.as_path());
            let read_script = scripts_dir.join("token-read.ps1");
            let write_script = scripts_dir.join("token-write.ps1");
            ensure_ortie_storage_scripts(&read_script, &write_script)?;

            let token_file = tokens_dir.join(name);
            let _ = std::fs::create_dir_all(&tokens_dir);
            let token_path = token_file.to_string_lossy().replace('\\', "/");
            let read_ps = read_script.to_string_lossy().replace('\\', "/");
            let write_ps = write_script.to_string_lossy().replace('\\', "/");

            // Scripts -File + stdin (pas de -Command / cmd /C : fragile sous Windows)
            set_path_str_array(
                account,
                &["storage", "read", "command"],
                &[
                    "powershell",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    &read_ps,
                    &token_path,
                ],
            );
            set_path_str_array(
                account,
                &["storage", "write", "command"],
                &[
                    "powershell",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    &write_ps,
                    &token_path,
                ],
            );
        }
    }

    std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())?;
    Ok(format!(
        "Ortie : {created} compte(s) préparé(s) dans {}.",
        path.display()
    ))
}

fn ensure_ortie_storage_scripts(read_script: &PathBuf, write_script: &PathBuf) -> Result<(), String> {
    if let Some(parent) = read_script.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    const READ_PS1: &str = r#"param([Parameter(Mandatory=$true)][string]$Path)
if (-not (Test-Path -LiteralPath $Path)) { exit 1 }
[Console]::Out.Write([IO.File]::ReadAllText($Path))
"#;
    const WRITE_PS1: &str = r#"param([Parameter(Mandatory=$true)][string]$Path)
$dir = Split-Path -Parent $Path
if ($dir -and -not (Test-Path -LiteralPath $dir)) {
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
}
$stdin = [Console]::OpenStandardInput()
$ms = New-Object System.IO.MemoryStream
$stdin.CopyTo($ms)
[IO.File]::WriteAllBytes($Path, $ms.ToArray())
"#;
    std::fs::write(read_script, READ_PS1).map_err(|e| e.to_string())?;
    std::fs::write(write_script, WRITE_PS1).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn ortie_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("ORTIE_CONFIG") {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            // Ne pas couper `C:\...` au `:` sur Windows (ORTIE_CONFIG accepte une liste `:`-délimitée).
            if cfg!(windows) && trimmed.chars().nth(1) == Some(':') {
                return PathBuf::from(trimmed);
            }
            let first = trimmed.split(':').next().unwrap_or(trimmed).trim();
            if !first.is_empty() {
                return PathBuf::from(first);
            }
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ortie")
        .join("config.toml")
}
