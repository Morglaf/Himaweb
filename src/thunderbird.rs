use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;

#[derive(Debug, Clone)]
pub struct ThunderbirdAccount {
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_user: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
}

pub fn discover_profiles() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(roaming) = dirs::config_dir() else {
        return out;
    };
    for folder in ["Thunderbird", "thunderbird"] {
        let profiles = roaming.join(folder).join("Profiles");
        if let Ok(entries) = std::fs::read_dir(&profiles) {
            for entry in entries.flatten() {
                let prefs = entry.path().join("prefs.js");
                if prefs.is_file() && !out.iter().any(|p| p == &prefs) {
                    out.push(prefs);
                }
            }
        }
    }
    out
}

pub fn parse_prefs_js(path: &Path) -> Result<Vec<ThunderbirdAccount>, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
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

    let accounts_csv = prefs
        .get("mail.accountmanager.accounts")
        .cloned()
        .unwrap_or_default();
    let account_ids: Vec<&str> = accounts_csv
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    let mut result = Vec::new();
    for acc_id in account_ids {
        let server_key = format!("mail.account.{acc_id}.server");
        let ident_key = format!("mail.account.{acc_id}.identities");
        let Some(server_id) = prefs.get(&server_key).cloned() else {
            continue;
        };
        let server_type = prefs
            .get(&format!("mail.server.{server_id}.type"))
            .cloned()
            .unwrap_or_default();
        if server_type != "imap" && server_type != "pop3" {
            continue;
        }

        let imap_host = prefs
            .get(&format!("mail.server.{server_id}.hostname"))
            .cloned()
            .unwrap_or_default();
        let imap_user = prefs
            .get(&format!("mail.server.{server_id}.userName"))
            .cloned()
            .unwrap_or_default();
        let imap_port: u16 = prefs
            .get(&format!("mail.server.{server_id}.port"))
            .and_then(|p| p.parse().ok())
            .unwrap_or(if server_type == "imap" { 993 } else { 995 });

        let identity_id = prefs
            .get(&ident_key)
            .and_then(|s| s.split(',').next().map(str::trim).map(str::to_string))
            .unwrap_or_default();

        let email = prefs
            .get(&format!("mail.identity.{identity_id}.useremail"))
            .cloned()
            .unwrap_or_else(|| imap_user.clone());
        let display_name = prefs
            .get(&format!("mail.identity.{identity_id}.fullName"))
            .cloned()
            .unwrap_or_default();

        let smtp_id = prefs
            .get(&format!("mail.identity.{identity_id}.smtpServer"))
            .cloned()
            .or_else(|| {
                prefs
                    .get("mail.smtpservers")
                    .and_then(|s| s.split(',').next().map(|x| x.trim().to_string()))
            })
            .unwrap_or_default();

        let smtp_host = prefs
            .get(&format!("mail.smtpserver.{smtp_id}.hostname"))
            .cloned()
            .unwrap_or_default();
        let smtp_user = prefs
            .get(&format!("mail.smtpserver.{smtp_id}.username"))
            .cloned()
            .unwrap_or_else(|| imap_user.clone());
        let smtp_port: u16 = prefs
            .get(&format!("mail.smtpserver.{smtp_id}.port"))
            .and_then(|p| p.parse().ok())
            .unwrap_or(465);

        if imap_host.is_empty() || email.is_empty() {
            continue;
        }

        let name = sanitize_account_name(&email);
        result.push(ThunderbirdAccount {
            name,
            email,
            display_name,
            imap_host,
            imap_port,
            imap_user,
            smtp_host,
            smtp_port,
            smtp_user,
        });
    }

    Ok(result)
}

fn sanitize_account_name(email: &str) -> String {
    email
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn is_gmail_account(acc: &ThunderbirdAccount) -> bool {
    let h = acc.imap_host.to_ascii_lowercase();
    let e = acc.email.to_ascii_lowercase();
    h.contains("gmail.com") || h.contains("googlemail.com") || e.ends_with("@gmail.com") || e.ends_with("@googlemail.com")
}

/// Himalaya v2 schema: `[accounts.NAME]` + imap.*/smtp.* keys.
/// Si `use_ortie` et compte Gmail : aperçu oauthbearer + commande Ortie.
pub fn to_himalaya_toml(accounts: &[ThunderbirdAccount], use_ortie: bool) -> String {
    let mut out = String::from(
        "# Généré par HimaWeb depuis Thunderbird (format Himalaya v2)\n\
         # Doc: https://github.com/pimalaya/himalaya/blob/master/config.sample.toml\n\n",
    );

    for (i, acc) in accounts.iter().enumerate() {
        let gmail_oauth = use_ortie && is_gmail_account(acc);
        out.push_str(&format!("[accounts.{}]\n", acc.name));
        if i == 0 {
            out.push_str("default = true\n");
        }
        out.push_str(&format!("email = \"{}\"\n", toml_escape(&acc.email)));
        if !acc.display_name.is_empty() {
            out.push_str(&format!(
                "display-name = \"{}\"\n",
                toml_escape(&acc.display_name)
            ));
        }
        out.push_str("mailbox.alias.inbox = \"Inbox\"\n");
        out.push_str("mailbox.alias.trash = \"Trash\"\n");
        out.push_str("mailbox.alias.sent = \"Sent\"\n");
        out.push_str("mailbox.alias.drafts = \"Drafts\"\n\n");

        if acc.imap_port == 993 {
            out.push_str(&format!(
                "imap.server = \"imaps://{}:{}\"\n",
                acc.imap_host, acc.imap_port
            ));
        } else {
            out.push_str(&format!(
                "imap.server = \"imap://{}:{}\"\n",
                acc.imap_host, acc.imap_port
            ));
            out.push_str("imap.starttls = true\n");
        }

        if gmail_oauth {
            out.push_str(&format!(
                "imap.sasl.oauthbearer.username = \"{}\"\n",
                toml_escape(&acc.email)
            ));
            out.push_str(&format!(
                "imap.sasl.oauthbearer.token.command = [\"ortie\", \"token\", \"show\", \"-a\", \"{}\"]\n\n",
                toml_escape(&acc.name)
            ));
        } else {
            out.push_str(&format!(
                "imap.sasl.plain.username = \"{}\"\n",
                toml_escape(&acc.imap_user)
            ));
            out.push_str("# imap.sasl.plain.password.raw = \"VOTRE_MOT_DE_PASSE\"\n\n");
        }

        if !acc.smtp_host.is_empty() {
            if acc.smtp_port == 465 {
                out.push_str(&format!(
                    "smtp.server = \"smtps://{}:{}\"\n",
                    acc.smtp_host, acc.smtp_port
                ));
            } else {
                out.push_str(&format!(
                    "smtp.server = \"smtp://{}:{}\"\n",
                    acc.smtp_host, acc.smtp_port
                ));
                out.push_str("smtp.starttls = true\n");
            }
            if gmail_oauth {
                out.push_str(&format!(
                    "smtp.sasl.oauthbearer.username = \"{}\"\n",
                    toml_escape(&acc.email)
                ));
                out.push_str(&format!(
                    "smtp.sasl.oauthbearer.token.command = [\"ortie\", \"token\", \"show\", \"-a\", \"{}\"]\n",
                    toml_escape(&acc.name)
                ));
            } else {
                out.push_str(&format!(
                    "smtp.sasl.plain.username = \"{}\"\n",
                    toml_escape(&acc.smtp_user)
                ));
                out.push_str("# smtp.sasl.plain.password.raw = \"VOTRE_MOT_DE_PASSE\"\n");
            }
        }
        out.push('\n');
    }
    out
}
