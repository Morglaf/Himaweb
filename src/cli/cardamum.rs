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
    #[serde(default)]
    pub tel: String,
    pub addressbook: String,
    pub account: String,
    #[serde(default)]
    pub etag: String,
}

/// Fiche contact enrichie pour le cache (lookup avatar).
#[derive(Debug, Clone)]
pub struct ContactRecord {
    pub email: String,
    pub name: String,
    pub card_id: String,
    pub book_ref: String,
    pub etag: String,
}

/// Champs formulaire contact (list Cardamum = FN/EMAIL/TEL ; détail via `card read` + tcard).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VcardFields {
    pub fn_name: String,
    pub nickname: String,
    pub email: String,
    #[serde(default)]
    pub emails: Vec<String>,
    pub tel: String,
    #[serde(default)]
    pub tels: Vec<String>,
    pub org: String,
    pub title: String,
    pub note: String,
    pub url: String,
    pub address: String,
    pub street: String,
    pub city: String,
    pub region: String,
    pub postal: String,
    pub country: String,
    pub has_photo: bool,
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

            let etag = item
                .get("etag")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();

            let tel = match item.get("tel").or_else(|| item.get("TEL")) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(|x| {
                        x.as_str()
                            .map(str::to_string)
                            .or_else(|| x.get("value").and_then(|v| v.as_str()).map(str::to_string))
                    })
                    .find(|s| !s.is_empty())
                    .unwrap_or_default(),
                _ => String::new(),
            };

            if emails.is_empty() {
                // Afficher quand même les fiches sans email (fn seulement)
                if !name.is_empty() {
                    out.push(ContactSuggest {
                        id: card_id.clone(),
                        name: name.clone(),
                        email: String::new(),
                        tel: tel.clone(),
                        addressbook: book.to_string(),
                        account: account.to_string(),
                        etag: etag.clone(),
                    });
                }
            } else {
                for email in emails {
                    out.push(ContactSuggest {
                        id: card_id.clone(),
                        name: name.clone(),
                        email,
                        tel: tel.clone(),
                        addressbook: book.to_string(),
                        account: account.to_string(),
                        etag: etag.clone(),
                    });
                }
            }
        }
        out
    }

    /// Lit le vCard brut (`card read --json` → `contents`).
    pub async fn read_card(&self, book_ref: &str, card_id: &str) -> CliResult<(String, String)> {
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
        let etag = v
            .get("etag")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let contents = match v.get("contents") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => {
                // Certains backends renvoient des octets JSON
                let bytes: Vec<u8> = arr.iter().filter_map(|x| x.as_u64().map(|n| n as u8)).collect();
                String::from_utf8_lossy(&bytes).into_owned()
            }
            _ => String::new(),
        };
        Ok((etag, contents))
    }

    pub fn to_records(items: &[ContactSuggest]) -> Vec<ContactRecord> {
        items
            .iter()
            .filter(|c| !c.email.trim().is_empty() && !c.id.is_empty())
            .map(|c| {
                let book_ref = if c.account.is_empty() {
                    c.addressbook.clone()
                } else if c.addressbook.contains("::") {
                    c.addressbook.clone()
                } else {
                    format!("{}::{}", c.account, c.addressbook)
                };
                ContactRecord {
                    email: c.email.trim().to_ascii_lowercase(),
                    name: c.name.clone(),
                    card_id: c.id.clone(),
                    book_ref,
                    etag: c.etag.clone(),
                }
            })
            .collect()
    }

    /// Extrait `(extension, octets)` depuis une propriété PHOTO vCard.
    pub fn parse_vcard_photo(vcard: &str) -> Option<(String, Vec<u8>)> {
        let unfolded = unfold_vcard(vcard);
        for line in unfolded.lines() {
            let upper = line.to_ascii_uppercase();
            if !upper.starts_with("PHOTO") {
                continue;
            }
            let Some((_, rest)) = line.split_once(':') else {
                continue;
            };
            // tcard échappe `;` et `,` dans les data URI (RFC 6350)
            let rest = unescape_vcard_text(rest.trim());
            if rest.is_empty() {
                continue;
            }
            // data URI
            if let Some(data) = rest.strip_prefix("data:") {
                let (meta, b64) = data.split_once(',')?;
                let mime = meta.split(';').next().unwrap_or("image/jpeg");
                let ext = mime_to_ext(mime);
                let bytes = decode_b64(b64)?;
                if !bytes.is_empty() {
                    return Some((ext, bytes));
                }
                continue;
            }
            // URI http(s) — ignoré (évite fetch réseau synchrone)
            if rest.starts_with("http://") || rest.starts_with("https://") {
                continue;
            }
            let params = line[..line.find(':').unwrap_or(0)].to_ascii_uppercase();
            let ext = if params.contains("PNG") {
                "png".into()
            } else if params.contains("GIF") {
                "gif".into()
            } else if params.contains("WEBP") {
                "webp".into()
            } else {
                "jpg".into()
            };
            let bytes = decode_b64(&rest)?;
            if !bytes.is_empty() {
                return Some((ext, bytes));
            }
        }
        None
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

    /// Remplace le vCard (`card update`, stdin).
    pub async fn update_card(
        &self,
        book_ref: &str,
        card_id: &str,
        vcard: &[u8],
        if_match: Option<&str>,
    ) -> CliResult<()> {
        let (account, book_id) = split_ref(book_ref);
        let mut owned = Vec::new();
        if let Some(a) = account.as_deref() {
            owned.push("--account".into());
            owned.push(a.to_string());
        }
        owned.extend([
            "card".into(),
            "update".into(),
            "-k".into(),
            book_id,
            card_id.to_string(),
            "-".into(),
        ]);
        if let Some(etag) = if_match.map(str::trim).filter(|s| !s.is_empty()) {
            owned.push("--if-match".into());
            owned.push(etag.to_string());
        }
        let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        self.runner
            .run_with_stdin(&self.bin, &refs, vcard)
            .await?;
        Ok(())
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

/// Id GAL SOGo typiques : `myaccount.com`, `myotheraccount.com`
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

fn unescape_vcard_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn unfold_vcard(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            out.push_str(line.trim_start_matches([' ', '\t']));
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    out
}

fn mime_to_ext(mime: &str) -> String {
    let m = mime.trim().to_ascii_lowercase();
    if m.contains("png") {
        "png".into()
    } else if m.contains("gif") {
        "gif".into()
    } else if m.contains("webp") {
        "webp".into()
    } else {
        "jpg".into()
    }
}

fn decode_b64(raw: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\r' && *c != '\n')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(&cleaned)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&cleaned))
        .ok()
}
