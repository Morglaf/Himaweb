use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::runner::{CliError, CliResult, CliRunner};

#[derive(Clone)]
pub struct CardamumClient {
    bin: String,
    runner: CliRunner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactSuggest {
    pub id: String,
    pub name: String,
    pub email: String,
    pub addressbook: String,
    pub account: String,
}

#[derive(Debug, Clone)]
pub struct AddressBookInfo {
    pub id: String,
    pub name: String,
    pub account: String,
}

#[derive(Debug, Clone)]
pub struct CardamumAccount {
    pub name: String,
    pub is_default: bool,
}

impl CardamumClient {
    pub fn new(bin: String, runner: CliRunner) -> Self {
        Self { bin, runner }
    }

    pub async fn list_accounts(&self) -> CliResult<Vec<CardamumAccount>> {
        let v = self.runner.run_json(&self.bin, &["account", "list"]).await?;
        Ok(Self::parse_accounts(v))
    }

    fn parse_accounts(v: Value) -> Vec<CardamumAccount> {
        let arr = match v {
            Value::Array(a) => a,
            Value::Object(o) => o
                .get("accounts")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => vec![],
        };
        arr.into_iter()
            .map(|item| CardamumAccount {
                name: item
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("default")
                    .to_string(),
                is_default: item
                    .get("default")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
            })
            .collect()
    }

    pub async fn list_all_addressbooks(&self) -> CliResult<Vec<AddressBookInfo>> {
        let accounts = self.list_accounts().await.unwrap_or_default();
        if accounts.is_empty() {
            return self.list_addressbooks_for(None).await;
        }
        let mut out = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        let mut last_err: Option<CliError> = None;
        for acc in &accounts {
            match self.list_addressbooks_for(Some(&acc.name)).await {
                Ok(list) => {
                    for mut b in list {
                        if b.account.is_empty() {
                            b.account = acc.name.clone();
                        }
                        let dedupe_key = format!("{}::{}", acc.name, b.id);
                        if !seen_ids.insert(dedupe_key.clone()) {
                            continue;
                        }
                        if !b.id.contains("::") {
                            b.id = format!("{}::{}", acc.name, b.id);
                        }
                        if !b.name.contains(&acc.name) {
                            b.name = format!("{} — {}", acc.name, b.name);
                        }
                        out.push(b);
                    }
                }
                Err(e) => last_err = Some(e),
            }
        }
        if out.is_empty() {
            if let Some(e) = last_err {
                return Err(e);
            }
        }
        Ok(out)
    }

    async fn list_addressbooks_for(
        &self,
        account: Option<&str>,
    ) -> CliResult<Vec<AddressBookInfo>> {
        let attempts: &[&[&str]] = &[&["addressbook", "list"], &["addressbooks", "list"]];
        for base in attempts {
            let args = with_account(account, base);
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            if let Ok(v) = self.runner.run_json(&self.bin, &args_ref).await {
                return Ok(Self::parse_books(v, account.unwrap_or("")));
            }
        }
        Ok(vec![])
    }

    fn parse_books(v: Value, account: &str) -> Vec<AddressBookInfo> {
        let arr = match v {
            Value::Array(a) => a,
            Value::Object(o) => o
                .get("addressbooks")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => vec![],
        };
        arr.into_iter()
            .map(|item| AddressBookInfo {
                id: item
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("default")
                    .to_string(),
                name: item
                    .get("name")
                    .or_else(|| item.get("displayName"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("Carnet")
                    .to_string(),
                account: account.to_string(),
            })
            .collect()
    }

    pub async fn list_contacts_in(
        &self,
        book_ref: Option<&str>,
    ) -> CliResult<Vec<ContactSuggest>> {
        let books = if let Some(r) = book_ref.filter(|s| !s.is_empty() && *s != "__all__") {
            let (acc, id) = split_ref(r);
            vec![(acc, id)]
        } else {
            let all = self.list_all_addressbooks().await?;
            all.into_iter()
                .map(|b| {
                    let (acc, id) = split_ref(&b.id);
                    (acc, id)
                })
                .collect()
        };
        self.list_contacts_from_books(&books).await
    }

    pub async fn list_contacts_from_books(
        &self,
        books: &[(Option<String>, String)],
    ) -> CliResult<Vec<ContactSuggest>> {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        for (account, book_id) in books {
            if looks_like_gal_id(book_id) {
                continue;
            }
            let mut base = vec![
                "card".to_string(),
                "list".to_string(),
                "-k".to_string(),
                book_id.clone(),
                "-s".to_string(),
                "500".to_string(),
            ];
            if let Some(a) = account {
                base.insert(0, a.clone());
                base.insert(0, "--account".into());
            }
            let args_ref: Vec<&str> = base.iter().map(|s| s.as_str()).collect();
            match self.runner.run_json(&self.bin, &args_ref).await {
                Ok(v) => {
                    let acc_label = account.clone().unwrap_or_default();
                    out.extend(Self::parse_cards(v, book_id, &acc_label));
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("404") || msg.contains("501") || msg.contains("405") {
                        tracing::debug!("card list skip {book_id}: {msg}");
                    } else {
                        tracing::warn!("card list {book_id}: {msg}");
                        errors.push(format!("{book_id}: {msg}"));
                    }
                }
            }
        }
        if out.is_empty() && !errors.is_empty() {
            return Err(CliError::Message(errors.join(" · ")));
        }
        Ok(out)
    }

    pub async fn list_contacts(&self) -> CliResult<Vec<ContactSuggest>> {
        self.list_contacts_in(Some("__all__")).await
    }

    fn parse_cards(v: Value, book: &str, account: &str) -> Vec<ContactSuggest> {
        let arr = match v {
            Value::Array(a) => a,
            Value::Object(o) => o
                .get("cards")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => vec![],
        };

        let mut out = Vec::new();
        for item in arr {
            let card_id = item
                .get("id")
                .map(|x| match x {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            let name = item
                .get("fn")
                .or_else(|| item.get("fn_value"))
                .or_else(|| item.get("FN"))
                .or_else(|| item.get("name"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();

            let emails: Vec<String> = match item.get("email").or_else(|| item.get("EMAIL")) {
                Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(|x| {
                        x.as_str()
                            .map(str::to_string)
                            .or_else(|| x.get("value").and_then(|v| v.as_str()).map(str::to_string))
                    })
                    .filter(|s| !s.is_empty())
                    .collect(),
                _ => vec![],
            };

            if emails.is_empty() {
                // Afficher quand même les fiches sans email (fn seulement)
                if !name.is_empty() {
                    out.push(ContactSuggest {
                        id: card_id.clone(),
                        name: name.clone(),
                        email: String::new(),
                        addressbook: book.to_string(),
                        account: account.to_string(),
                    });
                }
            } else {
                for email in emails {
                    out.push(ContactSuggest {
                        id: card_id.clone(),
                        name: name.clone(),
                        email,
                        addressbook: book.to_string(),
                        account: account.to_string(),
                    });
                }
            }
        }
        out
    }

    pub async fn read_card(
        &self,
        book_ref: &str,
        card_id: &str,
    ) -> CliResult<(String, Option<String>)> {
        let (account, book_id) = split_ref(book_ref);
        let mut owned = Vec::new();
        if let Some(a) = account.as_deref() {
            owned.push("--account".into());
            owned.push(a.to_string());
        }
        owned.extend([
            "card".into(),
            "read".into(),
            "-k".into(),
            book_id,
            card_id.to_string(),
        ]);
        let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        let v = self.runner.run_json(&self.bin, &refs).await?;
        let contents = v
            .get("contents")
            .or_else(|| v.get("content"))
            .or_else(|| v.get("vcard"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let etag = v
            .get("etag")
            .and_then(|x| x.as_str())
            .map(str::to_string);
        Ok((contents, etag))
    }

    pub async fn create_card(&self, book_ref: &str, vcard: &[u8]) -> CliResult<()> {
        let (account, book_id) = split_ref(book_ref);
        let mut owned = Vec::new();
        if let Some(a) = account.as_deref() {
            owned.push("--account".into());
            owned.push(a.to_string());
        }
        owned.extend([
            "card".into(),
            "create".into(),
            "-k".into(),
            book_id,
            "-".into(),
        ]);
        let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        self.runner
            .run_with_stdin(&self.bin, &refs, vcard)
            .await?;
        Ok(())
    }

    pub async fn update_card(
        &self,
        book_ref: &str,
        card_id: &str,
        vcard: &[u8],
        etag: Option<&str>,
    ) -> CliResult<()> {
        let (account, book_id) = split_ref(book_ref);
        let mut owned = Vec::new();
        if let Some(a) = account.as_deref() {
            owned.push("--account".into());
            owned.push(a.to_string());
        }
        owned.extend(["card".into(), "update".into(), "-k".into(), book_id]);
        if let Some(e) = etag.filter(|s| !s.is_empty()) {
            owned.push("--if-match".into());
            owned.push(e.to_string());
        }
        owned.push(card_id.to_string());
        owned.push("-".into());
        let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        self.runner
            .run_with_stdin(&self.bin, &refs, vcard)
            .await?;
        Ok(())
    }

    pub async fn delete_card(&self, book_ref: &str, card_id: &str) -> CliResult<()> {
        let (account, book_id) = split_ref(book_ref);
        let mut owned = Vec::new();
        if let Some(a) = account.as_deref() {
            owned.push("--account".into());
            owned.push(a.to_string());
        }
        owned.extend([
            "card".into(),
            "delete".into(),
            "-k".into(),
            book_id,
            card_id.to_string(),
        ]);
        let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        self.runner.run_json(&self.bin, &refs).await?;
        Ok(())
    }

    /// Construit une vCard 3.0 minimale.
    pub fn build_vcard(fn_name: &str, email: &str, tel: &str) -> String {
        let uid = uuid::Uuid::new_v4();
        let mut lines = vec![
            "BEGIN:VCARD".into(),
            "VERSION:3.0".into(),
            format!("UID:{uid}"),
            format!("FN:{}", escape_vcard(fn_name)),
        ];
        if !email.trim().is_empty() {
            lines.push(format!("EMAIL:{}", escape_vcard(email.trim())));
        }
        if !tel.trim().is_empty() {
            lines.push(format!("TEL:{}", escape_vcard(tel.trim())));
        }
        lines.push("END:VCARD".into());
        lines.join("\r\n")
    }

    pub async fn suggest(&self, query: &str) -> CliResult<Vec<ContactSuggest>> {
        let q = query.to_ascii_lowercase();
        let all = self.list_contacts().await?;
        Ok(all
            .into_iter()
            .filter(|c| {
                c.email.to_ascii_lowercase().contains(&q)
                    || c.name.to_ascii_lowercase().contains(&q)
            })
            .take(12)
            .collect())
    }
}

fn with_account(account: Option<&str>, rest: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(a) = account {
        out.push("--account".into());
        out.push(a.to_string());
    }
    out.extend(rest.iter().map(|s| (*s).to_string()));
    out
}

fn split_ref(r: &str) -> (Option<String>, String) {
    if let Some((a, id)) = r.split_once("::") {
        (Some(a.to_string()), id.to_string())
    } else {
        (None, r.to_string())
    }
}

/// Id GAL SOGo typiques : `morglaf.com`, `codecolliders.com`
fn looks_like_gal_id(id: &str) -> bool {
    let id = id.trim();
    if id.is_empty() || id.eq_ignore_ascii_case("personal") {
        return false;
    }
    let has_dot = id.contains('.');
    let no_slash = !id.contains('/');
    let mostly_dns = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    has_dot && no_slash && mostly_dns && !id.contains("6650")
}

fn escape_vcard(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}
