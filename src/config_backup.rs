//! Export / import ZIP de la config HimaWeb (prefs + TOML Pimalaya + Ortie).

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::accounts_config;
use crate::prefs::{self, Prefs};

const MANIFEST_NAME: &str = "himaweb-export.json";
const EXPORT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportManifest {
    pub version: u32,
    pub exported_at: String,
    pub app: String,
}

fn zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn add_file_if_exists<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    arc_name: &str,
    path: &Path,
) {
    if !path.is_file() {
        return;
    }
    let Ok(data) = std::fs::read(path) else {
        return;
    };
    if zip.start_file(arc_name, zip_options()).is_ok() {
        let _ = zip.write_all(&data);
    }
}

fn add_dir_files<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    base: &Path,
    prefix: &str,
) -> Result<(), String> {
    if !base.is_dir() {
        return Ok(());
    }
    let entries = std::fs::read_dir(base).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "." || name == ".." {
            continue;
        }
        let arc = format!("{prefix}/{name}");
        if path.is_file() {
            let data = std::fs::read(&path).map_err(|e| e.to_string())?;
            zip.start_file(&arc, zip_options())
                .map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        } else if path.is_dir() {
            add_dir_files(zip, &path, &arc)?;
        }
    }
    Ok(())
}

/// Construit un ZIP contenant prefs + configs comptes.
pub fn build_export_zip() -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);

        let manifest = ExportManifest {
            version: EXPORT_VERSION,
            exported_at: Utc::now().to_rfc3339(),
            app: "himaweb".into(),
        };
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
        zip.start_file(MANIFEST_NAME, zip_options())
            .map_err(|e| e.to_string())?;
        zip.write_all(&manifest_bytes)
            .map_err(|e| e.to_string())?;

        if let Ok(prefs_path) = Prefs::path() {
            add_file_if_exists(&mut zip, "prefs.json", &prefs_path);
        }

        add_file_if_exists(
            &mut zip,
            "himalaya/config.toml",
            &prefs::himalaya_config_path(),
        );
        add_file_if_exists(
            &mut zip,
            "cardamum/config.toml",
            &prefs::cardamum_config_path(),
        );
        add_file_if_exists(
            &mut zip,
            "calendula/config.toml",
            &prefs::calendula_config_path(),
        );

        let ortie = accounts_config::ortie_config_path();
        add_file_if_exists(&mut zip, "ortie/config.toml", &ortie);
        if let Some(parent) = ortie.parent() {
            let tokens = parent.join("tokens");
            add_dir_files(&mut zip, &tokens, "ortie/tokens")?;
        }

        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(cursor.into_inner())
}

fn backup_existing(path: &Path) {
    if !path.is_file() {
        return;
    }
    let bak = PathBuf::from(format!("{}.bak-import", path.display()));
    let _ = std::fs::copy(path, &bak);
}

fn write_bytes(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    backup_existing(path);
    std::fs::write(path, data).map_err(|e| e.to_string())
}

fn normalize_arc_name(name: &str) -> String {
    name.replace('\\', "/")
}

/// Applique un ZIP d’export. Retourne un résumé texte.
pub fn apply_import_zip(bytes: &[u8]) -> Result<String, String> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| format!("ZIP invalide: {e}"))?;

    let mut restored = Vec::new();
    let mut has_manifest = false;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("entrée ZIP: {e}"))?;
        if file.is_dir() {
            continue;
        }
        let name = normalize_arc_name(file.name());
        if name.is_empty() || name.contains("..") {
            continue;
        }

        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|e| format!("lecture {name}: {e}"))?;

        if name == MANIFEST_NAME {
            has_manifest = true;
            let _ = serde_json::from_slice::<ExportManifest>(&data);
            continue;
        }

        let dest = match name.as_str() {
            "prefs.json" => Prefs::path()?,
            "himalaya/config.toml" => prefs::himalaya_config_path(),
            "cardamum/config.toml" => prefs::cardamum_config_path(),
            "calendula/config.toml" => prefs::calendula_config_path(),
            "ortie/config.toml" => accounts_config::ortie_config_path(),
            other if other.starts_with("ortie/tokens/") => {
                let rel = other.trim_start_matches("ortie/tokens/");
                if rel.is_empty() || rel.contains("..") {
                    continue;
                }
                let ortie = accounts_config::ortie_config_path();
                let parent = ortie
                    .parent()
                    .ok_or_else(|| "dossier ortie introuvable".to_string())?;
                parent.join("tokens").join(rel)
            }
            _ => continue,
        };

        write_bytes(&dest, &data)?;
        restored.push(name);
    }

    if !has_manifest && restored.is_empty() {
        return Err("ZIP vide ou non reconnu (attendu: export HimaWeb)".into());
    }
    if restored.is_empty() {
        return Err("Aucune config trouvée dans le ZIP".into());
    }

    Ok(format!(
        "Import OK — {} fichier(s) restauré(s) : {}",
        restored.len(),
        restored.join(", ")
    ))
}
