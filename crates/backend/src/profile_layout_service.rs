//! Backend service seam for the persistent-profile flow.
//!
//! This is intentionally not called by Start. Future install/update and signed service/channel
//! code can call this only after it has resolved a desired local managed set.

use std::{io::ErrorKind, path::Path};

use bridge::instance::InstanceID;
use thiserror::Error;

use crate::{
    BackendState,
    profile_layout_flow::{DesiredManagedFile, PersistentProfileLayout, ProfileLayoutFlowError, ReconcileOutcome},
};

const CONTROL_DIR: &str = ".pandora-layout-v1";

#[derive(Debug, Error)]
pub enum ProfileLayoutServiceError {
    #[error("instance is no longer available")]
    MissingInstance,
    #[error("instance is running or still has a live launch keepalive")]
    InstanceRunning,
    #[error("legacy original_mods restoration did not complete or conflicts with persistent state")]
    LegacyRestoreIncomplete,
    #[error("sandbox profiles remain on the stock layout path in this slice")]
    SandboxStockOnly,
    #[error(transparent)]
    Layout(#[from] ProfileLayoutFlowError),
}

impl BackendState {
    /// Reconcile and publish a persistent profile while the instance is stopped.
    ///
    /// A profile with no persistent control namespace first runs Pandora's existing legacy
    /// `original_mods` restoration. If a persistent namespace already exists, `original_mods`
    /// is instead an ambiguous mixed-state signal: nothing live is changed before recovery has
    /// inspected the durable journal/manifest.
    pub fn reconcile_persistent_profile_layout(
        &self,
        id: InstanceID,
        desired: &[DesiredManagedFile],
    ) -> Result<ReconcileOutcome, ProfileLayoutServiceError> {
        let mut instance_state = self.instance_state.write();
        let instance = instance_state.instances.get_mut(id).ok_or(ProfileLayoutServiceError::MissingInstance)?;

        if !instance.processes.is_empty()
            || !instance.closing_processes.is_empty()
            || instance.launch_keepalive.as_ref().is_some_and(|keepalive| keepalive.is_alive())
        {
            return Err(ProfileLayoutServiceError::InstanceRunning);
        }
        if instance.configuration.get().sandbox {
            return Err(ProfileLayoutServiceError::SandboxStockOnly);
        }

        let persistent_state_exists = path_exists_no_follow(&instance.root_path.join(CONTROL_DIR))?;
        let original_mods = instance.root_path.join("original_mods");
        let had_original_mods = path_exists_no_follow(&original_mods)?;

        if persistent_state_exists {
            if had_original_mods {
                return Err(ProfileLayoutServiceError::LegacyRestoreIncomplete);
            }
        } else {
            self.restore_mods_folder_if_stopped(instance);
            verify_legacy_restore(&instance.root_path, had_original_mods)?;
        }

        let mut layout = PersistentProfileLayout::open(&instance.root_path)?;
        Ok(layout.reconcile(desired)?)
    }
}

fn path_exists_no_follow(path: &Path) -> Result<bool, ProfileLayoutServiceError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(ProfileLayoutFlowError::Io(err).into()),
    }
}

fn verify_legacy_restore(root: &Path, had_original_mods: bool) -> Result<(), ProfileLayoutServiceError> {
    if !had_original_mods {
        return Ok(());
    }
    if path_exists_no_follow(&root.join("original_mods"))? || !root.join(".minecraft/mods").is_dir() {
        return Err(ProfileLayoutServiceError::LegacyRestoreIncomplete);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;

    struct TestRoot(std::path::PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("pandora-layout-legacy-{}", Uuid::from_bytes(rand::random())));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn legacy_restore_must_finish_before_persistent_namespace_is_eligible() {
        let root = TestRoot::new();
        fs::create_dir_all(root.0.join("original_mods")).unwrap();
        assert!(matches!(
            verify_legacy_restore(&root.0, true),
            Err(ProfileLayoutServiceError::LegacyRestoreIncomplete)
        ));
        assert!(!root.0.join(CONTROL_DIR).exists());

        fs::create_dir_all(root.0.join(".minecraft")).unwrap();
        fs::rename(root.0.join("original_mods"), root.0.join(".minecraft/mods")).unwrap();
        verify_legacy_restore(&root.0, true).unwrap();
        assert!(!root.0.join("original_mods").exists());
    }

    #[test]
    fn mixed_persistent_and_legacy_state_is_detected_without_restoring_live() {
        let root = TestRoot::new();
        fs::create_dir_all(root.0.join(CONTROL_DIR)).unwrap();
        fs::create_dir_all(root.0.join("original_mods")).unwrap();
        assert!(path_exists_no_follow(&root.0.join(CONTROL_DIR)).unwrap());
        assert!(path_exists_no_follow(&root.0.join("original_mods")).unwrap());
        assert!(matches!(
            verify_legacy_restore(&root.0, true),
            Err(ProfileLayoutServiceError::LegacyRestoreIncomplete)
        ));
        assert!(root.0.join("original_mods").is_dir());
    }
}
