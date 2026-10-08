use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

use crate::cli::cardamum::ContactRecord;
use crate::cli::himalaya::{Envelope, Mailbox, MessageView};

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct Cache {
    conn: Connection,
}

impl Cache {
    pub fn open(path: &Path) -> Result<Self, CacheError> {
        let conn = Connection::open(path)?;
        let cache = Self { conn };
        cache.migrate()?;
        Ok(cache)
    }

    fn migrate(&self) -> Result<(), CacheError> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS mailboxes (
                name TEXT PRIMARY KEY,
                desc TEXT,
                unread INTEGER NOT NULL DEFAULT 0,
                synced_at TEXT
            );
            CREATE TABLE IF NOT EXISTS messages (
                mailbox TEXT NOT NULL,
                id TEXT NOT NULL,
                subject TEXT NOT NULL,
                sender TEXT NOT NULL,
                recipients TEXT NOT NULL,
                cc TEXT NOT NULL,
                date TEXT NOT NULL,
                flags TEXT NOT NULL,
                body_html TEXT NOT NULL,
                body_text TEXT NOT NULL,
                attachments_json TEXT NOT NULL,
                PRIMARY KEY (mailbox, id)
            );
            CREATE TABLE IF NOT EXISTS contacts (
                email TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cal_calendars (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                account TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS cal_events (
                calendar_id TEXT NOT NULL,
                id TEXT NOT NULL,
                summary TEXT NOT NULL,
                start_raw TEXT NOT NULL,
                end_raw TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                location TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (calendar_id, id)
            );
            CREATE INDEX IF NOT EXISTS idx_cal_events_start ON cal_events(start_raw);
            "#,
        )?;
        self.migrate_envelopes()?;
        self.migrate_cal_events()?;
        self.migrate_contacts_photos()?;
        Ok(())
    }

    fn migrate_contacts_photos(&self) -> Result<(), CacheError> {
        let cols: Vec<String> = {
            let mut stmt = self.conn.prepare("PRAGMA table_info(contacts)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            rows.filter_map(Result::ok).collect()
        };
        let add = |name: &str, decl: &str| -> Result<(), CacheError> {
            if !cols.iter().any(|c| c == name) {
                self.conn
                    .execute(&format!("ALTER TABLE contacts ADD COLUMN {name} {decl}"), [])?;
            }
            Ok(())
        };
        add("card_id", "TEXT NOT NULL DEFAULT ''")?;
        add("book_ref", "TEXT NOT NULL DEFAULT ''")?;
        add("etag", "TEXT NOT NULL DEFAULT ''")?;
        // 0 = inconnu, 1 = photo dispo, -1 = pas de PHOTO
        add("has_photo", "INTEGER NOT NULL DEFAULT 0")?;
        add("photo_ext", "TEXT NOT NULL DEFAULT ''")?;
        Ok(())
    }

    fn migrate_cal_events(&self) -> Result<(), CacheError> {
        let cols: Vec<String> = {
            let mut stmt = self.conn.prepare("PRAGMA table_info(cal_events)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            rows.filter_map(Result::ok).collect()
        };
        if !cols.iter().any(|c| c == "location") {
            self.conn
                .execute(
                    "ALTER TABLE cal_events ADD COLUMN location TEXT NOT NULL DEFAULT ''",
                    [],
                )?;
        }
        if !cols.iter().any(|c| c == "rrule") {
            self.conn.execute(
                "ALTER TABLE cal_events ADD COLUMN rrule TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        Ok(())
    }

    /// Les enveloppes sont toujours re-téléchargeables : plutôt que de migrer
    /// colonne par colonne, on repart d'une table neuve quand le schéma change.
    fn migrate_envelopes(&self) -> Result<(), CacheError> {
        const SCHEMA: &str = "2";
        let current = self.get_meta("envelopes_schema")?;
        let outdated = current.as_deref() != Some(SCHEMA);
        if outdated {
            self.conn.execute_batch("DROP TABLE IF EXISTS envelopes;")?;
        }
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS envelopes (
                account TEXT NOT NULL DEFAULT '',
                mailbox TEXT NOT NULL,
                id TEXT NOT NULL,
                message_id TEXT NOT NULL DEFAULT '',
                flags TEXT NOT NULL,
                subject TEXT NOT NULL,
                sender TEXT NOT NULL,
                recipient TEXT NOT NULL DEFAULT '',
                date TEXT NOT NULL,
                has_attachment INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account, mailbox, id)
            );
            "#,
        )?;
        if outdated {
            self.set_meta("envelopes_schema", SCHEMA)?;
        }
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>, CacheError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), CacheError> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }


    pub fn save_mailboxes(&self, boxes: &[Mailbox]) -> Result<(), CacheError> {
        let now = chrono::Utc::now().to_rfc3339();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM mailboxes", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO mailboxes(name, desc, unread, synced_at) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for m in boxes {
                stmt.execute(params![
                    m.name,
                    m.desc,
                    m.unread.unwrap_or(0) as i64,
                    now
                ])?;
            }
        }
        tx.commit()?;
        self.set_meta("mailboxes_synced_at", &now)?;
        Ok(())
    }

    pub fn load_mailboxes(&self) -> Result<Vec<Mailbox>, CacheError> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, desc, unread FROM mailboxes ORDER BY name")?;
        let rows = stmt.query_map([], |r| {
            Ok(Mailbox {
                name: r.get(0)?,
                desc: r.get(1)?,
                unread: Some(r.get::<_, i64>(2)? as u64),
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// `account` vide = compte Himalaya par défaut (pas de `--account`).
    pub fn save_envelopes(
        &self,
        account: &str,
        mailbox: &str,
        envelopes: &[Envelope],
    ) -> Result<(), CacheError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM envelopes WHERE account = ?1 AND mailbox = ?2",
            params![account, mailbox],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO envelopes(account, mailbox, id, message_id, flags, subject, sender,
                                       recipient, date, has_attachment)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for e in envelopes {
                let flags = e.flags.join(",");
                stmt.execute(params![
                    account,
                    mailbox,
                    e.id,
                    e.message_id,
                    flags,
                    e.subject,
                    e.from,
                    e.to,
                    e.date,
                    e.has_attachment as i64
                ])?;
            }
        }
        tx.commit()?;
        let now = chrono::Utc::now().to_rfc3339();
        self.set_meta(&format!("envelopes:{account}\u{1}{mailbox}"), &now)?;
        Ok(())
    }

    /// Ajoute un flag IMAP à une enveloppe en cache (ex. `answered` après reply).
    pub fn add_envelope_flag(
        &self,
        account: &str,
        mailbox: &str,
        id: &str,
        flag: &str,
    ) -> Result<(), CacheError> {
        let flag = flag.trim().to_ascii_lowercase();
        if flag.is_empty() {
            return Ok(());
        }
        let flags: Option<String> = self.conn.query_row(
            "SELECT flags FROM envelopes WHERE account = ?1 AND mailbox = ?2 AND id = ?3",
            params![account, mailbox, id],
            |r| r.get(0),
        ).optional()?;
        let Some(flags) = flags else {
            return Ok(());
        };
        let mut parts: Vec<String> = if flags.is_empty() {
            Vec::new()
        } else {
            flags
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };
        if parts.iter().any(|p| p.eq_ignore_ascii_case(&flag)) {
            return Ok(());
        }
        parts.push(flag);
        self.conn.execute(
            "UPDATE envelopes SET flags = ?1 WHERE account = ?2 AND mailbox = ?3 AND id = ?4",
            params![parts.join(","), account, mailbox, id],
        )?;
        Ok(())
    }

    pub fn load_envelopes(
        &self,
        account: &str,
        mailbox: &str,
    ) -> Result<Vec<Envelope>, CacheError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, flags, subject, sender, date, has_attachment, message_id, recipient
             FROM envelopes WHERE account = ?1 AND mailbox = ?2 ORDER BY date DESC",
        )?;
        let rows = stmt.query_map(params![account, mailbox], |r| {
            let flags: String = r.get(1)?;
            Ok(Envelope {
                id: r.get(0)?,
                flags: if flags.is_empty() {
                    vec![]
                } else {
                    flags.split(',').map(str::to_string).collect()
                },
                subject: r.get(2)?,
                from: r.get(3)?,
                to: r.get(7)?,
                date: r.get(4)?,
                has_attachment: r.get::<_, i64>(5)? != 0,
                message_id: r.get(6)?,
                in_reply_to: vec![],
                references: vec![],
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn save_message(&self, mailbox: &str, msg: &MessageView) -> Result<(), CacheError> {
        let flags = msg.flags.join(",");
        let attachments =
            serde_json::to_string(&msg.attachments).unwrap_or_else(|_| "[]".into());
        self.conn.execute(
            "INSERT INTO messages(mailbox, id, subject, sender, recipients, cc, date, flags, body_html, body_text, attachments_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(mailbox, id) DO UPDATE SET
               subject=excluded.subject, sender=excluded.sender, recipients=excluded.recipients,
               cc=excluded.cc, date=excluded.date, flags=excluded.flags,
               body_html=excluded.body_html, body_text=excluded.body_text,
               attachments_json=excluded.attachments_json",
            params![
                mailbox,
                msg.id,
                msg.subject,
                msg.from,
                msg.to,
                msg.cc,
                msg.date,
                flags,
                msg.body_html,
                msg.body_text,
                attachments
            ],
        )?;
        Ok(())
    }

    pub fn load_message(&self, mailbox: &str, id: &str) -> Result<Option<MessageView>, CacheError> {
        let row = self
            .conn
            .query_row(
                "SELECT subject, sender, recipients, cc, date, flags, body_html, body_text, attachments_json
                 FROM messages WHERE mailbox = ?1 AND id = ?2",
                params![mailbox, id],
                |r| {
                    let flags: String = r.get(5)?;
                    let attachments_json: String = r.get(8)?;
                    let attachments = serde_json::from_str(&attachments_json).unwrap_or_default();
                    Ok(MessageView {
                        id: id.to_string(),
                        subject: r.get(0)?,
                        from: r.get(1)?,
                        to: r.get(2)?,
                        cc: r.get(3)?,
                        reply_to: String::new(),
                        date: r.get(4)?,
                        flags: if flags.is_empty() {
                            vec![]
                        } else {
                            flags.split(',').map(str::to_string).collect()
                        },
                        body_html: r.get(6)?,
                        body_text: r.get(7)?,
                        attachments,
                        raw_preview: String::new(),
                        cid_map: vec![],
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn save_contacts(
        &self,
        contacts: &[(String, String)],
    ) -> Result<(), CacheError> {
        let records: Vec<ContactRecord> = contacts
            .iter()
            .filter(|(e, _)| !e.is_empty())
            .map(|(email, name)| ContactRecord {
                email: email.trim().to_ascii_lowercase(),
                name: name.clone(),
                card_id: String::new(),
                book_ref: String::new(),
                etag: String::new(),
            })
            .collect();
        self.save_contact_records(&records)
    }

    /// Remplace le carnet en préservant `has_photo` quand l’etag est inchangé.
    pub fn save_contact_records(&self, contacts: &[ContactRecord]) -> Result<(), CacheError> {
        let prev: std::collections::HashMap<String, (String, i64, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT lower(email), etag, has_photo, photo_ext FROM contacts",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    (
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                    ),
                ))
            })?;
            rows.filter_map(Result::ok).collect()
        };

        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM contacts", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO contacts(email, name, card_id, book_ref, etag, has_photo, photo_ext)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for c in contacts {
                if c.email.is_empty() {
                    continue;
                }
                let email = c.email.trim().to_ascii_lowercase();
                // Conserver une photo déjà résolue même si l’etag liste ≠ etag card read
                // (sinon les avatars mail disparaissent à chaque refresh carnet).
                let (has_photo, photo_ext) = match prev.get(&email) {
                    Some((_, hp, ext)) if *hp == 1 && !ext.is_empty() => (*hp, ext.clone()),
                    Some((old_etag, hp, ext)) if *old_etag == c.etag && !c.etag.is_empty() => {
                        (*hp, ext.clone())
                    }
                    Some((_, hp, ext)) if c.etag.is_empty() && c.card_id.is_empty() => {
                        (*hp, ext.clone())
                    }
                    Some((_, hp, ext)) if *hp == -1 => (*hp, ext.clone()),
                    _ => (0_i64, String::new()),
                };
                stmt.execute(params![
                    email,
                    c.name,
                    c.card_id,
                    c.book_ref,
                    c.etag,
                    has_photo,
                    photo_ext
                ])?;
            }
        }
        tx.commit()?;
        let _ = self.set_meta("contacts_synced_at", &chrono::Utc::now().to_rfc3339());
        Ok(())
    }

    /// Fusionne sans vider (pour warm / suggest).
    pub fn merge_contacts(&self, contacts: &[(String, String)]) -> Result<(), CacheError> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO contacts(email, name) VALUES (?1, ?2)
             ON CONFLICT(email) DO UPDATE SET name = excluded.name",
        )?;
        for (email, name) in contacts {
            if email.is_empty() {
                continue;
            }
            stmt.execute(params![email.trim().to_ascii_lowercase(), name])?;
        }
        Ok(())
    }

    /// Emails (lowercase) → `(has_photo, photo_ext, etag)` pour un batch d’adresses.
    pub fn photo_meta_for_emails(
        &self,
        emails: &[String],
    ) -> Result<std::collections::HashMap<String, (i64, String, String)>, CacheError> {
        let mut out = std::collections::HashMap::new();
        if emails.is_empty() {
            return Ok(out);
        }
        // Chunk pour rester sous la limite SQLite des variables liées
        for chunk in emails.chunks(80) {
            let placeholders: String = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT lower(email), has_photo, photo_ext, etag FROM contacts
                 WHERE lower(email) IN ({placeholders})"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let params_owned: Vec<String> = chunk
                .iter()
                .map(|e| e.trim().to_ascii_lowercase())
                .collect();
            let params_ref: Vec<&dyn rusqlite::ToSql> = params_owned
                .iter()
                .map(|s| s as &dyn rusqlite::ToSql)
                .collect();
            let rows = stmt.query_map(params_ref.as_slice(), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    (
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ),
                ))
            })?;
            for row in rows.flatten() {
                out.insert(row.0, row.1);
            }
        }
        Ok(out)
    }

    /// Contacts à lire pour une photo (`has_photo = 0`, card_id non vide).
    pub fn contacts_pending_photo(
        &self,
        limit: i64,
    ) -> Result<Vec<(String, String, String, String)>, CacheError> {
        // (email, card_id, book_ref, etag)
        let mut stmt = self.conn.prepare(
            "SELECT lower(email), card_id, book_ref, etag FROM contacts
             WHERE has_photo = 0 AND card_id != '' AND book_ref != ''
             ORDER BY lower(email)
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Contacts à résoudre pour une photo.
    ///
    /// - `emails` non vide : **uniquement** ces adresses (priorité mail), sans remplir
    ///   avec le reste du carnet (évite de noyer les expéditeurs visibles).
    /// - `emails` vide : file générale (warm), limitée à `limit`.
    pub fn contacts_pending_photo_for(
        &self,
        emails: &[String],
        limit: i64,
    ) -> Result<Vec<(String, String, String, String)>, CacheError> {
        if emails.is_empty() {
            return self.contacts_pending_photo(limit);
        }
        let mut out = Vec::new();
        for chunk in emails.chunks(80) {
            if out.len() as i64 >= limit {
                break;
            }
            let placeholders: String = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT lower(email), card_id, book_ref, etag FROM contacts
                 WHERE has_photo = 0 AND card_id != '' AND book_ref != ''
                   AND lower(email) IN ({placeholders})"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let params_owned: Vec<String> = chunk
                .iter()
                .map(|e| e.trim().to_ascii_lowercase())
                .collect();
            let params_ref: Vec<&dyn rusqlite::ToSql> = params_owned
                .iter()
                .map(|s| s as &dyn rusqlite::ToSql)
                .collect();
            let rows = stmt.query_map(params_ref.as_slice(), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
            for row in rows.flatten() {
                out.push(row);
                if out.len() as i64 >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// true si au moins un email prioritaire est absent du carnet ou sans card_id.
    pub fn contacts_need_refresh_for_photos(&self, emails: &[String]) -> Result<bool, CacheError> {
        for email in emails {
            let email = email.trim();
            if email.is_empty() || !email.contains('@') {
                continue;
            }
            match self.contact_by_email(email)? {
                None => return Ok(true),
                Some(c) if c.card_id.is_empty() || c.book_ref.is_empty() => return Ok(true),
                Some(_) => {}
            }
        }
        Ok(false)
    }

    /// Tous les emails locaux pointant vers la même fiche CardDAV.
    pub fn emails_for_card(
        &self,
        card_id: &str,
        book_ref: &str,
    ) -> Result<Vec<String>, CacheError> {
        let mut stmt = self.conn.prepare(
            "SELECT lower(email) FROM contacts
             WHERE card_id = ?1 AND book_ref = ?2 AND email != ''",
        )?;
        let rows = stmt.query_map(params![card_id, book_ref], |r| r.get(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn set_contact_photo(
        &self,
        email: &str,
        has_photo: i64,
        photo_ext: &str,
        etag: &str,
    ) -> Result<(), CacheError> {
        self.conn.execute(
            "UPDATE contacts SET has_photo = ?1, photo_ext = ?2, etag = CASE WHEN ?3 = '' THEN etag ELSE ?3 END
             WHERE lower(email) = lower(?4)",
            params![has_photo, photo_ext, etag, email.trim()],
        )?;
        Ok(())
    }

    /// Retrouve card_id / book_ref / name / etag pour un email (édition).
    pub fn contact_by_email(
        &self,
        email: &str,
    ) -> Result<Option<ContactRecord>, CacheError> {
        let row = self
            .conn
            .query_row(
                "SELECT email, name, card_id, book_ref, etag FROM contacts
                 WHERE lower(email) = lower(?1)
                 LIMIT 1",
                params![email.trim()],
                |r| {
                    Ok(ContactRecord {
                        email: r.get(0)?,
                        name: r.get(1)?,
                        card_id: r.get(2)?,
                        book_ref: r.get(3)?,
                        etag: r.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn contact_photo_file(
        &self,
        email: &str,
    ) -> Result<Option<(String, String)>, CacheError> {
        // (ext, etag) si has_photo = 1
        let row = self
            .conn
            .query_row(
                "SELECT photo_ext, etag FROM contacts
                 WHERE lower(email) = lower(?1) AND has_photo = 1",
                params![email.trim()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    pub fn contacts_count(&self) -> Result<i64, CacheError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM contacts", [], |r| r.get(0))?;
        Ok(n)
    }

    pub fn suggest_contacts(&self, query: &str) -> Result<Vec<(String, String)>, CacheError> {
        let q = format!("%{}%", query.to_ascii_lowercase());
        let mut stmt = self.conn.prepare(
            "SELECT email, name FROM contacts
             WHERE lower(email) LIKE ?1 OR lower(name) LIKE ?1
             LIMIT 12",
        )?;
        let rows = stmt.query_map(params![q], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    #[allow(dead_code)]
    pub fn list_contacts(&self, query: &str, limit: i64) -> Result<Vec<(String, String)>, CacheError> {
        Ok(self
            .list_contacts_full(query, limit)?
            .into_iter()
            .map(|c| (c.email, c.name))
            .collect())
    }

    /// Liste enrichie (card_id / book_ref) pour l’édition depuis le cache.
    pub fn list_contacts_full(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<ContactRecord>, CacheError> {
        let q = format!("%{}%", query.to_ascii_lowercase());
        let mut stmt = self.conn.prepare(
            "SELECT email, name, card_id, book_ref, etag FROM contacts
             WHERE lower(email) LIKE ?1 OR lower(name) LIKE ?1
             ORDER BY lower(name), lower(email)
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![q, limit], |r| {
            Ok(ContactRecord {
                email: r.get(0)?,
                name: r.get(1)?,
                card_id: r.get(2)?,
                book_ref: r.get(3)?,
                etag: r.get(4)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn save_calendars(&self, cals: &[(String, String, String)]) -> Result<(), CacheError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM cal_calendars", [])?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO cal_calendars(id, name, account) VALUES (?1, ?2, ?3)")?;
            for (id, name, account) in cals {
                stmt.execute(params![id, name, account])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_calendars(&self) -> Result<Vec<(String, String, String)>, CacheError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, account FROM cal_calendars ORDER BY name")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    }


    /// Remplace uniquement les événements du mois indiqué (préserve les autres mois en cache).
    /// Tuple: (id, summary, start, end, description, location, rrule)
    pub fn replace_calendar_events_in_month(
        &self,
        calendar_id: &str,
        year: i32,
        month: u32,
        events: &[(String, String, String, String, String, String, String)],
    ) -> Result<(), CacheError> {
        let prefix_compact = format!("{year:04}{month:02}");
        let prefix_dash = format!("{year:04}-{month:02}");
        let like_c = format!("{prefix_compact}%");
        let like_d = format!("{prefix_dash}%");
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM cal_events WHERE calendar_id = ?1
             AND (start_raw LIKE ?2 OR start_raw LIKE ?3)",
            params![calendar_id, like_c, like_d],
        )?;
        {
            let mut del = tx.prepare(
                "DELETE FROM cal_events WHERE calendar_id = ?1 AND id = ?2",
            )?;
            for (id, _, _, _, _, _, _) in events {
                del.execute(params![calendar_id, id])?;
            }
        }
        {
            let mut stmt = tx.prepare(
                "INSERT INTO cal_events(calendar_id, id, summary, start_raw, end_raw, description, location, rrule)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for (id, summary, start, end, desc, loc, rrule) in events {
                stmt.execute(params![calendar_id, id, summary, start, end, desc, loc, rrule])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Vue mois : événements du mois **+** masters récurrents (DTSTART hors mois, RRULE non vide).
    /// Retourne (calendar_id, id, summary, start, end, description, location, rrule).
    pub fn load_events_for_month_view(
        &self,
        calendar_ids: &[String],
        year: i32,
        month: u32,
    ) -> Result<Vec<(String, String, String, String, String, String, String, String)>, CacheError>
    {
        self.load_events_in_month_inner(calendar_ids, year, month, true)
    }

    fn load_events_in_month_inner(
        &self,
        calendar_ids: &[String],
        year: i32,
        month: u32,
        include_recurring_masters: bool,
    ) -> Result<Vec<(String, String, String, String, String, String, String, String)>, CacheError>
    {
        if calendar_ids.is_empty() {
            return Ok(vec![]);
        }
        let prefix_compact = format!("{year:04}{month:02}");
        let prefix_dash = format!("{year:04}-{month:02}");
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::<(String, String)>::new();
        for cid in calendar_ids {
            let mut stmt = self.conn.prepare(
                "SELECT calendar_id, id, summary, start_raw, end_raw, description, location, COALESCE(rrule, '')
                 FROM cal_events
                 WHERE calendar_id = ?1
                   AND (start_raw LIKE ?2 OR start_raw LIKE ?3)
                 ORDER BY start_raw",
            )?;
            let like_c = format!("{prefix_compact}%");
            let like_d = format!("{prefix_dash}%");
            let rows = stmt.query_map(params![cid, like_c, like_d], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6).unwrap_or_default(),
                    r.get::<_, String>(7).unwrap_or_default(),
                ))
            })?;
            for row in rows.filter_map(Result::ok) {
                seen.insert((row.0.clone(), row.1.clone()));
                out.push(row);
            }
            if include_recurring_masters {
                let mut stmt = self.conn.prepare(
                    "SELECT calendar_id, id, summary, start_raw, end_raw, description, location, COALESCE(rrule, '')
                     FROM cal_events
                     WHERE calendar_id = ?1
                       AND TRIM(COALESCE(rrule, '')) != ''
                       AND TRIM(COALESCE(rrule, '')) != 'none'",
                )?;
                let rows = stmt.query_map(params![cid], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6).unwrap_or_default(),
                        r.get::<_, String>(7).unwrap_or_default(),
                    ))
                })?;
                for row in rows.filter_map(Result::ok) {
                    if seen.insert((row.0.clone(), row.1.clone())) {
                        out.push(row);
                    }
                }
            }
        }
        Ok(out)
    }

    /// RRULE déjà en cache pour préserver l’enrichissement si un warm sans enrich réécrit le mois.
    pub fn load_rrules_for_calendar(
        &self,
        calendar_id: &str,
    ) -> Result<std::collections::HashMap<String, String>, CacheError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, COALESCE(rrule, '') FROM cal_events
             WHERE calendar_id = ?1 AND TRIM(COALESCE(rrule, '')) != ''",
        )?;
        let rows = stmt.query_map(params![calendar_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn mark_month_synced(&self, year: i32, month: u32) -> Result<(), CacheError> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS cal_month_sync (
                year INTEGER NOT NULL,
                month INTEGER NOT NULL,
                synced_at TEXT NOT NULL,
                PRIMARY KEY (year, month)
             )",
            [],
        )?;
        let now = chrono::Local::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO cal_month_sync(year, month, synced_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(year, month) DO UPDATE SET synced_at = excluded.synced_at",
            params![year, month as i64, now],
        )?;
        Ok(())
    }

    pub fn is_month_synced(&self, year: i32, month: u32) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM cal_month_sync WHERE year = ?1 AND month = ?2",
                params![year, month as i64],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// Prochains événements (start_raw >= aujourd'hui, limite N).
    pub fn load_upcoming_events(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, String, String, String)>, CacheError> {
        let now = chrono::Local::now().format("%Y-%m-%d").to_string();
        let now_compact = chrono::Local::now().format("%Y%m%d").to_string();
        // Pas de LIMIT avant filtre : sinon les vieux événements (anniversaires…)
        // saturent la fenêtre et masquent le mois suivant.
        let mut stmt = self.conn.prepare(
            "SELECT id, summary, start_raw, calendar_id, location FROM cal_events
             ORDER BY start_raw ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4).unwrap_or_default(),
            ))
        })?;
        let mut out = Vec::new();
        for row in rows.flatten() {
            let start = &row.2;
            let ok = if start.len() >= 10 && start.as_bytes().get(4) == Some(&b'-') {
                start[..10] >= *now.as_str()
            } else if start.len() >= 8 && start.chars().take(8).all(|c| c.is_ascii_digit()) {
                start[..8] >= *now_compact.as_str()
            } else {
                false
            };
            if ok {
                out.push(row);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }
}
