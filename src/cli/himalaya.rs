use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::runner::{CliError, CliResult, CliRunner};

#[derive(Clone)]
pub struct HimalayaClient {
    bin: String,
    runner: CliRunner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mailbox {
    pub name: String,
    pub desc: Option<String>,
    pub unread: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub id: String,
    pub flags: Vec<String>,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: String,
    pub has_attachment: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageView {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub date: String,
    pub flags: Vec<String>,
    pub body_html: String,
    pub body_text: String,
    pub attachments: Vec<AttachmentMeta>,
    pub raw_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub id: String,
    pub filename: String,
    pub mime: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub name: String,
    pub is_default: bool,
    pub backends: String,
}

impl HimalayaClient {
    pub fn new(bin: String, runner: CliRunner) -> Self {
        Self { bin, runner }
    }

    pub fn bin(&self) -> &str {
        &self.bin
    }

    async fn json(&self, args: &[&str]) -> CliResult<Value> {
        self.runner.run_json(&self.bin, args).await
    }

    fn with_account<'a>(account: Option<&'a str>, rest: &[&'a str]) -> Vec<&'a str> {
        let mut out = Vec::with_capacity(rest.len() + 2);
        if let Some(a) = account {
            out.push("--account");
            out.push(a);
        }
        out.extend_from_slice(rest);
        out
    }

    pub async fn list_accounts(&self) -> CliResult<Vec<AccountInfo>> {
        let v = self.json(&["account", "list"]).await?;
        Ok(Self::parse_accounts(v))
    }

    fn parse_accounts(v: Value) -> Vec<AccountInfo> {
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
            .map(|item| {
                let name = item
                    .get("name")
                    .or_else(|| item.get("id"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("default")
                    .to_string();
                let is_default = item
                    .get("default")
                    .or_else(|| item.get("is_default"))
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                let backends = match item.get("backends").or_else(|| item.get("backend")) {
                    Some(Value::Array(a)) => a
                        .iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                };
                AccountInfo {
                    name,
                    is_default,
                    backends,
                }
            })
            .collect()
    }

    pub async fn list_mailboxes(&self, account: Option<&str>) -> CliResult<Vec<Mailbox>> {
        let args = Self::with_account(account, &["mailbox", "list"]);
        let v = self.json(&args).await?;
        Ok(Self::parse_mailboxes(v))
    }

    fn parse_mailboxes(v: Value) -> Vec<Mailbox> {
        let arr = match v {
            Value::Array(a) => a,
            Value::Object(o) => o
                .get("mailboxes")
                .or_else(|| o.get("folders"))
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => vec![],
        };

        arr.into_iter()
            .map(|item| {
                let name = item
                    .get("name")
                    .or_else(|| item.get("id"))
                    .or_else(|| item.get("path"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("Inbox")
                    .to_string();
                let desc = item
                    .get("desc")
                    .or_else(|| item.get("description"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                let unread = item
                    .get("unread")
                    .or_else(|| item.get("unseen"))
                    .and_then(|x| match x {
                        Value::Null => None,
                        Value::Number(n) => n.as_u64(),
                        Value::String(s) => s.parse().ok(),
                        _ => None,
                    })
                    .unwrap_or(0);
                Mailbox {
                    name,
                    desc,
                    unread,
                }
            })
            .collect()
    }

    pub async fn count_unseen(
        &self,
        mailbox: &str,
        account: Option<&str>,
    ) -> CliResult<u64> {
        let args = Self::with_account(
            account,
            &[
                "envelope",
                "search",
                "--mailbox",
                mailbox,
                "--page",
                "1",
                "--page-size",
                "200",
                "--",
                "not",
                "flag",
                "seen",
            ],
        );
        let v = self.json(&args).await?;
        Ok(Self::parse_envelopes(v).len() as u64)
    }

    pub async fn list_envelopes(
        &self,
        mailbox: &str,
        page: u32,
        page_size: u32,
        account: Option<&str>,
    ) -> CliResult<Vec<Envelope>> {
        let page_s = page.to_string();
        let size_s = page_size.to_string();
        let args = Self::with_account(
            account,
            &[
                "envelope",
                "list",
                "--mailbox",
                mailbox,
                "--page",
                page_s.as_str(),
                "--page-size",
                size_s.as_str(),
                "--has-attachment",
            ],
        );
        let v = self.json(&args).await?;
        Ok(Self::parse_envelopes(v))
    }

    /// Recherche via le DSL Himalaya (`from`, `subject`, `body`, …).
    pub async fn search_envelopes(
        &self,
        mailbox: &str,
        query_tokens: &[&str],
        page: u32,
        page_size: u32,
        account: Option<&str>,
    ) -> CliResult<Vec<Envelope>> {
        let page_s = page.to_string();
        let size_s = page_size.to_string();
        let mut base = vec![
            "envelope".into(),
            "search".into(),
            "--mailbox".into(),
            mailbox.to_string(),
            "--page".into(),
            page_s,
            "--page-size".into(),
            size_s,
            "--".into(),
        ];
        for t in query_tokens {
            base.push((*t).to_string());
        }
        let args = if let Some(a) = account {
            let mut out = vec!["--account".into(), a.to_string()];
            out.extend(base);
            out
        } else {
            base
        };
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let v = self.json(&args_ref).await?;
        Ok(Self::parse_envelopes(v))
    }

    fn parse_envelopes(v: Value) -> Vec<Envelope> {
        let arr = match v {
            Value::Array(a) => a,
            Value::Object(o) => o
                .get("envelopes")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => vec![],
        };

        arr.into_iter()
            .map(|item| {
                let id = item
                    .get("id")
                    .or_else(|| item.get("uid"))
                    .map(|x| match x {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        _ => String::new(),
                    })
                    .unwrap_or_default();

                let flags = parse_flags(item.get("flags"));

                let subject = item
                    .get("subject")
                    .and_then(|x| x.as_str())
                    .unwrap_or("(sans objet)")
                    .to_string();

                let from = extract_addr(&item, "from");
                let to = extract_addr(&item, "to");
                let date = item
                    .get("date")
                    .or_else(|| item.get("internal_date"))
                    .map(|x| match x {
                        Value::String(s) => s.clone(),
                        other => other.to_string().trim_matches('"').to_string(),
                    })
                    .unwrap_or_default();

                let has_attachment = item
                    .get("has_attachment")
                    .or_else(|| item.get("has-attachment"))
                    .or_else(|| item.get("attachments"))
                    .or_else(|| item.get("att"))
                    .map(|x| match x {
                        Value::Bool(b) => *b,
                        Value::Null => false,
                        Value::Array(a) => !a.is_empty(),
                        Value::Number(n) => n.as_u64().unwrap_or(0) > 0,
                        Value::String(s) => !s.is_empty() && s != "-" && s != " ",
                        _ => false,
                    })
                    .unwrap_or(false);

                Envelope {
                    id,
                    flags,
                    subject,
                    from,
                    to,
                    date,
                    has_attachment,
                }
            })
            .collect()
    }

    pub async fn read_message(
        &self,
        mailbox: &str,
        id: &str,
        account: Option<&str>,
    ) -> CliResult<MessageView> {
        let args = Self::with_account(account, &["message", "read", "--mailbox", mailbox, id]);
        let v = self.json(&args).await?;
        Ok(Self::parse_message(id, v))
    }

    fn parse_message(id: &str, v: Value) -> MessageView {
        let obj = match &v {
            Value::Object(_) => &v,
            Value::Array(a) => a.first().unwrap_or(&Value::Null),
            _ => &v,
        };

        // Himalaya v2: parts + html_body/text_body indices
        let parts = obj.get("parts").and_then(|p| p.as_array());
        let (mut subject, mut from, mut to, mut cc, mut date) = (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );

        if let Some(parts) = parts {
            if let Some(root) = parts.first() {
                if let Some(headers) = root.get("headers").and_then(|h| h.as_array()) {
                    for h in headers {
                        let name = h
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        let value = h.get("value");
                        match name.as_str() {
                            "subject" => subject = header_text(value),
                            "from" => from = header_address(value),
                            "to" => to = header_address(value),
                            "cc" => cc = header_address(value),
                            "date" => date = header_date(value),
                            _ => {}
                        }
                    }
                }
            }
        }

        if subject.is_empty() {
            subject = obj
                .get("subject")
                .and_then(|x| x.as_str())
                .unwrap_or("(sans objet)")
                .to_string();
        }
        if from.is_empty() {
            from = extract_addr(obj, "from");
        }
        if to.is_empty() {
            to = extract_addr(obj, "to");
        }
        if cc.is_empty() {
            cc = extract_addr(obj, "cc");
        }
        if date.is_empty() {
            date = obj
                .get("date")
                .map(|x| match x {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
        }

        let flags = parse_flags(obj.get("flags"));

        let mut body_text = obj
            .get("text")
            .or_else(|| obj.get("body"))
            .or_else(|| obj.get("content"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        let mut body_html = obj
            .get("html")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        if let Some(parts) = parts {
            if body_text.is_empty() {
                if let Some(idx) = obj
                    .get("text_body")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first())
                    .and_then(|x| x.as_u64())
                {
                    body_text = part_body_text(parts, idx as usize);
                }
            }
            if body_html.is_empty() {
                if let Some(idx) = obj
                    .get("html_body")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first())
                    .and_then(|x| x.as_u64())
                {
                    body_html = part_body_html(parts, idx as usize);
                }
            }
        }

        let attachments = obj
            .get("attachments")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .enumerate()
                    .map(|(i, att)| {
                        // attachments may be indices into parts
                        if let Some(idx) = att.as_u64() {
                            let filename = parts
                                .and_then(|p| p.get(idx as usize))
                                .map(|p| part_filename(p))
                                .unwrap_or_else(|| format!("attachment-{idx}"));
                            AttachmentMeta {
                                id: idx.to_string(),
                                filename,
                                mime: "application/octet-stream".into(),
                                size: 0,
                            }
                        } else {
                            AttachmentMeta {
                                id: att
                                    .get("id")
                                    .or_else(|| att.get("index"))
                                    .map(|x| match x {
                                        Value::String(s) => s.clone(),
                                        Value::Number(n) => n.to_string(),
                                        _ => i.to_string(),
                                    })
                                    .unwrap_or_else(|| i.to_string()),
                                filename: att
                                    .get("filename")
                                    .or_else(|| att.get("name"))
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("attachment")
                                    .to_string(),
                                mime: att
                                    .get("mime")
                                    .or_else(|| att.get("type"))
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("application/octet-stream")
                                    .to_string(),
                                size: att.get("size").and_then(|x| x.as_u64()).unwrap_or(0),
                            }
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let raw_preview = if body_html.is_empty() && body_text.is_empty() {
            "(corps vide ou non textuel)".into()
        } else {
            String::new()
        };

        MessageView {
            id: id.to_string(),
            subject,
            from,
            to,
            cc,
            date,
            flags,
            body_html,
            body_text,
            attachments,
            raw_preview,
        }
    }

    pub async fn set_flag(
        &self,
        mailbox: &str,
        id: &str,
        flag: &str,
        add: bool,
        account: Option<&str>,
    ) -> CliResult<()> {
        let action = if add { "add" } else { "remove" };
        let args = Self::with_account(
            account,
            &["flag", action, "--mailbox", mailbox, "--flag", flag, id],
        );
        self.json(&args).await?;
        Ok(())
    }

    pub async fn move_message(
        &self,
        from: &str,
        to: &str,
        id: &str,
        account: Option<&str>,
    ) -> CliResult<()> {
        let args = Self::with_account(
            account,
            &["message", "move", "--from", from, "--to", to, id],
        );
        self.json(&args).await?;
        Ok(())
    }

    pub async fn delete_message(
        &self,
        mailbox: &str,
        id: &str,
        account: Option<&str>,
    ) -> CliResult<()> {
        let args =
            Self::with_account(account, &["message", "delete", "--mailbox", mailbox, id]);
        match self.json(&args).await {
            Ok(_) => Ok(()),
            Err(e) => match self.move_message(mailbox, "Trash", id, account).await {
                Ok(()) => Ok(()),
                Err(_) => Err(e),
            },
        }
    }

    pub async fn download_attachment(
        &self,
        mailbox: &str,
        message_id: &str,
        attachment_id: &str,
        dest_dir: &str,
        account: Option<&str>,
    ) -> CliResult<Vec<u8>> {
        let args = Self::with_account(
            account,
            &[
                "attachment",
                "download",
                "--mailbox",
                mailbox,
                "--dir",
                dest_dir,
                message_id,
                attachment_id,
            ],
        );
        self.json(&args).await?;
        let entries =
            std::fs::read_dir(dest_dir).map_err(|e| CliError::Message(e.to_string()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                return std::fs::read(&path).map_err(|e| CliError::Message(e.to_string()));
            }
        }
        Err(CliError::Message(
            "pièce jointe téléchargée introuvable".into(),
        ))
    }

    pub async fn send_message(
        &self,
        to: &str,
        cc: Option<&str>,
        bcc: Option<&str>,
        subject: &str,
        body: &str,
        account: Option<&str>,
    ) -> CliResult<()> {
        let mut owned: Vec<String> = Vec::new();
        let mut refs: Vec<&str> = Vec::new();
        if let Some(a) = account {
            owned.push(a.to_string());
        }
        // Build owned strings first so refs stay valid
        let account_owned = account.map(str::to_string);
        let to_owned = to.to_string();
        let subject_owned = subject.to_string();
        let body_owned = body.to_string();
        let cc_owned = cc.filter(|s| !s.trim().is_empty()).map(str::to_string);
        let bcc_owned = bcc.filter(|s| !s.trim().is_empty()).map(str::to_string);

        if let Some(ref a) = account_owned {
            refs.push("--account");
            refs.push(a.as_str());
        }
        refs.extend_from_slice(&[
            "message",
            "compose",
            "--to",
            to_owned.as_str(),
            "--subject",
            subject_owned.as_str(),
            "--body",
            body_owned.as_str(),
            "--send",
        ]);
        if let Some(ref c) = cc_owned {
            refs.push("--cc");
            refs.push(c.as_str());
        }
        if let Some(ref b) = bcc_owned {
            refs.push("--bcc");
            refs.push(b.as_str());
        }

        let _ = owned;
        self.json(&refs).await?;
        Ok(())
    }

    pub async fn send_raw_eml(&self, eml: &[u8], account: Option<&str>) -> CliResult<()> {
        let args = Self::with_account(account, &["message", "send"]);
        self.runner
            .run_with_stdin(&self.bin, &args, eml)
            .await
            .map(|_| ())
    }

    pub async fn compose_template(
        &self,
        kind: ComposeKind,
        mailbox: &str,
        id: Option<&str>,
        account: Option<&str>,
    ) -> CliResult<ComposeDraft> {
        let id = id.unwrap_or("");
        let args: Vec<&str> = match kind {
            ComposeKind::New => Self::with_account(account, &["message", "compose"]),
            ComposeKind::Reply => {
                Self::with_account(account, &["message", "reply", "--mailbox", mailbox, id])
            }
            ComposeKind::ReplyAll => Self::with_account(
                account,
                &["message", "reply", "--mailbox", mailbox, "--to", "", id],
            ),
            ComposeKind::Forward => {
                Self::with_account(account, &["message", "forward", "--mailbox", mailbox, id])
            }
        };

        // For reply-all, try without empty --to first via reply then enrich from message
        let result = if matches!(kind, ComposeKind::ReplyAll) {
            let args = Self::with_account(
                account,
                &["message", "reply", "--mailbox", mailbox, id],
            );
            self.json(&args).await
        } else if matches!(kind, ComposeKind::New) {
            // compose without send just needs flags; empty compose may fail — return blank draft
            return Ok(ComposeDraft::default());
        } else {
            self.json(&args).await
        };

        match result {
            Ok(v) => Ok(ComposeDraft::from_value(v)),
            Err(e) => Err(e),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ComposeKind {
    New,
    Reply,
    ReplyAll,
    Forward,
}

#[derive(Debug, Clone, Default)]
pub struct ComposeDraft {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
}

impl ComposeDraft {
    fn from_value(v: Value) -> Self {
        let subject_fallback = v
            .get("subject")
            .and_then(|x| x.as_str())
            .map(str::to_string);
        let content = v
            .get("content")
            .and_then(|c| c.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| match &v {
                Value::String(s) => s.clone(),
                other => other
                    .get("text")
                    .or_else(|| other.get("body"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
            });

        let mut draft = ComposeDraft::default();
        draft.to = extract_addr(&v, "to");
        draft.cc = extract_addr(&v, "cc");
        draft.bcc = extract_addr(&v, "bcc");
        draft.subject = v
            .get("subject")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        let mut in_headers = true;
        let mut body_lines = Vec::new();
        for line in content.lines() {
            if in_headers {
                if line.is_empty() {
                    in_headers = false;
                    continue;
                }
                if let Some((k, val)) = line.split_once(':') {
                    match k.trim().to_ascii_lowercase().as_str() {
                        "to" if draft.to.is_empty() => draft.to = val.trim().to_string(),
                        "cc" if draft.cc.is_empty() => draft.cc = val.trim().to_string(),
                        "bcc" if draft.bcc.is_empty() => draft.bcc = val.trim().to_string(),
                        "subject" if draft.subject.is_empty() => {
                            draft.subject = val.trim().to_string()
                        }
                        _ => {}
                    }
                } else {
                    in_headers = false;
                    body_lines.push(line.to_string());
                }
            } else {
                body_lines.push(line.to_string());
            }
        }
        if !body_lines.is_empty() {
            draft.body = body_lines.join("\n");
        } else if draft.body.is_empty() {
            draft.body = content;
        }
        if draft.subject.is_empty() {
            if let Some(s) = subject_fallback {
                draft.subject = s;
            }
        }
        draft
    }
}

fn parse_flags(flags: Option<&Value>) -> Vec<String> {
    flags
        .and_then(|f| f.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| match x {
                    Value::String(s) => Some(s.clone()),
                    Value::Object(o) => o
                        .get("iana")
                        .or_else(|| o.get("raw"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim_start_matches('\\').to_ascii_lowercase()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn part_body_text(parts: &[Value], idx: usize) -> String {
    parts
        .get(idx)
        .and_then(|p| p.get("body"))
        .and_then(|b| b.get("Text").or_else(|| b.get("text")))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string()
}

fn part_body_html(parts: &[Value], idx: usize) -> String {
    parts
        .get(idx)
        .and_then(|p| p.get("body"))
        .and_then(|b| b.get("Html").or_else(|| b.get("html")))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string()
}

fn part_filename(part: &Value) -> String {
    if let Some(headers) = part.get("headers").and_then(|h| h.as_array()) {
        for h in headers {
            let name = h.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name.eq_ignore_ascii_case("content_disposition")
                || name.eq_ignore_ascii_case("content-disposition")
            {
                // best effort
            }
            if name.eq_ignore_ascii_case("content_type") {
                if let Some(attrs) = h
                    .pointer("/value/ContentType/attributes")
                    .and_then(|a| a.as_array())
                {
                    for attr in attrs {
                        if attr.get("name").and_then(|n| n.as_str()) == Some("name") {
                            if let Some(v) = attr.get("value").and_then(|v| v.as_str()) {
                                return v.to_string();
                            }
                        }
                    }
                }
            }
        }
    }
    "attachment".into()
}

fn header_text(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    if let Some(s) = value.get("Text").and_then(|t| t.as_str()) {
        return s.to_string();
    }
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    String::new()
}

fn header_address(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    if let Some(list) = value.pointer("/Address/List").and_then(|a| a.as_array()) {
        return list
            .iter()
            .filter_map(|x| {
                let email = x
                    .get("address")
                    .or_else(|| x.get("email"))
                    .and_then(|e| e.as_str())?;
                let name = x.get("name").and_then(|n| n.as_str()).unwrap_or("");
                Some(if name.is_empty() {
                    email.to_string()
                } else {
                    format!("{name} <{email}>")
                })
            })
            .collect::<Vec<_>>()
            .join(", ");
    }
    header_text(Some(value))
}

fn header_date(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    if let Some(dt) = value.get("DateTime") {
        let y = dt.get("year").and_then(|x| x.as_i64()).unwrap_or(0);
        let m = dt.get("month").and_then(|x| x.as_u64()).unwrap_or(0);
        let d = dt.get("day").and_then(|x| x.as_u64()).unwrap_or(0);
        let h = dt.get("hour").and_then(|x| x.as_u64()).unwrap_or(0);
        let mi = dt.get("minute").and_then(|x| x.as_u64()).unwrap_or(0);
        return format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}");
    }
    header_text(Some(value))
}

fn extract_addr(item: &Value, key: &str) -> String {
    match item.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| {
                x.get("addr")
                    .or_else(|| x.get("email"))
                    .or_else(|| x.get("address"))
                    .and_then(|e| e.as_str())
                    .map(|email| {
                        let name = x.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        if name.is_empty() {
                            email.to_string()
                        } else {
                            format!("{name} <{email}>")
                        }
                    })
                    .or_else(|| x.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join(", "),
        Some(Value::Object(o)) => {
            if let Some(list) = o.get("List").and_then(|a| a.as_array()) {
                return list
                    .iter()
                    .filter_map(|x| {
                        let email = x
                            .get("address")
                            .or_else(|| x.get("email"))
                            .and_then(|e| e.as_str())?;
                        let name = x.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        Some(if name.is_empty() {
                            email.to_string()
                        } else {
                            format!("{name} <{email}>")
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
            }
            let email = o
                .get("addr")
                .or_else(|| o.get("email"))
                .or_else(|| o.get("address"))
                .and_then(|e| e.as_str())
                .unwrap_or("");
            let name = o.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name.is_empty() {
                email.to_string()
            } else {
                format!("{name} <{email}>")
            }
        }
        _ => String::new(),
    }
}
