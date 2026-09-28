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
}

#[derive(Debug, Clone)]
pub struct CalendulaAccount {
    pub name: String,
    pub is_default: bool,
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
                is_default: item
                    .get("default")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
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
                }
            })
            .collect()
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
