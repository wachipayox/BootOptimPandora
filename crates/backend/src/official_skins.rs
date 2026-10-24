//! Imports the official Minecraft Launcher's skin library into the launcher's own `skins` folder.
//!
//! The official launcher keeps custom skins in `launcher_custom_skins.json` inside its game
//! directory, each as a base64 PNG. A Microsoft account only stores the active skin and capes, so
//! this local library is not reachable through the account. Skins are imported once per official
//! skin id: `official-skins-imported.json` remembers which ids were taken, so skins added later in
//! the official launcher are picked up and skins removed here are not brought back. The official
//! file is only read. The slim/classic model is not kept because this library stores plain PNGs
//! and the model is chosen when a skin is applied.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

const OFFICIAL_FILE: &str = "launcher_custom_skins.json";
const RECORD_FILE: &str = "official-skins-imported.json";
const MAX_OFFICIAL_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Deserialize)]
struct OfficialLibrary {
    #[serde(rename = "customSkins", default)]
    custom_skins: BTreeMap<String, OfficialSkin>,
}

#[derive(Deserialize)]
struct OfficialSkin {
    #[serde(default)]
    name: String,
    #[serde(rename = "skinImage", default)]
    skin_image: String,
}

#[derive(Default, Deserialize, Serialize)]
struct ImportRecord {
    imported_ids: BTreeSet<String>,
}

/// The official launcher's game directory for this platform.
fn official_game_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join(".minecraft"))
    }
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/minecraft"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Some(PathBuf::from(std::env::var_os("HOME")?).join(".minecraft"))
    }
}

pub(crate) fn import_official_skins(launcher_dir: &Path, skin_library_dir: &Path) {
    let Some(game_dir) = official_game_dir() else {
        return;
    };
    match import_from(&game_dir.join(OFFICIAL_FILE), &launcher_dir.join(RECORD_FILE), skin_library_dir) {
        Ok(0) => {},
        Ok(added) => log::info!(
            "[skins] Imported {added} skin(s) from the official Minecraft Launcher library; its file was not modified."
        ),
        Err(error) => log::warn!("[skins] Could not import the official Minecraft Launcher skin library: {error}"),
    }
}

fn import_from(official_file: &Path, record_path: &Path, skin_library_dir: &Path) -> Result<usize, String> {
    let Ok(metadata) = std::fs::metadata(official_file) else {
        return Ok(0);
    };
    if !metadata.is_file() || metadata.len() > MAX_OFFICIAL_FILE_BYTES {
        return Err("the official skin library is not a regular file of a supported size".into());
    }
    let library: OfficialLibrary = serde_json::from_slice(&std::fs::read(official_file).map_err(|e| e.to_string())?)
        .map_err(|error| format!("unreadable {OFFICIAL_FILE} ({error})"))?;
    let mut record: ImportRecord = match std::fs::read(record_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| format!("unreadable {RECORD_FILE} ({error})"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ImportRecord::default(),
        Err(error) => return Err(error.to_string()),
    };

    std::fs::create_dir_all(skin_library_dir).map_err(|error| error.to_string())?;
    let mut added = 0;
    let mut changed = false;
    for (id, skin) in library.custom_skins {
        if record.imported_ids.contains(&id) {
            continue;
        }
        let Some(bytes) = decode_skin(&skin.skin_image) else {
            log::warn!("[skins] Skipped an official skin that is not a valid 64x64 or 64x32 PNG");
            record.imported_ids.insert(id);
            changed = true;
            continue;
        };
        if !already_in_library(skin_library_dir, &bytes) {
            let name = file_name_for(&skin.name);
            let name = crate::fs::unique_name(skin_library_dir, &name, false);
            crate::fs::write_safe(&skin_library_dir.join(&*name), &bytes).map_err(|error| error.to_string())?;
            added += 1;
        }
        record.imported_ids.insert(id);
        changed = true;
    }
    if changed {
        let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
        crate::fs::write_safe(record_path, &bytes).map_err(|error| error.to_string())?;
    }
    Ok(added)
}

fn decode_skin(data_url: &str) -> Option<Vec<u8>> {
    let encoded = data_url.strip_prefix("data:image/png;base64,")?;
    let bytes = STANDARD.decode(encoded.trim()).ok()?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
    crate::skin_manager::SkinManager::is_valid_size(&image).then_some(bytes)
}

fn already_in_library(dir: &Path, bytes: &[u8]) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry.metadata().is_ok_and(|metadata| metadata.len() == bytes.len() as u64)
                && std::fs::read(entry.path()).is_ok_and(|existing| existing == bytes)
        })
    })
}

fn file_name_for(name: &str) -> String {
    let options = sanitize_filename::Options {
        windows: true,
        ..Default::default()
    };
    let base = sanitize_filename::sanitize_with_options(name.trim(), options);
    let base = if base.is_empty() {
        "skin".to_owned()
    } else {
        base.chars().take(64).collect()
    };
    format!("{base}.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, shade: u8) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([shade, 0, 0, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn library(entries: &[(&str, &str, &[u8])]) -> String {
        let skins = entries
            .iter()
            .map(|(id, name, bytes)| {
                format!(
                    r#""{id}":{{"id":"{id}","name":"{name}","slim":false,"skinImage":"data:image/png;base64,{}"}}"#,
                    STANDARD.encode(bytes)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"customSkins":{{{skins}}},"version":1}}"#)
    }

    fn temp(label: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("wachiland-official-skins-{label}-{}", uuid::Uuid::from_bytes(rand::random())));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn imports_new_skins_once_and_skips_invalid_or_present_ones() {
        let root = temp("import");
        let (official, record, skins) = (root.join(OFFICIAL_FILE), root.join(RECORD_FILE), root.join("skins"));
        let (red, blue) = (png(64, 64, 200), png(64, 32, 20));
        std::fs::create_dir_all(&skins).unwrap();
        std::fs::write(skins.join("existing.png"), &blue).unwrap();
        let text = library(&[
            ("a", "My: skin", &red),
            ("b", "Old", &blue),
            ("c", "Bad", &png(10, 10, 1)),
        ]);
        std::fs::write(&official, &text).unwrap();

        assert_eq!(import_from(&official, &record, &skins).unwrap(), 1);
        assert_eq!(std::fs::read_dir(&skins).unwrap().count(), 2);
        assert_eq!(std::fs::read_to_string(&official).unwrap(), text);

        // Removing an imported skin here does not bring it back; a new official skin is added.
        for entry in std::fs::read_dir(&skins).unwrap().flatten() {
            if std::fs::read(entry.path()).unwrap() == red {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
        std::fs::write(&official, library(&[("a", "My: skin", &red), ("d", "New", &png(64, 64, 90))])).unwrap();
        assert_eq!(import_from(&official, &record, &skins).unwrap(), 1);
        assert_eq!(std::fs::read_dir(&skins).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_or_unreadable_library_changes_nothing() {
        let root = temp("missing");
        let (official, record, skins) = (root.join(OFFICIAL_FILE), root.join(RECORD_FILE), root.join("skins"));
        assert_eq!(import_from(&official, &record, &skins).unwrap(), 0);
        std::fs::write(&official, b"{broken").unwrap();
        assert!(import_from(&official, &record, &skins).is_err());
        assert!(!record.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
