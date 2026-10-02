use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

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
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM contacts", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO contacts(email, name) VALUES (?1, ?2)
                 ON CONFLICT(email) DO UPDATE SET name = excluded.name",
            )?;
            for (email, name) in contacts {
                if email.is_empty() {
                    continue;
                }
                stmt.execute(params![email, name])?;
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
            stmt.execute(params![email, name])?;
        }
        Ok(())
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

    pub fn list_contacts(&self, query: &str, limit: i64) -> Result<Vec<(String, String)>, CacheError> {
        let q = format!("%{}%", query.to_ascii_lowercase());
        let mut stmt = self.conn.prepare(
            "SELECT email, name FROM contacts
             WHERE lower(email) LIKE ?1 OR lower(name) LIKE ?1
             ORDER BY lower(name), lower(email)
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![q, limit], |r| Ok((r.get(0)?, r.get(1)?)))?;
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
    /// Tuple: (id, summary, start, end, description, location)
    pub fn replace_calendar_events_in_month(
        &self,
        calendar_id: &str,
        year: i32,
        month: u32,
        events: &[(String, String, String, String, String, String)],
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
            for (id, _, _, _, _, _) in events {
                del.execute(params![calendar_id, id])?;
            }
        }
        {
            let mut stmt = tx.prepare(
                "INSERT INTO cal_events(calendar_id, id, summary, start_raw, end_raw, description, location)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (id, summary, start, end, desc, loc) in events {
                stmt.execute(params![calendar_id, id, summary, start, end, desc, loc])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Events dont start_raw commence par YYYYMM (compact) ou YYYY-MM
    /// Retourne (calendar_id, id, summary, start, end, description, location)
    pub fn load_events_in_month(
        &self,
        calendar_ids: &[String],
        year: i32,
        month: u32,
    ) -> Result<Vec<(String, String, String, String, String, String, String)>, CacheError> {
        if calendar_ids.is_empty() {
            return Ok(vec![]);
        }
        let prefix_compact = format!("{year:04}{month:02}");
        let prefix_dash = format!("{year:04}-{month:02}");
        let mut out = Vec::new();
        for cid in calendar_ids {
            let mut stmt = self.conn.prepare(
                "SELECT calendar_id, id, summary, start_raw, end_raw, description, location
                 FROM cal_events
                 WHERE calendar_id = ?1
                   AND (start_raw LIKE ?2 OR start_raw LIKE ?3)
                 ORDER BY start_raw",
            )?;
            let like_c = format!("{prefix_compact}%");
            let like_d = format!("{prefix_dash}%");
            let rows = stmt.query_map(params![cid, like_c, like_d], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get::<_, String>(6).unwrap_or_default(),
                ))
            })?;
            out.extend(rows.filter_map(Result::ok));
        }
        Ok(out)
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
