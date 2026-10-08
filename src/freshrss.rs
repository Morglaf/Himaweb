//! Client FreshRSS via API Google Reader (`/api/greader.php`).

use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::plugins::RssItem;

const TAG_READ: &str = "user/-/state/com.google/read";
const TAG_STARRED: &str = "user/-/state/com.google/starred";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RssFilter {
    Unread,
    Starred,
    All,
}

impl RssFilter {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "starred" | "star" | "favoris" | "favorite" => Self::Starred,
            "all" | "tous" => Self::All,
            _ => Self::Unread,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FreshRssCreds {
    pub api_base: String,
    pub user: String,
    pub api_password: String,
}

struct AuthCache {
    api_base: String,
    user: String,
    auth: String,
    at: Instant,
}

static AUTH: OnceLock<Mutex<Option<AuthCache>>> = OnceLock::new();

fn auth_cache() -> &'static Mutex<Option<AuthCache>> {
    AUTH.get_or_init(|| Mutex::new(None))
}

/// Normalise l’URL saisie : accepte le host, `/api`, ou le chemin greader.php complet.
pub fn normalize_api_base(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches('/').to_string();
    if s.is_empty() {
        return s;
    }
    if !s.contains("greader.php") {
        if s.ends_with("/api") {
            s.push_str("/greader.php");
        } else {
            s.push_str("/api/greader.php");
        }
    }
    s
}

pub fn is_configured(api_base: &str, user: &str, api_password: &str) -> bool {
    !api_base.trim().is_empty() && !user.trim().is_empty() && !api_password.is_empty()
}

pub async fn fetch_items(
    creds: &FreshRssCreds,
    limit: usize,
    filter: RssFilter,
) -> Result<Vec<RssItem>, String> {
    let base = normalize_api_base(&creds.api_base);
    if !is_configured(&base, &creds.user, &creds.api_password) {
        return Err("FreshRSS non configuré".into());
    }
    let auth = login(&base, &creds.user, &creds.api_password).await?;
    let n = limit.clamp(1, 50);
    let url = match filter {
        RssFilter::Unread => format!(
            "{base}/reader/api/0/stream/contents/reading-list?output=json&n={n}&xt={TAG_READ}"
        ),
        RssFilter::Starred => format!(
            "{base}/reader/api/0/stream/contents/{TAG_STARRED}?output=json&n={n}"
        ),
        RssFilter::All => {
            format!("{base}/reader/api/0/stream/contents/reading-list?output=json&n={n}")
        }
    };
    let client = http_client()?;
    let res = auth_get(&client, &url, &auth).await?;
    if !res.status().is_success() {
        {
            let mut guard = auth_cache().lock().await;
            *guard = None;
        }
        let auth = login(&base, &creds.user, &creds.api_password).await?;
        let res = auth_get(&client, &url, &auth).await?;
        if !res.status().is_success() {
            return Err(format!("FreshRSS HTTP {}", res.status()));
        }
        return parse_stream(res.json().await.map_err(|e| e.to_string())?, n);
    }
    parse_stream(res.json().await.map_err(|e| e.to_string())?, n)
}

pub async fn set_read(creds: &FreshRssCreds, item_id: &str, read: bool) -> Result<(), String> {
    edit_tag(creds, item_id, TAG_READ, read).await
}

pub async fn set_starred(
    creds: &FreshRssCreds,
    item_id: &str,
    starred: bool,
) -> Result<(), String> {
    edit_tag(creds, item_id, TAG_STARRED, starred).await
}

pub async fn mark_all_read(creds: &FreshRssCreds) -> Result<(), String> {
    let base = normalize_api_base(&creds.api_base);
    if !is_configured(&base, &creds.user, &creds.api_password) {
        return Err("FreshRSS non configuré".into());
    }
    let auth = login(&base, &creds.user, &creds.api_password).await?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let body = format!(
        "s={}&ts={}",
        urlencoding_form("user/-/state/com.google/reading-list"),
        ts
    );
    let url = format!("{base}/reader/api/0/mark-all-as-read");
    let client = http_client()?;
    let res = client
        .post(&url)
        .header("Authorization", format!("GoogleLogin auth={auth}"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("FreshRSS mark-all-as-read HTTP {}", res.status()));
    }
    Ok(())
}

async fn edit_tag(
    creds: &FreshRssCreds,
    item_id: &str,
    tag: &str,
    add: bool,
) -> Result<(), String> {
    let id = item_id.trim();
    if id.is_empty() {
        return Err("id article manquant".into());
    }
    let base = normalize_api_base(&creds.api_base);
    if !is_configured(&base, &creds.user, &creds.api_password) {
        return Err("FreshRSS non configuré".into());
    }
    let auth = login(&base, &creds.user, &creds.api_password).await?;
    let op = if add { "a" } else { "r" };
    let body = format!(
        "i={}&{op}={}",
        urlencoding_form(id),
        urlencoding_form(tag)
    );
    let url = format!("{base}/reader/api/0/edit-tag");
    let client = http_client()?;
    let res = client
        .post(&url)
        .header("Authorization", format!("GoogleLogin auth={auth}"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("FreshRSS edit-tag HTTP {}", res.status()));
    }
    Ok(())
}

async fn auth_get(
    client: &reqwest::Client,
    url: &str,
    auth: &str,
) -> Result<reqwest::Response, String> {
    client
        .get(url)
        .header("Authorization", format!("GoogleLogin auth={auth}"))
        .send()
        .await
        .map_err(|e| e.to_string())
}

async fn login(api_base: &str, user: &str, pass: &str) -> Result<String, String> {
    {
        let guard = auth_cache().lock().await;
        if let Some(c) = guard.as_ref() {
            if c.api_base == api_base
                && c.user == user
                && c.at.elapsed() < Duration::from_secs(50 * 60)
            {
                return Ok(c.auth.clone());
            }
        }
    }
    let client = http_client()?;
    let url = format!("{api_base}/accounts/ClientLogin");
    let res = client
        .post(&url)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(format!(
            "Email={}&Passwd={}",
            urlencoding_form(user),
            urlencoding_form(pass)
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("FreshRSS login HTTP {}", res.status()));
    }
    let body = res.text().await.map_err(|e| e.to_string())?;
    let auth = body
        .lines()
        .find_map(|l| l.strip_prefix("Auth=").map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "FreshRSS: Auth manquant (vérifiez utilisateur / mot de passe API)".to_string()
        })?;
    {
        let mut guard = auth_cache().lock().await;
        *guard = Some(AuthCache {
            api_base: api_base.to_string(),
            user: user.to_string(),
            auth: auth.clone(),
            at: Instant::now(),
        });
    }
    Ok(auth)
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!("HimaWeb/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}

fn urlencoding_form(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct StreamResp {
    #[serde(default)]
    items: Vec<StreamItem>,
}

#[derive(Debug, Deserialize)]
struct StreamItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    canonical: Vec<HrefObj>,
    #[serde(default)]
    alternate: Vec<HrefObj>,
    #[serde(default)]
    origin: Option<Origin>,
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct HrefObj {
    #[serde(default)]
    href: String,
}

#[derive(Debug, Deserialize)]
struct Origin {
    #[serde(default, rename = "title")]
    title: String,
}

fn parse_stream(v: serde_json::Value, limit: usize) -> Result<Vec<RssItem>, String> {
    let parsed: StreamResp =
        serde_json::from_value(v).map_err(|e| format!("FreshRSS JSON: {e}"))?;
    let mut out = Vec::new();
    for it in parsed.items {
        let link = it
            .canonical
            .iter()
            .chain(it.alternate.iter())
            .map(|h| h.href.trim())
            .find(|h| !h.is_empty())
            .unwrap_or("")
            .to_string();
        let title = if it.title.trim().is_empty() {
            if link.is_empty() {
                continue;
            }
            link.clone()
        } else {
            it.title
        };
        let feed = it
            .origin
            .as_ref()
            .map(|o| o.title.clone())
            .unwrap_or_else(|| "FreshRSS".into());
        let unread = !it.categories.iter().any(|c| c.ends_with("/state/com.google/read"));
        let starred = it
            .categories
            .iter()
            .any(|c| c.ends_with("/state/com.google/starred"));
        out.push(RssItem {
            id: it.id,
            title,
            link,
            feed_title: feed,
            unread,
            starred,
        });
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

/// URL web « humaine » à partir de l’API (retire `/api/greader.php`).
pub fn web_ui_url(api_base: &str) -> String {
    let base = normalize_api_base(api_base);
    base.trim_end_matches("/api/greader.php")
        .trim_end_matches("/greader.php")
        .trim_end_matches("/api")
        .to_string()
}
