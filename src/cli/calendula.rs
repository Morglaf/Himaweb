use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::runner::{CliResult, CliRunner};

#[derive(Clone)]
pub struct CalendulaClient {
    bin: String,
    runner: CliRunner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarInfo {
    pub id: String,
    pub name: String,
    pub account: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub summary: String,
    pub date: String,
    pub end: String,
    pub description: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub rrule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarTodo {
    pub id: String,
    pub summary: String,
    pub due: String,
    pub status: String,
    pub percent_complete: u8,
    pub calendar_id: String,
}

#[derive(Debug, Clone)]
pub struct CalendulaAccount {
    pub name: String,
}

impl CalendulaClient {
    pub fn new(bin: String, runner: CliRunner) -> Self {
        Self { bin, runner }
    }

    pub async fn list_accounts(&self) -> CliResult<Vec<CalendulaAccount>> {
        let v = self.runner.run_json(&self.bin, &["account", "list"]).await?;
        Ok(Self::parse_accounts(v))
    }

    fn parse_accounts(v: Value) -> Vec<CalendulaAccount> {
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
            .map(|item| CalendulaAccount {
                name: item
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("default")
                    .to_string(),
            })
            .collect()
    }

    pub async fn list_calendars(&self) -> CliResult<Vec<CalendarInfo>> {
        let accounts = self.list_accounts().await.unwrap_or_default();
        if accounts.is_empty() {
            return self.list_calendars_for(None).await;
        }
        let mut out = Vec::new();
        let mut last_err = None;
        for acc in &accounts {
            match self.list_calendars_for(Some(&acc.name)).await {
                Ok(list) => {
                    for mut c in list {
                        if c.account.is_empty() {
                            c.account = acc.name.clone();
                        }
                        // Prefix id so we can round-trip account+calendar
                        if !c.id.contains("::") {
                            c.id = format!("{}::{}", acc.name, c.id);
                        }
                        if !c.name.contains(&acc.name) {
                            c.name = format!("{} — {}", acc.name, c.name);
                        }
                        out.push(c);
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

    async fn list_calendars_for(&self, account: Option<&str>) -> CliResult<Vec<CalendarInfo>> {
        let attempts: &[&[&str]] = &[
            &["calendar", "list"],
            &["calendars", "list"],
            &["caldav", "list"],
        ];
        for base in attempts {
            let args = with_account(account, base);
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            if let Ok(v) = self.runner.run_json(&self.bin, &args_ref).await {
                return Ok(Self::parse_calendars(v, account.unwrap_or("")));
            }
        }
        Ok(vec![])
    }

    fn parse_calendars(v: Value, account: &str) -> Vec<CalendarInfo> {
        let arr = match v {
            Value::Array(a) => a,
            Value::Object(o) => o
                .get("calendars")
                .or_else(|| o.get("items"))
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => vec![],
        };
        arr.into_iter()
            .map(|item| CalendarInfo {
                id: item
                    .get("id")
                    .or_else(|| item.get("path"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("default")
                    .to_string(),
                name: item
                    .get("name")
                    .or_else(|| item.get("displayName"))
                    .or_else(|| item.get("display-name"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("Calendrier")
                    .to_string(),
                account: account.to_string(),
                color: item
                    .get("color")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
            })
            .collect()
    }

    pub async fn list_events(
        &self,
        calendar_ref: &str,
        from: Option<&str>,
        to: Option<&str>,
    ) -> CliResult<Vec<CalendarEvent>> {
        let (account, cal_id) = split_cal_ref(calendar_ref);
        let mut args = Vec::new();
        if let Some(a) = account.as_deref() {
            args.push("--account".into());
            args.push(a.to_string());
        }
        args.extend([
            "event".into(),
            "list".into(),
            "-k".into(),
            cal_id,
            "-s".into(),
            "500".into(),
        ]);
        if let Some(f) = from {
            args.push("--from".into());
            args.push(f.to_string());
        }
        if let Some(t) = to {
            args.push("--to".into());
            args.push(t.to_string());
        }
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let v = self.runner.run_json(&self.bin, &args_ref).await?;
        Ok(Self::parse_events(v))
    }

    fn parse_events(v: Value) -> Vec<CalendarEvent> {
        let arr = match v {
            Value::Array(a) => a,
            Value::Object(o) => o
                .get("events")
                .or_else(|| o.get("items"))
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => vec![],
        };

        arr.into_iter()
            .map(|item| {
                let summary = item
                    .get("summary")
                    .or_else(|| item.get("desc"))
                    .or_else(|| item.get("description"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("(sans titre)")
                    .to_string();
                let date = item
                    .get("start")
                    .or_else(|| item.get("date"))
                    .or_else(|| item.get("dtstart"))
                    .map(|x| match x {
                        Value::String(s) => s.clone(),
                        other => other.to_string().trim_matches('"').to_string(),
                    })
                    .unwrap_or_default();
                let end = item
                    .get("end")
                    .or_else(|| item.get("dtend"))
                    .map(|x| match x {
                        Value::String(s) => s.clone(),
                        other => other.to_string().trim_matches('"').to_string(),
                    })
                    .unwrap_or_default();
                let description = item
                    .get("description")
                    .or_else(|| item.get("desc"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let location = item
                    .get("location")
                    .or_else(|| item.get("LOCATION"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let rrule = item
                    .get("rrule")
                    .or_else(|| item.get("RRULE"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let id = item
                    .get("id")
                    .map(|x| match x {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        _ => String::new(),
                    })
                    .unwrap_or_default();
                CalendarEvent {
                    id,
                    summary,
                    date,
                    end,
                    description,
                    location,
                    rrule,
                }
            })
            .collect()
    }

    /// Lit le iCal brut d’un événement et extrait description / location / rrule.
    pub async fn enrich_event_from_ical(
        &self,
        calendar_ref: &str,
        event_id: &str,
    ) -> CliResult<(String, String, String)> {
        let ical = self.read_event_ical(calendar_ref, event_id).await?;
        Ok(parse_ical_fields(&ical))
    }

    pub async fn read_event_ical(&self, calendar_ref: &str, event_id: &str) -> CliResult<String> {
        let (account, cal_id) = split_cal_ref(calendar_ref);
        let mut args = Vec::new();
        if let Some(a) = account.as_deref() {
            args.push("--account".into());
            args.push(a.to_string());
        }
        args.extend([
            "event".into(),
            "read".into(),
            "-k".into(),
            cal_id,
            event_id.to_string(),
        ]);
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let v = self.runner.run_json(&self.bin, &refs).await?;
        Ok(v.get("contents")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string())
    }

    pub async fn list_todos(
        &self,
        calendar_ref: &str,
        from: Option<&str>,
        to: Option<&str>,
    ) -> CliResult<Vec<CalendarTodo>> {
        let (account, cal_id) = split_cal_ref(calendar_ref);
        let mut args = Vec::new();
        if let Some(a) = account.as_deref() {
            args.push("--account".into());
            args.push(a.to_string());
        }
        args.extend([
            "todo".into(),
            "list".into(),
            "-k".into(),
            cal_id.clone(),
            "-s".into(),
            "200".into(),
        ]);
        if let Some(f) = from {
            args.push("--from".into());
            args.push(f.to_string());
        }
        if let Some(t) = to {
            args.push("--to".into());
            args.push(t.to_string());
        }
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let v = self.runner.run_json(&self.bin, &args_ref).await?;
        Ok(Self::parse_todos(v, calendar_ref))
    }

    fn parse_todos(v: Value, calendar_ref: &str) -> Vec<CalendarTodo> {
        let arr = match v {
            Value::Array(a) => a,
            Value::Object(o) => o
                .get("todos")
                .or_else(|| o.get("items"))
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => vec![],
        };
        arr.into_iter()
            .map(|item| {
                let percent = item
                    .get("percentComplete")
                    .or_else(|| item.get("percent_complete"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0) as u8;
                let status = item
                    .get("status")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                CalendarTodo {
                    id: item
                        .get("id")
                        .map(|x| match x {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            _ => String::new(),
                        })
                        .unwrap_or_default(),
                    summary: item
                        .get("summary")
                        .and_then(|x| x.as_str())
                        .unwrap_or("(sans titre)")
                        .to_string(),
                    due: item
                        .get("due")
                        .map(|x| match x {
                            Value::String(s) => s.clone(),
                            other => other.to_string().trim_matches('"').to_string(),
                        })
                        .unwrap_or_default(),
                    status,
                    percent_complete: percent,
                    calendar_id: calendar_ref.to_string(),
                }
            })
            .collect()
    }

    pub async fn create_todo(&self, calendar_ref: &str, ical: &[u8]) -> CliResult<String> {
        let (account, cal_id) = split_cal_ref(calendar_ref);
        let mut args = Vec::new();
        if let Some(a) = account.as_deref() {
            args.push("--account".into());
            args.push(a.to_string());
        }
        args.extend([
            "todo".into(),
            "create".into(),
            "-k".into(),
            cal_id,
            "-".into(),
        ]);
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let v = self.runner.run_with_stdin(&self.bin, &refs, ical).await?;
        Ok(v.get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string())
    }

    pub async fn update_todo(
        &self,
        calendar_ref: &str,
        todo_id: &str,
        ical: &[u8],
    ) -> CliResult<()> {
        let (account, cal_id) = split_cal_ref(calendar_ref);
        let mut args = Vec::new();
        if let Some(a) = account.as_deref() {
            args.push("--account".into());
            args.push(a.to_string());
        }
        args.extend([
            "todo".into(),
            "update".into(),
            "-k".into(),
            cal_id,
            todo_id.to_string(),
            "-".into(),
        ]);
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.runner.run_with_stdin(&self.bin, &refs, ical).await?;
        Ok(())
    }

    pub async fn delete_todo(&self, calendar_ref: &str, todo_id: &str) -> CliResult<()> {
        let (account, cal_id) = split_cal_ref(calendar_ref);
        let mut args = Vec::new();
        if let Some(a) = account.as_deref() {
            args.push("--account".into());
            args.push(a.to_string());
        }
        args.extend([
            "todo".into(),
            "delete".into(),
            "-k".into(),
            cal_id,
            todo_id.to_string(),
        ]);
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.runner.run_json(&self.bin, &refs).await?;
        Ok(())
    }

    /// iCalendar VTODO.
    pub fn build_vtodo(summary: &str, due: &str, completed: bool) -> String {
        let uid = uuid::Uuid::new_v4();
        let dtstamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let status = if completed {
            "COMPLETED"
        } else {
            "NEEDS-ACTION"
        };
        let percent = if completed { 100 } else { 0 };
        let mut lines = vec![
            "BEGIN:VCALENDAR".into(),
            "VERSION:2.0".into(),
            "PRODID:-//HimaWeb//EN".into(),
            "BEGIN:VTODO".into(),
            format!("UID:{uid}"),
            format!("DTSTAMP:{dtstamp}"),
            format!("SUMMARY:{}", escape_ical(summary)),
            format!("STATUS:{status}"),
            format!("PERCENT-COMPLETE:{percent}"),
        ];
        if let Some(d) = ical_date_only(due) {
            lines.push(format!("DUE;VALUE=DATE:{d}"));
        }
        if completed {
            lines.push(format!("COMPLETED:{dtstamp}"));
        }
        lines.push("END:VTODO".into());
        lines.push("END:VCALENDAR".into());
        lines.join("\r\n")
    }

    /// VTODO en réutilisant un UID existant (pour update / toggle).
    pub fn build_vtodo_with_uid(uid: &str, summary: &str, due: &str, completed: bool) -> String {
        let dtstamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let status = if completed {
            "COMPLETED"
        } else {
            "NEEDS-ACTION"
        };
        let percent = if completed { 100 } else { 0 };
        let mut lines = vec![
            "BEGIN:VCALENDAR".into(),
            "VERSION:2.0".into(),
            "PRODID:-//HimaWeb//EN".into(),
            "BEGIN:VTODO".into(),
            format!("UID:{}", uid.trim()),
            format!("DTSTAMP:{dtstamp}"),
            format!("SUMMARY:{}", escape_ical(summary)),
            format!("STATUS:{status}"),
            format!("PERCENT-COMPLETE:{percent}"),
        ];
        if let Some(d) = ical_date_only(due) {
            lines.push(format!("DUE;VALUE=DATE:{d}"));
        }
        if completed {
            lines.push(format!("COMPLETED:{dtstamp}"));
        }
        lines.push("END:VTODO".into());
        lines.push("END:VCALENDAR".into());
        lines.join("\r\n")
    }


    pub async fn create_event(&self, calendar_ref: &str, ical: &[u8]) -> CliResult<String> {
        let (account, cal_id) = split_cal_ref(calendar_ref);
        let mut args = Vec::new();
        if let Some(a) = account.as_deref() {
            args.push("--account".into());
            args.push(a.to_string());
        }
        args.extend([
            "event".into(),
            "create".into(),
            "-k".into(),
            cal_id,
            "-".into(),
        ]);
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let v = self.runner.run_with_stdin(&self.bin, &refs, ical).await?;
        Ok(v
            .get("id")
            .map(|x| match x {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default())
    }

    pub async fn update_event(
        &self,
        calendar_ref: &str,
        event_id: &str,
        ical: &[u8],
        etag: Option<&str>,
    ) -> CliResult<()> {
        let (account, cal_id) = split_cal_ref(calendar_ref);
        let mut args = Vec::new();
        if let Some(a) = account.as_deref() {
            args.push("--account".into());
            args.push(a.to_string());
        }
        args.extend(["event".into(), "update".into(), "-k".into(), cal_id]);
        if let Some(e) = etag.filter(|s| !s.is_empty()) {
            args.push("--if-match".into());
            args.push(e.to_string());
        }
        args.push(event_id.to_string());
        args.push("-".into());
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.runner.run_with_stdin(&self.bin, &refs, ical).await?;
        Ok(())
    }

    pub async fn delete_event(&self, calendar_ref: &str, event_id: &str) -> CliResult<()> {
        let (account, cal_id) = split_cal_ref(calendar_ref);
        let mut args = Vec::new();
        if let Some(a) = account.as_deref() {
            args.push("--account".into());
            args.push(a.to_string());
        }
        args.extend([
            "event".into(),
            "delete".into(),
            "-k".into(),
            cal_id,
            event_id.to_string(),
        ]);
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.runner.run_json(&self.bin, &refs).await?;
        Ok(())
    }

    /// iCalendar VEVENT (timestamps locaux flottants + récurrence optionnelle).
    pub fn build_ical(
        summary: &str,
        start: &str,
        end: &str,
        description: &str,
        location: &str,
        rrule: &str,
    ) -> String {
        let uid = uuid::Uuid::new_v4();
        let dtstamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let dtstart = ical_datetime(start);
        let dtend = ical_datetime(if end.trim().is_empty() { start } else { end });
        let mut lines = vec![
            "BEGIN:VCALENDAR".into(),
            "VERSION:2.0".into(),
            "PRODID:-//HimaWeb//EN".into(),
            "BEGIN:VEVENT".into(),
            format!("UID:{uid}"),
            format!("DTSTAMP:{dtstamp}"),
            format!("DTSTART:{dtstart}"),
            format!("DTEND:{dtend}"),
            format!("SUMMARY:{}", escape_ical(summary)),
        ];
        if !description.trim().is_empty() {
            lines.push(format!("DESCRIPTION:{}", escape_ical(description.trim())));
        }
        if !location.trim().is_empty() {
            lines.push(format!("LOCATION:{}", escape_ical(location.trim())));
        }
        if let Some(rule) = normalize_rrule(rrule) {
            lines.push(format!("RRULE:{rule}"));
        }
        lines.push("END:VEVENT".into());
        lines.push("END:VCALENDAR".into());
        lines.join("\r\n")
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

fn split_cal_ref(calendar_ref: &str) -> (Option<String>, String) {
    if let Some((acc, id)) = calendar_ref.split_once("::") {
        (Some(acc.to_string()), id.to_string())
    } else {
        (None, calendar_ref.to_string())
    }
}

fn escape_ical(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

/// Extrait (description, location, rrule) depuis un iCal VEVENT.
fn parse_ical_fields(ical: &str) -> (String, String, String) {
    let mut description = String::new();
    let mut location = String::new();
    let mut rrule = String::new();
    for raw_line in ical.lines() {
        let line = raw_line.trim_end_matches('\r');
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("DESCRIPTION") {
            if let Some(v) = ical_prop_value(line) {
                description = unescape_ical(&v);
            }
        } else if upper.starts_with("LOCATION") {
            if let Some(v) = ical_prop_value(line) {
                location = unescape_ical(&v);
            }
        } else if upper.starts_with("RRULE") {
            if let Some(v) = ical_prop_value(line) {
                rrule = v;
            }
        }
    }
    (description, location, rrule)
}

fn ical_prop_value(line: &str) -> Option<String> {
    let (_, rest) = line.split_once(':')?;
    Some(rest.to_string())
}

fn unescape_ical(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

fn ical_date_only(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 8 {
        return Some(digits[..8].to_string());
    }
    if t.len() >= 10 && t.as_bytes().get(4) == Some(&b'-') {
        return Some(t[..10].replace('-', ""));
    }
    None
}

fn normalize_rrule(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") {
        return None;
    }
    let upper = t.to_ascii_uppercase();
    let rule = if upper.starts_with("FREQ=") || upper.starts_with("RRULE:") {
        upper.trim_start_matches("RRULE:").to_string()
    } else {
        match t.to_ascii_lowercase().as_str() {
            "daily" | "quotidien" => "FREQ=DAILY".into(),
            "weekly" | "hebdo" | "hebdomadaire" => "FREQ=WEEKLY".into(),
            "monthly" | "mensuel" => "FREQ=MONTHLY".into(),
            "yearly" | "annuel" => "FREQ=YEARLY".into(),
            other => format!("FREQ={}", other.to_ascii_uppercase()),
        }
    };
    Some(rule)
}

fn ical_datetime(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
    }
    if t.chars().all(|c| c.is_ascii_digit() || c == 'T' || c == 'Z') && t.len() >= 8 {
        return t.to_string();
    }
    let cleaned = t.replace(' ', "T");
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%dT%H:%M") {
        return dt.format("%Y%m%dT%H%M%S").to_string();
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%dT%H:%M:%S") {
        return dt.format("%Y%m%dT%H%M%S").to_string();
    }
    // datetime-local HTML: 2024-01-15T10:30
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y-%m-%dT%H:%M") {
        return dt.format("%Y%m%dT%H%M%S").to_string();
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return d.format("%Y%m%d").to_string();
    }
    t.replace(['-', ':', ' '], "")
}
