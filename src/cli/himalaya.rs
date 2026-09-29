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
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub in_reply_to: Vec<String>,
    #[serde(default)]
    pub references: Vec<String>,
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

                let message_id = item
                    .get("message-id")
                    .or_else(|| item.get("message_id"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .trim_matches(|c| c == '<' || c == '>')
                    .to_string();

                let in_reply_to = parse_id_list(
                    item
                        .get("in-reply-to")
                        .or_else(|| item.get("in_reply_to")),
                );
                let references = parse_id_list(item.get("references"));

                Envelope {
                    id,
                    flags,
                    subject,
                    from,
                    to,
                    date,
                    has_attachment,
                    message_id,
                    in_reply_to,
                    references,
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
        if account_prefers_copy_move(account) {
            return self.move_via_copy_purge(from, to, id, account).await;
        }
        let args = Self::with_account(
            account,
            &["message", "move", "--from", from, "--to", to, id],
        );
        match self.json(&args).await {
            Ok(_) => Ok(()),
            Err(e) if is_uid_move_unsupported(&e) => {
                self.move_via_copy_purge(from, to, id, account).await
            }
            Err(e) => Err(e),
        }
    }

    pub async fn copy_message(
        &self,
        from: &str,
        to: &str,
        id: &str,
        account: Option<&str>,
    ) -> CliResult<()> {
        let args = Self::with_account(
            account,
            &["message", "copy", "--from", from, "--to", to, id],
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
        if account_prefers_copy_move(account) {
            return self
                .delete_without_uid_move(
                    mailbox,
                    id,
                    account,
                    &CliError::Message("préférence compte: COPY sans UID MOVE".into()),
                )
                .await;
        }
        let args =
            Self::with_account(account, &["message", "delete", "--mailbox", mailbox, id]);
        match self.json(&args).await {
            Ok(_) => Ok(()),
            Err(e) => {
                if is_uid_move_unsupported(&e) {
                    return self
                        .delete_without_uid_move(mailbox, id, account, &e)
                        .await;
                }
                // Ancien fallback : tenter un move vers des noms de poubelle connus
                let mut candidates: Vec<String> = Vec::new();
                if let Some(acc) = account {
                    if let Some(alias) = crate::accounts_config::get_mailbox_alias(acc, "trash") {
                        candidates.push(alias);
                    }
                }
                for name in [
                    "Trash",
                    "Corbeille",
                    "Deleted Items",
                    "Deleted Messages",
                    "INBOX.Trash",
                    "INBOX/Trash",
                    "[Gmail]/Trash",
                    "Bin",
                ] {
                    if !candidates.iter().any(|c| c.eq_ignore_ascii_case(name)) {
                        candidates.push(name.to_string());
                    }
                }
                let mut last_err = e.to_string();
                for trash in &candidates {
                    match self.move_message(mailbox, trash, id, account).await {
                        Ok(()) => return Ok(()),
                        Err(me) if is_uid_move_unsupported(&me) => {
                            return self
                                .delete_without_uid_move(mailbox, id, account, &me)
                                .await;
                        }
                        Err(me) => last_err = me.to_string(),
                    }
                }
                Err(CliError::Message(format!(
                    "{last_err} — vérifiez la poubelle (Paramètres → Mail → Modifier le compte → Poubelle), ou activez « COPY sans UID MOVE »."
                )))
            }
        }
    }

    /// COPY vers `to`, puis purge de l’original dans `from` (serveurs sans UID MOVE).
    async fn move_via_copy_purge(
        &self,
        from: &str,
        to: &str,
        id: &str,
        account: Option<&str>,
    ) -> CliResult<()> {
        let Some(acc) = account.filter(|s| !s.is_empty()) else {
            return Err(CliError::Message(
                "compte requis pour déplacer via COPY (serveur sans UID MOVE).".into(),
            ));
        };
        if from.eq_ignore_ascii_case(to) {
            return Ok(());
        }
        self.copy_message(from, to, id, Some(acc)).await?;
        self.purge_original(from, id, acc).await.map_err(|pe| {
            CliError::Message(format!(
                "copie vers « {to} » ok, mais purge de l’original échouée : {pe}"
            ))
        })
    }

    /// Serveurs IMAP sans UID MOVE : COPY vers la poubelle, puis purge de l’original.
    async fn delete_without_uid_move(
        &self,
        mailbox: &str,
        id: &str,
        account: Option<&str>,
        original_err: &CliError,
    ) -> CliResult<()> {
        let Some(acc) = account.filter(|s| !s.is_empty()) else {
            return Err(CliError::Message(format!(
                "{original_err} — compte requis pour le repli COPY (serveur sans UID MOVE)."
            )));
        };

        let trash = crate::accounts_config::get_mailbox_alias(acc, "trash")
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Trash".into());

        let in_trash = mailbox.eq_ignore_ascii_case(&trash)
            || mailbox.eq_ignore_ascii_case("Trash")
            || mailbox.eq_ignore_ascii_case("Corbeille");

        if !in_trash {
            self.copy_message(mailbox, &trash, id, Some(acc))
                .await
                .map_err(|ce| {
                    CliError::Message(format!(
                        "UID MOVE indisponible ({original_err}) ; COPY vers « {trash} » a aussi échoué : {ce}"
                    ))
                })?;
        }

        self.purge_original(mailbox, id, acc).await.map_err(|pe| {
            CliError::Message(format!(
                "copie en poubelle ok, mais purge de l’original échouée : {pe}"
            ))
        })
    }

    /// Purge permanente via delete Himalaya en forçant alias.trash = dossier source
    /// (évite UID MOVE : Himalaya croit être déjà dans la trash).
    async fn purge_original(&self, mailbox: &str, id: &str, acc: &str) -> CliResult<()> {
        static CONFIG_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _guard = CONFIG_LOCK.lock().await;
        let previous = crate::accounts_config::get_mailbox_alias(acc, "trash");
        let restore_default = previous
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Trash".into());

        crate::accounts_config::set_mailbox_alias_value(acc, "trash", Some(mailbox)).map_err(
            |e| CliError::Message(format!("impossible d’ajuster mailbox.alias.trash : {e}")),
        )?;

        let args =
            Self::with_account(Some(acc), &["message", "delete", "--mailbox", mailbox, id]);
        let purge = self.json(&args).await;

        if let Err(re) =
            crate::accounts_config::set_mailbox_alias_value(acc, "trash", Some(&restore_default))
        {
            tracing::error!(
                "alias trash non restauré pour {acc} (voulu {restore_default}): {re}"
            );
        }

        purge.map(|_| ())
    }
}

fn account_prefers_copy_move(account: Option<&str>) -> bool {
    let Some(acc) = account.filter(|s| !s.is_empty()) else {
        return false;
    };
    crate::prefs::Prefs::load().uses_copy_move(acc)
}

fn is_uid_move_unsupported(err: &CliError) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("command not permitted with uid")
        || (s.contains("move") && s.contains("uid") && (s.contains("bad") || s.contains("failed")))
}

impl HimalayaClient {
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
        from: Option<&str>,
    ) -> CliResult<()> {
        let account_owned = account.map(str::to_string);
        let from_owned = from
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let to_owned = to.to_string();
        let subject_owned = subject.to_string();
        let body_owned = body.to_string();
        let cc_owned = cc.filter(|s| !s.trim().is_empty()).map(str::to_string);
        let bcc_owned = bcc.filter(|s| !s.trim().is_empty()).map(str::to_string);

        let mut refs: Vec<&str> = Vec::new();
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
        if let Some(ref f) = from_owned {
            refs.push("--from");
            refs.push(f.as_str());
        }
        if let Some(ref c) = cc_owned {
            refs.push("--cc");
            refs.push(c.as_str());
        }
        if let Some(ref b) = bcc_owned {
            refs.push("--bcc");
            refs.push(b.as_str());
        }

        self.json(&refs).await?;
        Ok(())
    }

    pub async fn save_draft(
        &self,
        to: &str,
        cc: Option<&str>,
        bcc: Option<&str>,
        subject: &str,
        body: &str,
        account: Option<&str>,
        from: Option<&str>,
        mailbox: &str,
    ) -> CliResult<()> {
        let account_owned = account.map(str::to_string);
        let from_owned = from
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let to_owned = to.to_string();
        let subject_owned = subject.to_string();
        let body_owned = body.to_string();
        let cc_owned = cc.filter(|s| !s.trim().is_empty()).map(str::to_string);
        let bcc_owned = bcc.filter(|s| !s.trim().is_empty()).map(str::to_string);
        let mailbox_owned = if mailbox.trim().is_empty() {
            "drafts".to_string()
        } else {
            mailbox.trim().to_string()
        };

        let mut refs: Vec<&str> = Vec::new();
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
            "--save",
            mailbox_owned.as_str(),
        ]);
        if let Some(ref f) = from_owned {
            refs.push("--from");
            refs.push(f.as_str());
        }
        if let Some(ref c) = cc_owned {
            refs.push("--cc");
            refs.push(c.as_str());
        }
        if let Some(ref b) = bcc_owned {
            refs.push("--bcc");
            refs.push(b.as_str());
        }

        self.json(&refs).await?;
        Ok(())
    }

    pub async fn save_raw_draft(
        &self,
        eml: &[u8],
        mailbox: &str,
        account: Option<&str>,
    ) -> CliResult<()> {
        let mb = if mailbox.trim().is_empty() {
            "drafts"
        } else {
            mailbox.trim()
        };
        let args = Self::with_account(
            account,
            &["message", "add", "--mailbox", mb, "--flag", "draft"],
        );
        self.runner
            .run_with_stdin(&self.bin, &args, eml)
            .await
            .map(|_| ())
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
        if matches!(kind, ComposeKind::New) {
            return Ok(ComposeDraft::default());
        }

        // `message reply|forward` écrit du MIME brut (pas du JSON), même avec --json.
        let args: Vec<&str> = match kind {
            ComposeKind::Reply | ComposeKind::ReplyAll => {
                Self::with_account(account, &["message", "reply", "--mailbox", mailbox, id])
            }
            ComposeKind::Forward => {
                Self::with_account(account, &["message", "forward", "--mailbox", mailbox, id])
            }
            ComposeKind::New => unreachable!(),
        };

        let bytes = self.runner.run_raw(&self.bin, &args).await?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(ComposeDraft::from_mime(&text))
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
    fn from_mime(raw: &str) -> Self {
        let mut draft = ComposeDraft::default();
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut lines = raw.lines().peekable();
        while let Some(line) = lines.next() {
            if line.is_empty() {
                break;
            }
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some((_, v)) = headers.last_mut() {
                    v.push(' ');
                    v.push_str(line.trim());
                }
                continue;
            }
            if let Some((k, val)) = line.split_once(':') {
                headers.push((k.trim().to_ascii_lowercase(), val.trim().to_string()));
            } else {
                break;
            }
        }
        let body: String = lines.collect::<Vec<_>>().join("\n");
        let cte = headers
            .iter()
            .find(|(k, _)| k == "content-transfer-encoding")
            .map(|(_, v)| v.to_ascii_lowercase())
            .unwrap_or_default();
        for (k, v) in headers {
            match k.as_str() {
                "to" if draft.to.is_empty() => {
                    draft.to = normalize_addr_header(&decode_rfc2047(&v))
                }
                "cc" if draft.cc.is_empty() => {
                    draft.cc = normalize_addr_header(&decode_rfc2047(&v))
                }
                "bcc" if draft.bcc.is_empty() => {
                    draft.bcc = normalize_addr_header(&decode_rfc2047(&v))
                }
                "subject" if draft.subject.is_empty() => draft.subject = decode_rfc2047(&v),
                _ => {}
            }
        }
        draft.body = if cte.contains("quoted-printable") {
            decode_quoted_printable(&body)
        } else {
            decode_quoted_printable(&body) // souvent du QP même sans en-tête clair
        };
        draft.to = repair_utf8_mojibake(&draft.to);
        draft.cc = repair_utf8_mojibake(&draft.cc);
        draft.bcc = repair_utf8_mojibake(&draft.bcc);
        draft.subject = repair_utf8_mojibake(&draft.subject);
        draft.body = repair_utf8_mojibake(&draft.body);
        draft
    }

    #[allow(dead_code)]
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

        let mut draft = if content.contains("\n\n") || content.contains("Subject:") {
            Self::from_mime(&content)
        } else {
            ComposeDraft::default()
        };
        if draft.to.is_empty() {
            draft.to = normalize_addr_header(&extract_addr(&v, "to"));
        }
        if draft.cc.is_empty() {
            draft.cc = normalize_addr_header(&extract_addr(&v, "cc"));
        }
        if draft.bcc.is_empty() {
            draft.bcc = normalize_addr_header(&extract_addr(&v, "bcc"));
        }
        if draft.subject.is_empty() {
            draft.subject = decode_rfc2047(
                v.get("subject")
                    .and_then(|x| x.as_str())
                    .unwrap_or(""),
            );
        }
        if draft.subject.is_empty() {
            if let Some(s) = subject_fallback {
                draft.subject = decode_rfc2047(&s);
            }
        }
        if draft.body.is_empty() && !content.is_empty() && !content.contains("Subject:") {
            draft.body = decode_quoted_printable(&content);
        }
        draft
    }
}

/// Décode les encoded-words RFC 2047 (`=?utf-8?Q?...?=` / `?B?`).
pub fn decode_rfc2047(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'?' {
            if let Some((end, decoded)) = try_decode_encoded_word(&input[i..]) {
                out.push_str(&decoded);
                i += end;
                // Espace entre encoded-words adjacent à ignorer (RFC 2047)
                while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                    let rest = &input[i..];
                    if rest.trim_start().starts_with("=?") {
                        i += rest.len() - rest.trim_start().len();
                        break;
                    }
                    out.push(bytes[i] as char);
                    i += 1;
                }
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    repair_utf8_mojibake(&out)
}

/// Répare le mojibake classique UTF-8 lu comme Latin-1 (`AurÃ©lien` → `Aurélien`).
pub fn repair_utf8_mojibake(s: &str) -> String {
    const MARKERS: &[&str] = &[
        "Ã©", "Ã¨", "Ã ", "Ã§", "Ã´", "Ã¢", "Ã®", "Ã¯", "Ã»", "Ã¹", "Ã¼", "Ã¤", "Ã¶",
        "Ã‰", "Ãˆ", "ÃŠ", "Ã‡", "â€™", "â€œ", "â€", "Ã±", "Ã¡", "Ã³",
    ];
    if !MARKERS.iter().any(|m| s.contains(m)) {
        return s.to_string();
    }
    let mut bytes = Vec::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        if u > 0xFF {
            return s.to_string();
        }
        bytes.push(u as u8);
    }
    match String::from_utf8(bytes) {
        Ok(fixed) if fixed != s => fixed,
        _ => s.to_string(),
    }
}

fn try_decode_encoded_word(s: &str) -> Option<(usize, String)> {
    // =?charset?Q|B?text?=
    if !s.starts_with("=?") {
        return None;
    }
    let rest = &s[2..];
    let (charset, rest) = rest.split_once('?')?;
    let (encoding, rest) = rest.split_once('?')?;
    let (text, _after) = rest.split_once("?=")?;
    let end = 2 + charset.len() + 1 + encoding.len() + 1 + text.len() + 2;
    let decoded = match encoding.to_ascii_uppercase().as_str() {
        "Q" => decode_q_encoding(text, charset),
        "B" => decode_b_encoding(text, charset),
        _ => text.to_string(),
    };
    Some((end, decoded))
}

fn decode_q_encoding(text: &str, charset: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let t = text.as_bytes();
    let mut i = 0;
    while i < t.len() {
        match t[i] {
            b'_' => {
                bytes.push(b' ');
                i += 1;
            }
            b'=' if i + 2 < t.len() => {
                let h = std::str::from_utf8(&t[i + 1..i + 3]).ok();
                if let Some(h) = h {
                    if let Ok(v) = u8::from_str_radix(h, 16) {
                        bytes.push(v);
                        i += 3;
                        continue;
                    }
                }
                bytes.push(t[i]);
                i += 1;
            }
            c => {
                bytes.push(c);
                i += 1;
            }
        }
    }
    charset_bytes_to_string(&bytes, charset)
}

fn decode_b_encoding(text: &str, charset: &str) -> String {
    let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    match base64_decode(&cleaned) {
        Some(bytes) => charset_bytes_to_string(&bytes, charset),
        None => text.to_string(),
    }
}

fn charset_bytes_to_string(bytes: &[u8], charset: &str) -> String {
    let cs = charset.to_ascii_lowercase();
    let raw = if cs.contains("utf-8") || cs == "utf8" {
        String::from_utf8_lossy(bytes).into_owned()
    } else if cs.contains("iso-8859-1") || cs.contains("latin1") || cs == "latin-1" || cs.contains("windows-1252") {
        let as_latin: String = bytes.iter().map(|&b| b as char).collect();
        // Souvent du vrai UTF-8 mal étiqueté Latin-1 → AurÃ©lien
        let repaired = repair_utf8_mojibake(&as_latin);
        if repaired != as_latin {
            repaired
        } else if let Ok(utf8) = std::str::from_utf8(bytes) {
            utf8.to_string()
        } else {
            as_latin
        }
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    repair_utf8_mojibake(&raw)
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const T: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [255u8; 256];
    for (i, &c) in T.iter().enumerate() {
        table[c as usize] = i as u8;
    }
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for b in input.bytes() {
        if b == b'=' {
            break;
        }
        let v = table[b as usize];
        if v == 255 {
            continue;
        }
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Décode quoted-printable (`=C3=A9`, soft breaks `=\n`).
pub fn decode_quoted_printable(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            if i + 1 < bytes.len() && (bytes[i + 1] == b'\n') {
                i += 2;
                continue;
            }
            if i + 2 < bytes.len() && bytes[i + 1] == b'\r' && bytes[i + 2] == b'\n' {
                i += 3;
                continue;
            }
            if i + 2 < bytes.len() {
                let h = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(h) = h {
                    if let Ok(v) = u8::from_str_radix(h, 16) {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Normalise `Name <email>` / `"Name" <email>` / email nu → forme SMTP utilisable.
pub fn normalize_addr_header(raw: &str) -> String {
    let raw = repair_utf8_mojibake(&decode_rfc2047(raw.trim()));
    match smtp_address_list(&raw) {
        Ok(s) => s,
        Err(_) => raw,
    }
}

/// Liste d'adresses SMTP (virgules). Refuse les noms sans `@`.
pub fn smtp_address_list(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("destinataire vide".into());
    }
    let mut out = Vec::new();
    for part in split_addr_list(raw) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some(email) = extract_email_addr(part) else {
            return Err(format!(
                "adresse invalide (email manquant) : « {part} ». Utilisez name@domaine ou Nom <name@domaine>."
            ));
        };
        let name = display_name_before_email(part, &email);
        if name.is_empty() {
            out.push(email);
        } else {
            out.push(format!("{name} <{email}>"));
        }
    }
    if out.is_empty() {
        return Err("aucun destinataire".into());
    }
    Ok(out.join(", "))
}

fn split_addr_list(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_quotes = false;
    for ch in raw.chars() {
        match ch {
            '"' if depth == 0 => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            '<' if !in_quotes => {
                depth += 1;
                cur.push(ch);
            }
            '>' if !in_quotes => {
                depth = (depth - 1).max(0);
                cur.push(ch);
            }
            ',' if !in_quotes && depth == 0 => {
                parts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

fn extract_email_addr(s: &str) -> Option<String> {
    if let Some(start) = s.rfind('<') {
        if let Some(end) = s[start + 1..].find('>') {
            let inner = s[start + 1..start + 1 + end].trim();
            if looks_like_email(inner) {
                return Some(inner.to_string());
            }
        }
    }
    let lower = s.trim().to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("mailto:") {
        let rest = rest.split('&').next().unwrap_or(rest).trim();
        if looks_like_email(rest) {
            return Some(rest.to_string());
        }
    }
    for tok in s.split_whitespace() {
        let t = tok.trim_matches(|c: char| {
            !(c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-' ))
        });
        if looks_like_email(t) {
            return Some(t.to_string());
        }
    }
    let t = s.trim();
    if looks_like_email(t) {
        return Some(t.to_string());
    }
    None
}

fn looks_like_email(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 3 || s.contains(' ') {
        return false;
    }
    let mut parts = s.split('@');
    let Some(local) = parts.next() else {
        return false;
    };
    let Some(domain) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && !local.is_empty()
        && domain.contains('.')
        && domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn display_name_before_email(raw: &str, email: &str) -> String {
    let mut name = raw.to_string();
    if let Some(idx) = name.find('<') {
        name = name[..idx].to_string();
    } else {
        name = name.replace(email, "");
    }
    name = name
        .trim()
        .trim_matches('"')
        .trim()
        .trim_matches(',')
        .trim()
        .to_string();
    name
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

fn parse_id_list(v: Option<&Value>) -> Vec<String> {
    let Some(v) = v else {
        return vec![];
    };
    let normalize = |s: &str| {
        s.trim()
            .trim_matches(|c| c == '<' || c == '>')
            .trim()
            .to_string()
    };
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(normalize))
            .filter(|s| !s.is_empty())
            .collect(),
        Value::String(s) => s
            .split_whitespace()
            .map(normalize)
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec![],
    }
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
