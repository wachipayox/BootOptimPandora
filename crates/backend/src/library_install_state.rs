use std::{
    io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};

const STATE_DIR: &str = ".bootoptim";
const STATE_FILE: &str = "game-files-state-v1.json";
static STATE_LOCK: Mutex<()> = Mutex::new(());

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
    STATE_LOCK
        .lock()
        .map_err(|_| io::Error::other("game-files state lock poisoned"))
}

fn write_state_unlocked(instance_root: &Path, state: StateKind, reason: &str) -> io::Result<()> {
    let dir = instance_root.join(STATE_DIR);
    std::fs::create_dir_all(&dir)?;
    let bytes = serde_json::to_vec(&StateFile {
        schema: 1,
        state,
        reason: reason.to_owned(),
    })
    .map_err(io::Error::other)?;
    crate::fs::write_safe(&dir.join(STATE_FILE), &bytes)
}

pub fn mark_incomplete(instance_root: &Path, reason: &str) -> io::Result<()> {
    let _guard = lock_state()?;
    write_state_unlocked(instance_root, StateKind::Incomplete, reason)
}

pub fn mark_published(instance_root: &Path, reason: &str) -> io::Result<()> {
    let _guard = lock_state()?;
    write_state_unlocked(instance_root, StateKind::Published, reason)
}

pub fn publish_if_incomplete_reason(
    instance_root: &Path,
    expected_reason: &str,
    publish_reason: &str,
) -> io::Result<bool> {
    let _guard = lock_state()?;
    match read_state_unlocked(instance_root)? {
        StartStatus::Incomplete(reason) if reason == expected_reason => {
            write_state_unlocked(instance_root, StateKind::Published, publish_reason)?;
            Ok(true)
        },
        _ => Ok(false),
    }
}

pub fn start_status(instance_root: &Path) -> io::Result<StartStatus> {
    let _guard = lock_state()?;
    read_state_unlocked(instance_root)
}

fn read_state_unlocked(instance_root: &Path) -> io::Result<StartStatus> {
    let path = state_path(instance_root);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(StartStatus::LegacyPublished),
        Err(err) => return Err(err),
    };
    let state: StateFile = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if state.schema != 1 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported game-files state schema"));
    }
    Ok(match state.state {
        StateKind::Published => StartStatus::Published,
        StateKind::Incomplete => StartStatus::Incomplete(state.reason),
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
        mark_incomplete(&root, "repair-in-progress").unwrap();
        assert_eq!(start_status(&root).unwrap(), StartStatus::Incomplete("repair-in-progress".to_owned()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conditional_publish_does_not_overwrite_a_newer_transition() {
        let root = root("conditional-publish");
        mark_incomplete(&root, "version-change").unwrap();
        mark_incomplete(&root, "repair-in-progress").unwrap();

        assert!(!publish_if_incomplete_reason(
            &root,
            "version-change",
            "identity-update-complete",
        )
        .unwrap());
        assert_eq!(
            start_status(&root).unwrap(),
            StartStatus::Incomplete("repair-in-progress".to_owned())
        );

        assert!(publish_if_incomplete_reason(
            &root,
            "repair-in-progress",
            "repair-complete",
        )
        .unwrap());
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
