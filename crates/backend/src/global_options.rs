//! Launcher-wide Minecraft settings: storage and the launch/exit hooks.
//!
//! The settings live in `<launcher>/global_options.json` in canonical form. Before
//! an instance starts they are written into its `.minecraft/options.txt` in that
//! instance's own format; what was written is remembered in the instance root.
//! When the game exits, every setting the player changed there since is copied
//! back to the global settings, so the next instance starts with it.
//! Pure format logic lives in `schema::global_options`.
use std::{
    io::Read,
    path::{Path, PathBuf},
};

use schema::{
    global_options::{self, ApplyOutcome, Canonical, OptionsText},
    loader::Loader,
};
use serde::{Deserialize, Serialize};

const STORE_FILE: &str = "global_options.json";
const SNAPSHOT_FILE: &str = ".pandora-global-options-v1.json";

/// Serialises read-modify-write of the store between instances exiting together.
static LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub values: Canonical,
    /// Where the settings were first taken from, for the settings page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Snapshot {
    values: Canonical,
}

/// One instance, as the hooks need it.
pub struct Target<'a> {
    pub root: &'a Path,
    pub dot_minecraft: &'a Path,
    pub minecraft_version: &'a str,
    pub loader: Loader,
}

impl Target<'_> {
    fn options_path(&self) -> PathBuf {
        self.dot_minecraft.join("options.txt")
    }
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

/// Copies the player's changes since the last apply into the store. Returns
/// how many settings changed.
fn collect(launcher_dir: &Path, target: &Target, store: &mut Store) -> std::io::Result<usize> {
    let Some(file) = read_options(&target.options_path())? else {
        return Ok(0);
    };
    let snapshot: Snapshot = std::fs::read(target.root.join(SNAPSHOT_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let changes = global_options::changes_since(&file, &snapshot.values);
    if changes.is_empty() {
        return Ok(0);
    }
    let count = changes.len();
    store.values.extend(changes);
    save_store(launcher_dir, store)?;
    Ok(count)
}

/// Writes the store into the instance and remembers what it looks like now.
fn apply(libraries_dir: &Path, target: &Target, store: &Store) -> std::io::Result<ApplyOutcome> {
    let path = target.options_path();
    let existing = read_options(&path)?;
    let mut file = existing.clone().unwrap_or_default();
    let fallback = if file.data_version().is_none() {
        client_jar_data_version(libraries_dir, target.minecraft_version)
    } else {
        None
    };
    let outcome = global_options::apply(&mut file, &store.values, target.loader, fallback);
    match outcome {
        ApplyOutcome::NeedsDataVersion => return Ok(outcome),
        ApplyOutcome::Changed(_) => {
            std::fs::create_dir_all(target.dot_minecraft)?;
            write_atomic(&path, file.render().as_bytes())?;
        },
        ApplyOutcome::Unchanged => {},
    }
    let snapshot = Snapshot {
        values: global_options::capture(&file),
    };
    write_atomic(&target.root.join(SNAPSHOT_FILE), &serde_json::to_vec(&snapshot)?)?;
    Ok(outcome)
}

/// Before launch: keep changes made while the launcher was not watching, then
/// bring the instance up to date. An empty store is first filled from this
/// instance, so turning the feature on keeps the settings the player has.
pub fn before_launch(launcher_dir: &Path, libraries_dir: &Path, target: &Target) {
    let _guard = LOCK.lock();
    let mut store = load_store(launcher_dir);
    let result: std::io::Result<ApplyOutcome> = (|| {
        if store.values.is_empty() {
            if let Some(file) = read_options(&target.options_path())? {
                store.values = global_options::capture(&file);
                store.source.get_or_insert_with(|| {
                    target
                        .root
                        .file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
                });
                save_store(launcher_dir, &store)?;
            }
        } else {
            collect(launcher_dir, target, &mut store)?;
        }
        if store.values.is_empty() {
            return Ok(ApplyOutcome::Unchanged);
        }
        apply(libraries_dir, target, &store)
    })();
    match result {
        Ok(ApplyOutcome::Changed(count)) => log::info!("Applied {count} global Minecraft settings"),
        Ok(_) => {},
        Err(error) => log::warn!("Unable to apply global Minecraft settings: {error}"),
    }
}

/// After the game exits: publish the player's changes and re-apply, which also
/// covers an instance whose first run created `options.txt`.
pub fn after_exit(launcher_dir: &Path, libraries_dir: &Path, target: &Target) {
    let _guard = LOCK.lock();
    let mut store = load_store(launcher_dir);
    let result: std::io::Result<usize> = (|| {
        let collected = collect(launcher_dir, target, &mut store)?;
        if store.values.is_empty() {
            return Ok(collected);
        }
        apply(libraries_dir, target, &store)?;
        Ok(collected)
    })();
    match result {
        Ok(0) => {},
        Ok(count) => log::info!("Kept {count} changed Minecraft settings as global"),
        Err(error) => log::warn!("Unable to keep changed Minecraft settings: {error}"),
    }
}

/// Replaces the global settings with the ones in `options`. Used by the
/// installer's migration and by "use this instance's settings".
pub fn import(launcher_dir: &Path, options: &Path, source: &str) -> std::io::Result<usize> {
    let _guard = LOCK.lock();
    let Some(file) = read_options(options)? else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "options.txt not found"));
    };
    let store = Store {
        values: global_options::capture(&file),
        source: Some(source.to_string()),
    };
    save_store(launcher_dir, &store)?;
    Ok(store.values.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (TempDir, PathBuf, PathBuf, PathBuf) {
        let temp = TempDir(std::env::temp_dir().join(format!("pandora-global-options-{}", uuid::Uuid::new_v4())));
        let launcher = temp.path().join("launcher");
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        for dir in [&launcher, &a.join(".minecraft"), &b.join(".minecraft")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        (temp, launcher, a, b)
    }

    fn target<'a>(root: &'a Path, dot: &'a Path, loader: Loader) -> Target<'a> {
        Target {
            root,
            dot_minecraft: dot,
            minecraft_version: "1.21.1",
            loader,
        }
    }

    #[test]
    fn a_change_in_one_instance_reaches_the_next() {
        let (_temp, launcher, a, b) = fixture();
        let (a_dot, b_dot) = (a.join(".minecraft"), b.join(".minecraft"));
        std::fs::write(
            a_dot.join("options.txt"),
            "version:3955\nfov:0.0\nkey_key.jei.toggleOverlay:key.keyboard.o:CONTROL\n",
        )
        .unwrap();
        std::fs::write(b_dot.join("options.txt"), "version:1343\nfov:0.5\nlang:en_US\n").unwrap();

        // First launch fills the empty store from instance A.
        before_launch(&launcher, &launcher, &target(&a, &a_dot, Loader::NeoForge));
        // The player changes the FOV in A and quits.
        std::fs::write(
            a_dot.join("options.txt"),
            "version:3955\nfov:0.75\nkey_key.jei.toggleOverlay:key.keyboard.o:CONTROL\n",
        )
        .unwrap();
        after_exit(&launcher, &launcher, &target(&a, &a_dot, Loader::NeoForge));
        assert_eq!(load_store(&launcher).values["fov"], "0.75");

        // B is old Vanilla: it gets the FOV, but not a Ctrl binding it cannot express.
        before_launch(&launcher, &launcher, &target(&b, &b_dot, Loader::Vanilla));
        let b_file = OptionsText::parse(&std::fs::read_to_string(b_dot.join("options.txt")).unwrap());
        assert_eq!(b_file.get("fov"), Some("0.75"));
        assert_eq!(b_file.get("key_key.jei.toggleOverlay"), None);
        assert_eq!(b_file.get("version"), Some("1343"));
    }

    #[test]
    fn import_replaces_the_store() {
        let (temp, launcher, _, _) = fixture();
        let old = temp.path().join("options.txt");
        std::fs::write(&old, "version:3955\nresourcePacks:[]\ngamma:0.4\n").unwrap();
        assert_eq!(import(&launcher, &old, "migración").unwrap(), 1);
        let store = load_store(&launcher);
        assert_eq!(store.values["gamma"], "0.4");
        assert_eq!(store.source.as_deref(), Some("migración"));
    }
}
