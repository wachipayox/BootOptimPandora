use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::{Deserialize, Serialize};

const STATE_DIR: &str = ".bootoptim";
const STATE_FILE: &str = "game-files-state-v1.json";
static STATE_LOCK: Mutex<()> = Mutex::new(());
static STATE_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartStatus {
    Published,
    LegacyPublished,
    Incomplete(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct StateFile {
    schema: u32,
    state: StateKind,
    reason: String,
    #[serde(default)]
    generation: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum StateKind {
    Published,
    Incomplete,
}

fn state_path(instance_root: &Path) -> PathBuf {
    instance_root.join(STATE_DIR).join(STATE_FILE)
}

fn lock_state() -> io::Result<MutexGuard<'static, ()>> {
    STATE_LOCK.lock().map_err(|_| io::Error::other("game-files state lock poisoned"))
}

fn write_state_unlocked(instance_root: &Path, state: StateKind, reason: &str, generation: u64) -> io::Result<()> {
    let dir = instance_root.join(STATE_DIR);
    std::fs::create_dir_all(&dir)?;
    let bytes = serde_json::to_vec(&StateFile {
        schema: 1,
        state,
        reason: reason.to_owned(),
        generation,
    })
    .map_err(io::Error::other)?;
    crate::fs::write_safe(&dir.join(STATE_FILE), &bytes)
}

fn read_state_file_unlocked(instance_root: &Path) -> io::Result<Option<StateFile>> {
    let path = state_path(instance_root);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let state: StateFile = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if state.schema != 1 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported game-files state schema"));
    }
    Ok(Some(state))
}

pub fn mark_incomplete(instance_root: &Path, reason: &str) -> io::Result<u64> {
    let _guard = lock_state()?;
    let generation = STATE_GENERATION.fetch_add(1, Ordering::Relaxed);
    write_state_unlocked(instance_root, StateKind::Incomplete, reason, generation)?;
    Ok(generation)
}

pub fn mark_published(instance_root: &Path, reason: &str) -> io::Result<()> {
    let _guard = lock_state()?;
    write_state_unlocked(instance_root, StateKind::Published, reason, 0)
}

pub fn incomplete_generation(instance_root: &Path, expected_reason: &str) -> io::Result<Option<u64>> {
    let _guard = lock_state()?;
    Ok(match read_state_file_unlocked(instance_root)? {
        Some(StateFile {
            state: StateKind::Incomplete,
            reason,
            generation,
            ..
        }) if reason == expected_reason => Some(generation),
        _ => None,
    })
}

pub fn publish_if_incomplete_generation(
    instance_root: &Path,
    expected_reason: &str,
    expected_generation: u64,
    publish_reason: &str,
) -> io::Result<bool> {
    let _guard = lock_state()?;
    match read_state_file_unlocked(instance_root)? {
        Some(StateFile {
            state: StateKind::Incomplete,
            reason,
            generation,
            ..
        }) if reason == expected_reason && generation == expected_generation => {
            write_state_unlocked(instance_root, StateKind::Published, publish_reason, generation)?;
            Ok(true)
        },
        _ => Ok(false),
    }
}

pub fn start_status(instance_root: &Path) -> io::Result<StartStatus> {
    let _guard = lock_state()?;
    Ok(match read_state_file_unlocked(instance_root)? {
        None => StartStatus::LegacyPublished,
        Some(StateFile {
            state: StateKind::Published,
            ..
        }) => StartStatus::Published,
        Some(StateFile {
            state: StateKind::Incomplete,
            reason,
            ..
        }) => StartStatus::Incomplete(reason),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root(name: &str) -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("pandora-agent202-state-{name}-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn legacy_install_is_accepted_without_scanning() {
        let root = root("legacy");
        assert_eq!(start_status(&root).unwrap(), StartStatus::LegacyPublished);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn interrupted_operation_blocks_start_until_publish() {
        let root = root("interrupted");
        mark_incomplete(&root, "update-in-progress").unwrap();
        assert_eq!(start_status(&root).unwrap(), StartStatus::Incomplete("update-in-progress".to_owned()));
        mark_published(&root, "test-publish").unwrap();
        assert_eq!(start_status(&root).unwrap(), StartStatus::Published);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cancelled_repair_does_not_publish() {
        let root = root("cancel");
        let repair_generation = mark_incomplete(&root, "repair-in-progress").unwrap();
        mark_incomplete(&root, "repair-cancelled").unwrap();

        assert!(
            !publish_if_incomplete_generation(&root, "repair-in-progress", repair_generation, "repair-complete",)
                .unwrap()
        );
        assert_eq!(start_status(&root).unwrap(), StartStatus::Incomplete("repair-cancelled".to_owned()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conditional_publish_does_not_overwrite_a_newer_transition() {
        let root = root("conditional-publish");
        let version_generation = mark_incomplete(&root, "version-change").unwrap();
        let repair_generation = mark_incomplete(&root, "repair-in-progress").unwrap();

        assert!(
            !publish_if_incomplete_generation(&root, "version-change", version_generation, "identity-update-complete",)
                .unwrap()
        );
        assert_eq!(start_status(&root).unwrap(), StartStatus::Incomplete("repair-in-progress".to_owned()));

        assert!(
            publish_if_incomplete_generation(&root, "repair-in-progress", repair_generation, "repair-complete",)
                .unwrap()
        );
        assert_eq!(start_status(&root).unwrap(), StartStatus::Published);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn repeated_same_reason_requires_the_latest_generation() {
        let root = root("same-reason-generation");
        let stale_generation = mark_incomplete(&root, "loader-changed").unwrap();
        let current_generation = mark_incomplete(&root, "loader-changed").unwrap();

        assert!(
            !publish_if_incomplete_generation(&root, "loader-changed", stale_generation, "identity-update-complete",)
                .unwrap()
        );
        assert!(
            publish_if_incomplete_generation(&root, "loader-changed", current_generation, "identity-update-complete",)
                .unwrap()
        );
        assert_eq!(start_status(&root).unwrap(), StartStatus::Published);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_state_fails_closed() {
        let root = root("corrupt");
        std::fs::create_dir_all(root.join(STATE_DIR)).unwrap();
        std::fs::write(state_path(&root), b"{not-json").unwrap();
        assert!(start_status(&root).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
