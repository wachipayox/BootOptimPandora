//! Explicit, read-only discovery; only confirmation writes into our skin library.
use base64::{Engine, engine::general_purpose::STANDARD};
use bridge::message::{SkinImportCandidate, SkinImportPlan, SkinImportSource};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    io::Cursor,
    path::{Path, PathBuf},
};
const MAX_PNG: u64 = 2 * 1024 * 1024;
const MAX_LIBRARY: u64 = 64 * 1024 * 1024;
const MAX_CANDIDATES: usize = 512;

fn pixels(bytes: &[u8]) -> Option<[u8; 32]> {
    if bytes.len() as u64 > MAX_PNG {
        return None;
    }
    let reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let (width, height) = reader.into_dimensions().ok()?;
    if width != 64 || !matches!(height, 32 | 64) {
        return None;
    }
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?.to_rgba8();
    let mut hash = Sha256::new();
    hash.update(height.to_le_bytes());
    hash.update(image.as_raw());
    Some(hash.finalize().into())
}
fn pngs(folder: &Path) -> Vec<(String, Vec<u8>)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return vec![];
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .take(MAX_CANDIDATES)
        .filter_map(|path| {
            if path.metadata().ok()?.len() > MAX_PNG {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            pixels(&bytes)?;
            Some((path.file_stem()?.to_string_lossy().into_owned(), bytes))
        })
        .collect()
}
fn official(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    if !path.exists() {
        return Ok(vec![]);
    }
    if path.metadata().map_err(|e| e.to_string())?.len() > MAX_LIBRARY {
        return Err("skin library too large".into());
    }
    #[derive(serde::Deserialize)]
    struct Library {
        #[serde(rename = "customSkins", default)]
        skins: BTreeMap<String, Skin>,
    }
    #[derive(serde::Deserialize)]
    struct Skin {
        #[serde(default)]
        name: String,
        #[serde(rename = "skinImage", default)]
        image: String,
    }
    let library: Library =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(library
        .skins
        .into_values()
        .take(MAX_CANDIDATES)
        .filter_map(|skin| {
            let bytes = STANDARD.decode(skin.image.strip_prefix("data:image/png;base64,")?.trim()).ok()?;
            pixels(&bytes)?;
            Some((skin.name, bytes))
        })
        .collect())
}
fn roots() -> Option<(PathBuf, Vec<PathBuf>)> {
    #[cfg(windows)]
    {
        let root = PathBuf::from(std::env::var_os("APPDATA")?);
        Some((root.join(".minecraft"), vec![root.join("PrismLauncher")]))
    }
    #[cfg(target_os = "macos")]
    {
        let root = PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support");
        Some((root.join("minecraft"), vec![root.join("PrismLauncher")]))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        Some((
            home.join(".minecraft"),
            vec![
                data.join("PrismLauncher"),
                home.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher"),
            ],
        ))
    }
}
fn prism_skins(root: &Path) -> PathBuf {
    // Prism's SkinsDir is configurable; relative values use its data root.
    let configured = std::fs::read_to_string(root.join("prismlauncher.cfg"))
        .ok()
        .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("SkinsDir=").map(str::to_owned)))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    match configured {
        Some(path) if path.is_absolute() => path,
        Some(path) => root.join(path),
        None => root.join("skins"),
    }
}
pub(crate) fn discover(library: &Path) -> Result<SkinImportPlan, String> {
    let Some((minecraft, prism)) = roots() else {
        return Ok(SkinImportPlan::default());
    };
    let mut sources = vec![(
        "Launcher oficial de Minecraft".to_owned(),
        official(&minecraft.join("launcher_custom_skins.json"))?,
    )];
    let prism = prism.into_iter().flat_map(|root| pngs(&prism_skins(&root))).collect();
    sources.push(("Prism Launcher".into(), prism));
    Ok(plan(library, sources))
}
fn existing_pixels(library: &Path) -> HashSet<[u8; 32]> {
    let mut seen = HashSet::new();
    let Ok(entries) = std::fs::read_dir(library) else {
        return seen;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file())
            || !entry.metadata().is_ok_and(|meta| meta.len() <= MAX_PNG)
        {
            continue;
        }
        if let Ok(bytes) = std::fs::read(entry.path())
            && let Some(hash) = pixels(&bytes)
        {
            seen.insert(hash);
        }
    }
    seen
}
fn plan(library: &Path, sources: Vec<(String, Vec<(String, Vec<u8>)>)>) -> SkinImportPlan {
    let mut seen = existing_pixels(library);
    let mut plan = SkinImportPlan::default();
    for (label, candidates) in sources {
        if candidates.is_empty() {
            continue;
        }
        let found = candidates.len();
        let mut repeated = 0;
        for (name, bytes) in candidates {
            let Some(hash) = pixels(&bytes) else {
                continue;
            };
            if !seen.insert(hash) {
                repeated += 1;
                continue;
            }
            plan.candidates.push(SkinImportCandidate { name, bytes });
        }
        plan.sources.push(SkinImportSource { label, found, repeated });
        plan.repeated += repeated;
    }
    plan
}
pub(crate) fn import(library: &Path, plan: SkinImportPlan) -> Result<usize, String> {
    std::fs::create_dir_all(library).map_err(|e| e.to_string())?;
    let mut seen = existing_pixels(library);
    let mut added = 0;
    for candidate in plan.candidates {
        let Some(hash) = pixels(&candidate.bytes) else {
            continue;
        };
        if !seen.insert(hash) {
            continue;
        }
        let name = sanitize_filename::sanitize_with_options(
            candidate.name.trim(),
            sanitize_filename::Options {
                windows: true,
                ..Default::default()
            },
        );
        let name: String = name.chars().take(64).collect();
        let name = format!("{}.png", if name.is_empty() { "skin" } else { &name });
        let name = crate::fs::unique_name(library, &name, false);
        crate::fs::write_safe(&library.join(&*name), &candidate.bytes).map_err(|e| e.to_string())?;
        added += 1;
    }
    Ok(added)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn png(shade: u8, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(64, height, image::Rgba([shade, 0, 0, 255]));
        let mut bytes = Cursor::new(vec![]);
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    #[test]
    fn discovery_is_read_only_and_confirmation_deduplicates_again() {
        let root = std::env::temp_dir().join(format!("pandora-skin-import-{}", uuid::Uuid::from_bytes(rand::random())));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("launcher_custom_skins.json");
        let bytes = png(120, 64);
        let json = serde_json::json!({"customSkins":{"one":{"name":"../one", "skinImage":format!("data:image/png;base64,{}",STANDARD.encode(&bytes))}}});
        let source = serde_json::to_vec(&json).unwrap();
        std::fs::write(&path, &source).unwrap();
        let library = root.join("our-library");
        let found = official(&path).unwrap();
        let proposal = plan(
            &library,
            vec![
                ("Official".into(), found),
                ("Prism".into(), vec![("copy".into(), bytes.clone()), ("legacy".into(), png(2, 32))]),
            ],
        );
        assert_eq!(proposal.candidates.len(), 2);
        assert_eq!(proposal.repeated, 1);
        assert!(!library.exists());
        // A concurrent addition after discovery must not be overwritten or imported twice.
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(library.join("existing.png"), bytes).unwrap();
        assert_eq!(import(&library, proposal.clone()).unwrap(), 1);
        assert_eq!(import(&library, proposal).unwrap(), 0);
        assert_eq!(std::fs::read_dir(&library).unwrap().count(), 2);
        assert_eq!(std::fs::read(path).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_wrong_dimensions_and_honors_prism_directory() {
        assert!(pixels(&png(1, 63)).is_none());
        assert!(pixels(b"not png").is_none());
        let root = std::env::temp_dir().join(format!("pandora-skin-path-{}", uuid::Uuid::from_bytes(rand::random())));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("prismlauncher.cfg"), "[General]\nSkinsDir=custom-skins\n").unwrap();
        assert_eq!(prism_skins(&root), root.join("custom-skins"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
