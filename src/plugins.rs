use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
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
        let (name, description) = if manifest_path.is_file() {
            match fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|s| toml::from_str::<Manifest>(&s).ok())
            {
                Some(m) => (
                    if m.name.is_empty() { id.clone() } else { m.name },
                    m.description,
                ),
                None => (id.clone(), String::new()),
            }
        } else {
            (id.clone(), "Sans manifeste".into())
        };
        out.push(PluginInfo {
            id,
            name,
            description,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
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
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

fn ensure_manifest(dest: &Path, id: &str) -> Result<(), String> {
    let manifest = dest.join("himaweb-plugin.toml");
    if manifest.exists() {
        return Ok(());
    }
    let body = format!(
        "name = \"{id}\"\ndescription = \"Plugin installé depuis git\"\nhooks = []\n"
    );
    fs::write(manifest, body).map_err(|e| e.to_string())
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

/// Message reçu depuis un topic ntfy (poll JSON).
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

/// Récupère les messages d’un topic (poll, NDJSON).
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
