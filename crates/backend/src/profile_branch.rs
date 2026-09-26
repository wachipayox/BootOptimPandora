//! Local branch/profile model shared by Settings-facing backend code and the
//! transactional persistent-layout reconciler.
//!
//! Distribution owns only immutable global revisions. Everything in this module is local:
//! lineage pins, local overlays, effective-entry ownership, and the applied-vs-target revision
//! comparison. No API here uploads private overlay data.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const PROFILE_BRANCH_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GlobalRevisionPin {
    pub profile_id: String,
    pub revision_id: String,
    pub manifest_sha256: String,
}

impl GlobalRevisionPin {
    pub fn new(
        profile_id: impl Into<String>,
        revision_id: impl Into<String>,
        manifest_sha256: impl Into<String>,
    ) -> Result<Self, ProfileBranchError> {
        let pin = Self {
            profile_id: profile_id.into(),
            revision_id: revision_id.into(),
            manifest_sha256: manifest_sha256.into().to_ascii_lowercase(),
        };
        pin.validate()?;
        Ok(pin)
    }

    pub fn validate(&self) -> Result<(), ProfileBranchError> {
        if self.profile_id.trim().is_empty() || self.revision_id.trim().is_empty() {
            return Err(ProfileBranchError::InvalidRevisionPin);
        }
        validate_sha256(&self.manifest_sha256).map_err(|_| ProfileBranchError::InvalidRevisionPin)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProfileParentRef {
    GlobalRevision {
        pin: GlobalRevisionPin,
    },
    LocalProfile {
        profile_uuid: Uuid,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileLineage {
    pub schema: u32,
    pub parent: Option<ProfileParentRef>,
    /// Cached global root/ancestor pin used only for local repair eligibility and ancestry
    /// presentation. For a direct global parent it must equal that parent pin.
    pub global_ancestor: Option<GlobalRevisionPin>,
}

impl Default for ProfileLineage {
    fn default() -> Self {
        Self {
            schema: PROFILE_BRANCH_SCHEMA_VERSION,
            parent: None,
            global_ancestor: None,
        }
    }
}

impl ProfileLineage {
    pub fn pure_local() -> Self {
        Self::default()
    }

    pub fn from_global(pin: GlobalRevisionPin) -> Result<Self, ProfileBranchError> {
        pin.validate()?;
        Ok(Self {
            schema: PROFILE_BRANCH_SCHEMA_VERSION,
            parent: Some(ProfileParentRef::GlobalRevision { pin: pin.clone() }),
            global_ancestor: Some(pin),
        })
    }

    pub fn from_local(
        parent_uuid: Uuid,
        global_ancestor: Option<GlobalRevisionPin>,
    ) -> Result<Self, ProfileBranchError> {
        if let Some(pin) = &global_ancestor {
            pin.validate()?;
        }
        Ok(Self {
            schema: PROFILE_BRANCH_SCHEMA_VERSION,
            parent: Some(ProfileParentRef::LocalProfile {
                profile_uuid: parent_uuid,
            }),
            global_ancestor,
        })
    }

    pub fn validate(&self, self_uuid: Uuid) -> Result<(), ProfileBranchError> {
        if self.schema != PROFILE_BRANCH_SCHEMA_VERSION {
            return Err(ProfileBranchError::UnsupportedSchema(self.schema));
        }
        if let Some(pin) = &self.global_ancestor {
            pin.validate()?;
        }
        match &self.parent {
            Some(ProfileParentRef::GlobalRevision { pin }) => {
                pin.validate()?;
                if self.global_ancestor.as_ref() != Some(pin) {
                    return Err(ProfileBranchError::InvalidLineage);
                }
            },
            Some(ProfileParentRef::LocalProfile { profile_uuid }) if *profile_uuid == self_uuid => {
                return Err(ProfileBranchError::LineageCycle);
            },
            _ => {},
        }
        Ok(())
    }

    pub fn can_repair_modpack(&self) -> bool {
        self.global_ancestor.is_some()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileFilePolicy {
    #[default]
    Enforced,
    DefaultOnce,
    UserOwned,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileEntryOwnership {
    #[default]
    Inherited,
    Local,
    UserOwned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProfileEntryOrigin {
    GlobalRevision {
        pin: GlobalRevisionPin,
    },
    LocalProfile {
        profile_uuid: Uuid,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntryMetadata {
    pub logical_identity: String,
    pub source_sha256: String,
    pub origin: ProfileEntryOrigin,
    pub ownership: ProfileEntryOwnership,
    pub policy: ProfileFilePolicy,
}

impl ProfileEntryMetadata {
    pub fn validate(&self) -> Result<(), ProfileBranchError> {
        if self.logical_identity.trim().is_empty() {
            return Err(ProfileBranchError::InvalidLogicalIdentity);
        }
        validate_sha256(&self.source_sha256)?;
        if let ProfileEntryOrigin::GlobalRevision { pin } = &self.origin {
            pin.validate()?;
        }
        Ok(())
    }
}

/// Durable local override representing an explicit child-profile deletion.
///
/// The marker is local by construction. It is intentionally separate from file policy: deleting an
/// enforced parent entry remains a local tombstone rather than being reclassified as default-once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntryTombstone {
    pub policy: ProfileFilePolicy,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileBranchManifest {
    pub lineage: ProfileLineage,
    pub applied_revision: Option<GlobalRevisionPin>,
    #[serde(default)]
    pub entries: BTreeMap<String, ProfileEntryMetadata>,
    #[serde(default)]
    pub tombstones: BTreeMap<String, ProfileEntryTombstone>,
}

impl ProfileBranchManifest {
    pub fn validate(&self, self_uuid: Uuid) -> Result<(), ProfileBranchError> {
        self.lineage.validate(self_uuid)?;
        if let Some(pin) = &self.applied_revision {
            pin.validate()?;
        }
        for (path, entry) in &self.entries {
            validate_profile_relative_path(path)?;
            entry.validate()?;
        }
        for path in self.tombstones.keys() {
            validate_profile_relative_path(path)?;
            if self.entries.contains_key(path) {
                return Err(ProfileBranchError::DuplicatePath(path.clone()));
            }
        }
        Ok(())
    }

    pub fn revision_difference(&self, target: Option<&GlobalRevisionPin>) -> RevisionDifference {
        revision_difference(self.applied_revision.as_ref(), target)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveProfileEntry {
    pub path: String,
    /// Verified immutable object path supplied by the caller. Only changed destinations need one.
    pub source: PathBuf,
    pub metadata: ProfileEntryMetadata,
}

impl EffectiveProfileEntry {
    pub fn validate(&self) -> Result<(), ProfileBranchError> {
        validate_profile_relative_path(&self.path)?;
        self.metadata.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileDeltaChange {
    Upsert(EffectiveProfileEntry),
    Remove {
        path: String,
        origin: ProfileEntryOrigin,
        ownership: ProfileEntryOwnership,
        policy: ProfileFilePolicy,
    },
}

impl ProfileDeltaChange {
    pub fn path(&self) -> &str {
        match self {
            Self::Upsert(entry) => &entry.path,
            Self::Remove { path, .. } => path,
        }
    }

    pub fn validate(&self) -> Result<(), ProfileBranchError> {
        match self {
            Self::Upsert(entry) => entry.validate(),
            Self::Remove { path, origin, .. } => {
                validate_profile_relative_path(path)?;
                if let ProfileEntryOrigin::GlobalRevision { pin } = origin {
                    pin.validate()?;
                }
                Ok(())
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileRevisionDelta {
    pub lineage: ProfileLineage,
    pub target_revision: Option<GlobalRevisionPin>,
    /// Revision-history delta only. Unchanged destinations must not be listed or verified.
    pub changes: Vec<ProfileDeltaChange>,
}

impl ProfileRevisionDelta {
    pub fn validate(&self, profile_uuid: Uuid) -> Result<(), ProfileBranchError> {
        self.lineage.validate(profile_uuid)?;
        if let Some(pin) = &self.target_revision {
            pin.validate()?;
        }
        let mut seen = BTreeMap::<&str, ()>::new();
        for change in &self.changes {
            change.validate()?;
            if seen.insert(change.path(), ()).is_some() {
                return Err(ProfileBranchError::DuplicatePath(change.path().to_owned()));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalOverlayChange {
    Upsert {
        path: String,
        source: PathBuf,
        logical_identity: String,
        source_sha256: String,
        policy: ProfileFilePolicy,
    },
    Remove {
        path: String,
    },
}

/// Resolve an already verified parent effective tree plus a private local overlay.
///
/// This compatibility entry point has no persisted local tombstones. Call
/// `resolve_effective_entries_for_branch` when resolving a previously saved local branch.
pub fn resolve_effective_entries(
    parent_entries: impl IntoIterator<Item = EffectiveProfileEntry>,
    local_profile_uuid: Uuid,
    overlay: &[LocalOverlayChange],
) -> Result<Vec<EffectiveProfileEntry>, ProfileBranchError> {
    resolve_effective_entries_for_branch(parent_entries, local_profile_uuid, &ProfileBranchManifest::default(), overlay)
}

/// Resolve parent entries while honoring durable local tombstones from a saved branch.
///
/// Parent origins are preserved, inherited ownership is explicit, and local overlay bytes stay
/// local. A later explicit local upsert may still replace a tombstoned path in the returned view.
pub fn resolve_effective_entries_for_branch(
    parent_entries: impl IntoIterator<Item = EffectiveProfileEntry>,
    local_profile_uuid: Uuid,
    branch: &ProfileBranchManifest,
    overlay: &[LocalOverlayChange],
) -> Result<Vec<EffectiveProfileEntry>, ProfileBranchError> {
    branch.validate(local_profile_uuid)?;
    let mut resolved = BTreeMap::<String, EffectiveProfileEntry>::new();
    for mut entry in parent_entries {
        entry.validate()?;
        entry.metadata.ownership = ProfileEntryOwnership::Inherited;
        if branch.tombstones.contains_key(&entry.path) {
            continue;
        }
        if resolved.insert(entry.path.clone(), entry).is_some() {
            return Err(ProfileBranchError::DuplicatePath("parent effective tree".to_owned()));
        }
    }

    for change in overlay {
        match change {
            LocalOverlayChange::Upsert {
                path,
                source,
                logical_identity,
                source_sha256,
                policy,
            } => {
                validate_profile_relative_path(path)?;
                validate_sha256(source_sha256)?;
                let entry = EffectiveProfileEntry {
                    path: path.clone(),
                    source: source.clone(),
                    metadata: ProfileEntryMetadata {
                        logical_identity: logical_identity.clone(),
                        source_sha256: source_sha256.to_ascii_lowercase(),
                        origin: ProfileEntryOrigin::LocalProfile {
                            profile_uuid: local_profile_uuid,
                        },
                        ownership: ProfileEntryOwnership::Local,
                        policy: *policy,
                    },
                };
                resolved.insert(path.clone(), entry);
            },
            LocalOverlayChange::Remove { path } => {
                validate_profile_relative_path(path)?;
                resolved.remove(path);
            },
        }
    }
    Ok(resolved.into_values().collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevisionDifference {
    PureLocal,
    Initial {
        target: GlobalRevisionPin,
    },
    Unchanged {
        revision: GlobalRevisionPin,
    },
    Update {
        applied: GlobalRevisionPin,
        target: GlobalRevisionPin,
    },
    Detached {
        applied: GlobalRevisionPin,
    },
}

pub fn revision_difference(
    applied: Option<&GlobalRevisionPin>,
    target: Option<&GlobalRevisionPin>,
) -> RevisionDifference {
    match (applied, target) {
        (None, None) => RevisionDifference::PureLocal,
        (None, Some(target)) => RevisionDifference::Initial { target: target.clone() },
        (Some(applied), Some(target)) if applied == target => RevisionDifference::Unchanged {
            revision: target.clone(),
        },
        (Some(applied), Some(target)) => RevisionDifference::Update {
            applied: applied.clone(),
            target: target.clone(),
        },
        (Some(applied), None) => RevisionDifference::Detached {
            applied: applied.clone(),
        },
    }
}

pub fn validate_profile_relative_path(relative: &str) -> Result<(), ProfileBranchError> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.ends_with('/')
        || relative.contains('\\')
        || relative.contains(':')
        || relative.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(ProfileBranchError::UnsafeRelativePath(relative.to_owned()));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), ProfileBranchError> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ProfileBranchError::InvalidSha256(value.to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ProfileBranchError {
    #[error("unsupported local profile schema {0}")]
    UnsupportedSchema(u32),
    #[error("invalid global revision pin")]
    InvalidRevisionPin,
    #[error("invalid local profile lineage")]
    InvalidLineage,
    #[error("local profile cannot parent itself")]
    LineageCycle,
    #[error("unsafe profile-relative path: {0}")]
    UnsafeRelativePath(String),
    #[error("invalid SHA-256: {0}")]
    InvalidSha256(String),
    #[error("invalid logical identity")]
    InvalidLogicalIdentity,
    #[error("duplicate profile path: {0}")]
    DuplicatePath(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(revision: &str, byte: char) -> GlobalRevisionPin {
        GlobalRevisionPin::new("global-root", revision, byte.to_string().repeat(64)).unwrap()
    }

    #[test]
    fn revision_difference_is_metadata_only_and_stable() {
        let a = pin("r1", 'a');
        let b = pin("r2", 'b');
        assert!(matches!(revision_difference(Some(&a), Some(&a)), RevisionDifference::Unchanged { .. }));
        assert!(matches!(revision_difference(Some(&a), Some(&b)), RevisionDifference::Update { .. }));
    }

    #[test]
    fn local_overlay_preserves_parent_origin_and_marks_local_ownership() {
        let local_uuid = Uuid::from_bytes([7; 16]);
        let global = pin("r1", 'a');
        let parent = EffectiveProfileEntry {
            path: "mods/base.jar".to_owned(),
            source: PathBuf::from("base"),
            metadata: ProfileEntryMetadata {
                logical_identity: "base".to_owned(),
                source_sha256: "1".repeat(64),
                origin: ProfileEntryOrigin::GlobalRevision { pin: global },
                ownership: ProfileEntryOwnership::Inherited,
                policy: ProfileFilePolicy::Enforced,
            },
        };
        let resolved = resolve_effective_entries(
            [parent],
            local_uuid,
            &[LocalOverlayChange::Upsert {
                path: "config/private.json".to_owned(),
                source: PathBuf::from("private"),
                logical_identity: "private".to_owned(),
                source_sha256: "2".repeat(64),
                policy: ProfileFilePolicy::DefaultOnce,
            }],
        )
        .unwrap();

        assert_eq!(resolved.len(), 2);
        let local = resolved.iter().find(|entry| entry.path == "config/private.json").unwrap();
        assert_eq!(local.metadata.ownership, ProfileEntryOwnership::Local);
        assert!(matches!(
            local.metadata.origin,
            ProfileEntryOrigin::LocalProfile { profile_uuid } if profile_uuid == local_uuid
        ));
    }

    #[test]
    fn persisted_local_tombstone_masks_parent_entry_during_resolution() {
        let local_uuid = Uuid::from_bytes([9; 16]);
        let global = pin("r2", 'b');
        let parent = EffectiveProfileEntry {
            path: "mods/removed.jar".to_owned(),
            source: PathBuf::from("removed"),
            metadata: ProfileEntryMetadata {
                logical_identity: "removed".to_owned(),
                source_sha256: "3".repeat(64),
                origin: ProfileEntryOrigin::GlobalRevision { pin: global },
                ownership: ProfileEntryOwnership::Inherited,
                policy: ProfileFilePolicy::Enforced,
            },
        };
        let mut branch = ProfileBranchManifest::default();
        branch.tombstones.insert(
            "mods/removed.jar".to_owned(),
            ProfileEntryTombstone {
                policy: ProfileFilePolicy::Enforced,
            },
        );

        let resolved = resolve_effective_entries_for_branch([parent], local_uuid, &branch, &[]).unwrap();
        assert!(resolved.is_empty());
    }

    #[test]
    fn pure_local_lineage_cannot_repair_modpack() {
        assert!(!ProfileLineage::pure_local().can_repair_modpack());
        assert!(ProfileLineage::from_global(pin("r1", 'a')).unwrap().can_repair_modpack());
    }
}
