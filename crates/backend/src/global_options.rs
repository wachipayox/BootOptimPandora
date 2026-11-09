//! Launcher-wide Minecraft settings: storage, and the copy into new instances.
//!
//! The settings live in `<launcher>/global_options.json` in canonical form. A new
//! instance (created from scratch or from a global profile) is marked
//! `global_options_pending`; before its first start the settings are written
//! into its `.minecraft/options.txt` in that instance's own format, and from then
//! on the file is the instance's own. Settings the player marked as not synced
//! are never written. Pure format logic lives in `schema::global_options`.
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};

use schema::{
    global_options::{self, ApplyOutcome, Canonical, OptionsText},
    loader::Loader,
};
use serde::{Deserialize, Serialize};

const STORE_FILE: &str = "global_options.json";

/// Serialises read-modify-write of the store.
static LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub values: Canonical,
    /// Keys the player chose not to copy into new instances.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub excluded: BTreeSet<String>,
    /// Where the settings were last taken from, for the settings page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl Store {
    /// What a new instance receives.
    pub fn effective(&self) -> Canonical {
        self.values
            .iter()
            .filter(|(key, _)| !self.excluded.contains(*key))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// One instance, as the copy needs it.
pub struct Target<'a> {
    pub dot_minecraft: &'a Path,
    pub minecraft_version: &'a str,
    pub loader: Loader,
}

fn store_path(launcher_dir: &Path) -> PathBuf {
    launcher_dir.join(STORE_FILE)
}

pub fn load_store(launcher_dir: &Path) -> Store {
    match std::fs::read(store_path(launcher_dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            log::warn!("Ignoring unreadable global Minecraft settings: {error}");
            Store::default()
        }),
        Err(_) => Store::default(),
    }
}

fn save_store(launcher_dir: &Path, store: &Store) -> std::io::Result<()> {
    write_atomic(&store_path(launcher_dir), &serde_json::to_vec_pretty(store)?)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension("wbombo-tmp");
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

fn read_options(path: &Path) -> std::io::Result<Option<OptionsText>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(OptionsText::parse(&String::from_utf8_lossy(&bytes)))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Data version of a Minecraft release from its client jar (`version.json`,
/// 1.14+), for instances that have never written an `options.txt`.
fn client_jar_data_version(libraries_dir: &Path, minecraft_version: &str) -> Option<u32> {
    let jar = libraries_dir
        .join("net/minecraft")
        .join(minecraft_version)
        .join(format!("minecraft-client-{minecraft_version}.jar"));
    let mut archive = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    let mut entry = archive.by_name("version.json").ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let world = json.get("world_version")?.as_u64()?;
    u32::try_from(world).ok()
}

/// Whether a new instance should be marked to receive the settings.
pub fn has_settings(launcher_dir: &Path) -> bool {
    !load_store(launcher_dir).effective().is_empty()
}

/// Writes the global settings into one instance's `options.txt`.
pub fn copy_into(launcher_dir: &Path, libraries_dir: &Path, target: &Target) -> std::io::Result<ApplyOutcome> {
    let store = {
        let _guard = LOCK.lock();
        load_store(launcher_dir)
    };
    let path = target.dot_minecraft.join("options.txt");
    let mut file = read_options(&path)?.unwrap_or_default();
    let fallback = if file.data_version().is_none() {
        client_jar_data_version(libraries_dir, target.minecraft_version)
    } else {
        None
    };
    let outcome = global_options::apply(&mut file, &store.effective(), target.loader, fallback);
    if let ApplyOutcome::Changed(_) = outcome {
        std::fs::create_dir_all(target.dot_minecraft)?;
        write_atomic(&path, file.render().as_bytes())?;
    }
    Ok(outcome)
}

/// Replaces the global settings with the ones in `options`, keeping which keys
/// are not synced. Used by the installer's migration and by "use this
/// instance's settings".
pub fn import(launcher_dir: &Path, options: &Path, source: &str) -> std::io::Result<usize> {
    let _guard = LOCK.lock();
    let Some(file) = read_options(options)? else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "options.txt not found"));
    };
    let mut store = load_store(launcher_dir);
    store.values = global_options::capture(&file);
    store.source = Some(source.to_string());
    save_store(launcher_dir, &store)?;
    Ok(store.values.len())
}

/// Sets (or, with `None`, forgets) one setting.
pub fn set_value(launcher_dir: &Path, key: &str, value: Option<&str>) -> std::io::Result<()> {
    let _guard = LOCK.lock();
    let mut store = load_store(launcher_dir);
    match value {
        Some(value) => store.values.insert(key.to_string(), value.to_string()),
        None => store.values.remove(key),
    };
    save_store(launcher_dir, &store)
}

/// Whether one setting is copied into new instances.
pub fn set_synced(launcher_dir: &Path, key: &str, synced: bool) -> std::io::Result<()> {
    let _guard = LOCK.lock();
    let mut store = load_store(launcher_dir);
    if synced {
        store.excluded.remove(key);
    } else {
        store.excluded.insert(key.to_string());
    }
    save_store(launcher_dir, &store)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        let temp = TempDir(std::env::temp_dir().join(format!("pandora-global-options-{}", uuid::Uuid::new_v4())));
        let launcher = temp.0.join("launcher");
        let dot = temp.0.join("instance/.minecraft");
        std::fs::create_dir_all(&launcher).unwrap();
        std::fs::create_dir_all(&dot).unwrap();
        (temp, launcher, dot)
    }

    #[test]
    fn imported_settings_reach_a_new_instance_in_its_format_except_excluded_ones() {
        let (temp, launcher, dot) = fixture();
        let old = temp.0.join("options.txt");
        std::fs::write(&old, "version:3955\nresourcePacks:[]\nfov:0.75\nrenderDistance:8\nlang:es_es\n").unwrap();
        assert_eq!(import(&launcher, &old, "migración").unwrap(), 3);
        set_synced(&launcher, "renderDistance", false).unwrap();
        assert!(has_settings(&launcher));

        // A 1.12 profile shipped its own first-run options.
        std::fs::write(dot.join("options.txt"), "version:1343\nfov:0.0\nrenderDistance:12\nlang:en_US\n").unwrap();
        let target = Target {
            dot_minecraft: &dot,
            minecraft_version: "1.12.2",
            loader: Loader::Forge,
        };
        assert_eq!(copy_into(&launcher, &launcher, &target).unwrap(), ApplyOutcome::Changed(2));
        let file = OptionsText::parse(&std::fs::read_to_string(dot.join("options.txt")).unwrap());
        assert_eq!(file.get("fov"), Some("0.75"));
        assert_eq!(file.get("lang"), Some("es_ES"));
        assert_eq!(file.get("renderDistance"), Some("12"));

        // Re-importing keeps the player's exclusions and the source.
        import(&launcher, &old, "otra").unwrap();
        let store = load_store(&launcher);
        assert!(store.excluded.contains("renderDistance"));
        assert_eq!(store.source.as_deref(), Some("otra"));
    }

    #[test]
    fn an_instance_that_never_started_waits_for_its_format() {
        let (_temp, launcher, dot) = fixture();
        set_value(&launcher, "fov", Some("0.5")).unwrap();
        let target = Target {
            dot_minecraft: &dot,
            minecraft_version: "1.21.1",
            loader: Loader::Vanilla,
        };
        assert_eq!(copy_into(&launcher, &launcher, &target).unwrap(), ApplyOutcome::NeedsDataVersion);
        assert!(!dot.join("options.txt").exists());
        set_value(&launcher, "fov", None).unwrap();
        assert!(!has_settings(&launcher));
    }
}
