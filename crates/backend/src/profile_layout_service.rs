//! Backend service seam for the persistent-profile flow.
//!
//! This is intentionally not called by Start. Future install/update and signed service/channel
//! code can call this only after it has resolved a desired local managed set.

use std::{io::ErrorKind, path::Path};

use bridge::instance::InstanceID;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    BackendState,
    profile_branch::{
        EffectiveProfileEntry, GlobalRevisionPin, ProfileBranchManifest, ProfileLineage, ProfileRevisionDelta,
        RevisionDifference,
    },
    profile_layout_flow::{
        DesiredManagedFile, PersistentProfileLayout, ProfileLayoutFlowError, ProfileLayoutState, ReconcileOutcome,
    },
    profile_layout_identity::{ProfileIdentityError, acquire_or_initialize_profile_lock},
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
    #[error("persistent profile {0} is busy in another launcher process")]
    ProfileBusy(Uuid),
    #[error("persistent profile identity/lock could not be established safely: {0}")]
    ProfileLock(String),
    #[error(transparent)]
    Layout(#[from] ProfileLayoutFlowError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileBranchSnapshot {
    pub profile_uuid: Uuid,
    pub generation: Option<u64>,
    pub layout_state: ProfileLayoutState,
    pub stock_fallback: bool,
    pub branch: ProfileBranchManifest,
    pub revision_difference: RevisionDifference,
    pub repair_modpack_available: bool,
}

impl BackendState {
    /// Returns the local branch state for Settings without walking/hash-verifying the managed tree.
    /// Recovery, if needed, runs while the per-profile OS lock is held.
    pub fn persistent_profile_branch_status(
        &self,
        id: InstanceID,
        target_revision: Option<&GlobalRevisionPin>,
    ) -> Result<ProfileBranchSnapshot, ProfileLayoutServiceError> {
        self.with_stopped_profile_layout(id, |layout| {
            let branch = layout.branch_manifest()?;
            let status = layout.status().clone();
            Ok(ProfileBranchSnapshot {
                profile_uuid: layout.profile_uuid(),
                generation: status.generation,
                layout_state: status.state,
                stock_fallback: status.stock_fallback,
                revision_difference: branch.revision_difference(target_revision),
                repair_modpack_available: branch.lineage.can_repair_modpack(),
                branch,
            })
        })
    }

    /// Persist a global-revision pin or local-parent reference without touching .minecraft files.
    pub fn configure_persistent_profile_lineage(
        &self,
        id: InstanceID,
        lineage: ProfileLineage,
    ) -> Result<ReconcileOutcome, ProfileLayoutServiceError> {
        self.with_stopped_profile_layout(id, move |layout| layout.configure_branch_lineage(lineage))
    }

    /// Apply a verified effective revision-history delta. Unchanged destinations are not observed.
    pub fn apply_persistent_profile_delta(
        &self,
        id: InstanceID,
        delta: &ProfileRevisionDelta,
    ) -> Result<ReconcileOutcome, ProfileLayoutServiceError> {
        self.with_stopped_profile_layout(id, |layout| layout.reconcile_revision_delta(delta))
    }

    /// Explicit full managed-tree parity. Purely local profiles are rejected by the layout layer.
    /// This is intentionally separate from the existing Repair game files backend.
    pub fn repair_persistent_modpack(
        &self,
        id: InstanceID,
        effective: &[EffectiveProfileEntry],
    ) -> Result<ReconcileOutcome, ProfileLayoutServiceError> {
        self.with_stopped_profile_layout(id, |layout| layout.repair_modpack(effective))
    }

    /// Legacy full desired-set seam retained for callers that have not moved to revision deltas.
    /// It is not a Start hook and should not be used for stable/no-op updates.
    pub fn reconcile_persistent_profile_layout(
        &self,
        id: InstanceID,
        desired: &[DesiredManagedFile],
    ) -> Result<ReconcileOutcome, ProfileLayoutServiceError> {
        self.with_stopped_profile_layout(id, |layout| layout.reconcile(desired))
    }

    fn with_stopped_profile_layout<T>(
        &self,
        id: InstanceID,
        action: impl FnOnce(&mut PersistentProfileLayout) -> Result<T, ProfileLayoutFlowError>,
    ) -> Result<T, ProfileLayoutServiceError> {
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

        // The state write guard prevents Start/state mutation while recovery/reconcile runs.
        // The OS lock isolates a profile UUID from a second launcher process.
        let _profile_lock = acquire_or_initialize_profile_lock(&instance.root_path).map_err(map_lock_error)?;
        let mut layout = PersistentProfileLayout::open(&instance.root_path)?;
        Ok(action(&mut layout)?)
    }
}

fn map_lock_error(error: ProfileIdentityError) -> ProfileLayoutServiceError {
    match error {
        ProfileIdentityError::Busy(profile_uuid) => ProfileLayoutServiceError::ProfileBusy(profile_uuid),
        other => ProfileLayoutServiceError::ProfileLock(other.to_string()),
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
