//! Vérification de version (GitHub Releases) — hors hot path HTMX.
//! Canal de maj recommandé : Winget / UniGet (`Morglaf.HimaWeb`).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::Mutex;

const GITHUB_LATEST: &str = "https://api.github.com/repos/Morglaf/Himaweb/releases/latest";
const WINGET_ID: &str = "Morglaf.HimaWeb";
const CACHE_TTL: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub release_url: String,
    pub winget_id: String,
    pub winget_cmd: String,
    pub checked_at: String,
    pub error: Option<String>,
}

struct Cache {
    at: Instant,
    info: UpdateInfo,
}

static CACHE: OnceLock<Mutex<Option<Cache>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<Cache>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.split('.');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next().unwrap_or("0").parse().ok()?;
    let c = parts
        .next()
        .unwrap_or("0")
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    Some((a, b, c))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => latest.trim_start_matches('v') != current.trim_start_matches('v')
            && !latest.is_empty(),
    }
}

/// Check GitHub ; résultat mis en cache (TTL). `force` ignore le cache.
pub async fn check(force: bool) -> UpdateInfo {
    let current = current_version().to_string();
    if !force {
        let guard = cache().lock().await;
        if let Some(c) = guard.as_ref() {
            if c.at.elapsed() < CACHE_TTL {
                return c.info.clone();
            }
        }
    }

    let info = match fetch_latest().await {
        Ok((tag, html_url)) => {
            let newer = is_newer(&tag, &current);
            UpdateInfo {
                current: current.clone(),
                latest: tag,
                newer,
                release_url: html_url,
                winget_id: WINGET_ID.into(),
                winget_cmd: format!("winget upgrade {WINGET_ID}"),
                checked_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                error: None,
            }
        }
        Err(e) => UpdateInfo {
            current: current.clone(),
            latest: String::new(),
            newer: false,
            release_url: "https://github.com/Morglaf/Himaweb/releases".into(),
            winget_id: WINGET_ID.into(),
            winget_cmd: format!("winget upgrade {WINGET_ID}"),
            checked_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            error: Some(e),
        },
    };

    {
        let mut guard = cache().lock().await;
        *guard = Some(Cache {
            at: Instant::now(),
            info: info.clone(),
        });
    }
    info
}

async fn fetch_latest() -> Result<(String, String), String> {
    let client = reqwest::Client::builder()
        .user_agent(format!("HimaWeb/{}", current_version()))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get(GITHUB_LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("GitHub HTTP {}", res.status()));
    }
    let v: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    let tag = v
        .get("tag_name")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if tag.is_empty() {
        return Err("tag_name manquant".into());
    }
    let url = v
        .get("html_url")
        .and_then(|x| x.as_str())
        .unwrap_or("https://github.com/Morglaf/Himaweb/releases")
        .to_string();
    Ok((tag, url))
}
