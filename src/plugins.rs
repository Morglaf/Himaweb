use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub hooks: Vec<PluginHook>,
}

#[derive(Debug, Clone)]
pub struct PluginHook {
    pub kind: String,
    pub path: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct PluginPanel {
    #[allow(dead_code)]
    pub plugin_id: String,
    pub title: String,
    pub html: String,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    hooks: Vec<ManifestHook>,
}

#[derive(Debug, Deserialize)]
struct ManifestHook {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Clone)]
pub struct RssItem {
    pub title: String,
    pub link: String,
    pub feed_title: String,
}

pub fn plugins_dir() -> Result<PathBuf, String> {
    let base = dirs::data_local_dir().ok_or("LOCALAPPDATA introuvable")?;
    let dir = base.join("HimaWeb").join("plugins");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn list_plugins() -> Vec<PluginInfo> {
    let Ok(dir) = plugins_dir() else {
        return vec![];
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let manifest_path = path.join("himaweb-plugin.toml");
        let (name, description, hooks) = if manifest_path.is_file() {
            match fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|s| toml::from_str::<Manifest>(&s).ok())
            {
                Some(m) => (
                    if m.name.is_empty() {
                        id.clone()
                    } else {
                        m.name
                    },
                    m.description,
                    m.hooks
                        .into_iter()
                        .filter(|h| !h.kind.is_empty())
                        .map(|h| PluginHook {
                            kind: h.kind,
                            path: h.path,
                            title: h.title,
                        })
                        .collect(),
                ),
                None => (id.clone(), String::new(), vec![]),
            }
        } else {
            (id.clone(), "Sans manifeste".into(), vec![])
        };
        out.push(PluginInfo {
            id,
            name,
            description,
            hooks,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Panneaux HTML déclarés par les plugins (`hooks.kind = "sidebar_panel"`).
pub fn sidebar_panels() -> Vec<PluginPanel> {
    let Ok(root) = plugins_dir() else {
        return vec![];
    };
    let mut out = Vec::new();
    for p in list_plugins() {
        for h in &p.hooks {
            if h.kind != "sidebar_panel" {
                continue;
            }
            let rel = h.path.trim().trim_start_matches('/').trim_start_matches('\\');
            if rel.is_empty() || rel.contains("..") {
                continue;
            }
            let file = root.join(&p.id).join(rel);
            let Ok(html) = fs::read_to_string(&file) else {
                continue;
            };
            let title = if h.title.is_empty() {
                p.name.clone()
            } else {
                h.title.clone()
            };
            out.push(PluginPanel {
                plugin_id: p.id.clone(),
                title,
                html,
            });
        }
    }
    out
}

/// Sert un fichier du plugin (assets du panneau). Refuse `..`.
pub fn plugin_file(id: &str, rel: &str) -> Result<(PathBuf, Vec<u8>), String> {
    let id = id.trim();
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        return Err("id invalide".into());
    }
    let rel = rel.trim().trim_start_matches('/').trim_start_matches('\\');
    if rel.is_empty() || rel.contains("..") {
        return Err("chemin invalide".into());
    }
    let root = plugins_dir()?;
    let path = root.join(id).join(rel);
    if !path.starts_with(&root) {
        return Err("chemin hors plugin".into());
    }
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    Ok((path, bytes))
}

/// Clone ou pull un dépôt git dans `plugins/<id>`.
pub fn install_from_git(repo: &str) -> Result<PluginInfo, String> {
    let repo = repo.trim();
    if repo.is_empty() {
        return Err("URL git vide".into());
    }
    let dir = plugins_dir()?;
    let id = repo_id(repo);
    let dest = dir.join(&id);
    if dest.exists() {
        let mut cmd = Command::new("git");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let status = cmd
            .args(["-C", dest.to_str().unwrap_or("."), "pull", "--ff-only"])
            .status()
            .map_err(|e| format!("git pull: {e}"))?;
        if !status.success() {
            return Err("git pull a échoué".into());
        }
    } else {
        let mut cmd = Command::new("git");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let status = cmd
            .args(["clone", "--depth", "1", repo, dest.to_str().unwrap_or(".")])
            .status()
            .map_err(|e| format!("git clone: {e}"))?;
        if !status.success() {
            return Err("git clone a échoué".into());
        }
        ensure_manifest(&dest, &id)?;
    }
    list_plugins()
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| "plugin installé mais introuvable".into())
}

pub fn remove_plugin(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        return Err("id plugin invalide".into());
    }
    let dest = plugins_dir()?.join(id);
    if !dest.exists() {
        return Err("plugin introuvable".into());
    }
    fs::remove_dir_all(&dest).map_err(|e| e.to_string())
}

fn repo_id(repo: &str) -> String {
    let cleaned = repo.trim_end_matches('/').trim_end_matches(".git");
    cleaned
        .rsplit('/')
        .next()
        .unwrap_or("plugin")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn ensure_manifest(dest: &Path, id: &str) -> Result<(), String> {
    let manifest = dest.join("himaweb-plugin.toml");
    if manifest.exists() {
        return Ok(());
    }
    let body = format!(
        "name = \"{id}\"\ndescription = \"Plugin installé depuis git\"\n\n# Exemple de panneau latéral :\n# [[hooks]]\n# kind = \"sidebar_panel\"\n# path = \"panel.html\"\n# title = \"Mon plugin\"\n"
    );
    fs::write(manifest, body).map_err(|e| e.to_string())
}

/// Fetch + parse minimal RSS 2.0 / Atom (title + link).
pub async fn fetch_rss(url: &str, feed_title: &str, limit: usize) -> Result<Vec<RssItem>, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("URL vide".into());
    }
    let client = reqwest::Client::builder()
        .user_agent(format!("HimaWeb/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    let text = res.text().await.map_err(|e| e.to_string())?;
    Ok(parse_feed(&text, feed_title, limit))
}

fn parse_feed(xml: &str, feed_title: &str, limit: usize) -> Vec<RssItem> {
    let mut items = Vec::new();
    for block in split_tags(xml, "item") {
        if let Some(it) = item_from_block(&block, feed_title) {
            items.push(it);
            if items.len() >= limit {
                return items;
            }
        }
    }
    if items.is_empty() {
        for block in split_tags(xml, "entry") {
            if let Some(it) = item_from_block(&block, feed_title) {
                items.push(it);
                if items.len() >= limit {
                    break;
                }
            }
        }
    }
    items
}

fn split_tags(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let lower = xml.to_ascii_lowercase();
    let open_l = open.to_ascii_lowercase();
    let close_l = close.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(i) = lower[start..].find(&open_l) {
        let abs = start + i;
        let Some(end_rel) = lower[abs..].find(&close_l) else {
            break;
        };
        let end = abs + end_rel + close.len();
        out.push(xml[abs..end].to_string());
        start = end;
    }
    out
}

fn item_from_block(block: &str, feed_title: &str) -> Option<RssItem> {
    let title = xml_text(block, "title").unwrap_or_default();
    let mut link = xml_text(block, "link").unwrap_or_default();
    if link.is_empty() {
        if let Some(h) = attr_value(block, "link", "href") {
            link = h;
        }
    }
    if title.is_empty() && link.is_empty() {
        return None;
    }
    Some(RssItem {
        title: if title.is_empty() {
            link.clone()
        } else {
            title
        },
        link,
        feed_title: feed_title.to_string(),
    })
}

fn xml_text(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let lower = block.to_ascii_lowercase();
    let open_l = open.to_ascii_lowercase();
    let close_l = close.to_ascii_lowercase();
    let i = lower.find(&open_l)?;
    let after = &block[i..];
    let gt = after.find('>')?;
    let content_start = i + gt + 1;
    let j = lower[content_start..].find(&close_l)?;
    let raw = &block[content_start..content_start + j];
    let t = raw
        .replace("<![CDATA[", "")
        .replace("]]>", "")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let t = t.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn attr_value(block: &str, tag: &str, attr: &str) -> Option<String> {
    let lower = block.to_ascii_lowercase();
    let needle = format!("<{tag}");
    let i = lower.find(&needle.to_ascii_lowercase())?;
    let slice = &block[i..];
    let end = slice.find('>').unwrap_or(slice.len().min(200));
    let tag_src = &slice[..end];
    let attr_l = attr.to_ascii_lowercase();
    let tag_l = tag_src.to_ascii_lowercase();
    let a = tag_l.find(&attr_l)?;
    let after = &tag_src[a + attr.len()..];
    let after = after.trim_start_matches([' ', '\t', '=']);
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &after[1..];
    let end_q = rest.find(quote)?;
    Some(rest[..end_q].to_string())
}

/// Envoie une notification NTFY (plugin intégré).
pub async fn ntfy_publish(server: &str, topic: &str, title: &str, body: &str) -> Result<(), String> {
    let server = server.trim_end_matches('/');
    let topic = topic.trim();
    if topic.is_empty() {
        return Err("topic NTFY vide".into());
    }
    let url = format!("{server}/{topic}");
    let client = reqwest::Client::new();
    let res = client
        .post(&url)
        .header("Title", title)
        .header("Tags", "email")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("NTFY HTTP {}", res.status()));
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NtfyMessage {
    pub id: String,
    #[serde(default)]
    pub time: i64,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn ntfy_poll(server: &str, topic: &str) -> Result<Vec<NtfyMessage>, String> {
    let server = server.trim_end_matches('/');
    let topic = topic.trim();
    if topic.is_empty() {
        return Err("topic NTFY vide".into());
    }
    let url = format!("{server}/{topic}/json?poll=1");
    let client = reqwest::Client::new();
    let res = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("NTFY HTTP {}", res.status()));
    }
    let text = res.text().await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<NtfyMessage>(line) {
            Ok(m) if m.event == "message" || m.event.is_empty() => out.push(m),
            Ok(_) => {}
            Err(e) => tracing::debug!("ntfy parse: {e} — {line}"),
        }
    }
    out.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(out)
}
