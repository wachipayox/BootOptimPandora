//! Transactional persistent-profile client flow.
//!
//! This module is deliberately not selected by Start. It is the stopped-profile
//! reconciliation/publication boundary that a later install/update/service hook can call.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    profile_branch::{
        EffectiveProfileEntry, ProfileBranchError, ProfileBranchManifest, ProfileDeltaChange, ProfileEntryOrigin,
        ProfileEntryOwnership, ProfileEntryTombstone, ProfileFilePolicy, ProfileLineage, ProfileRevisionDelta,
    },
    profile_layout_ownership::{
        ConflictReason, LiveEntry, ManagedEntry, PathReconcileInput, ReconcileAction, plan_profile,
    },
};

const CONTROL_DIR: &str = ".pandora-layout-v1";
const IDENTITY_FILE: &str = "identity.json";
const MANIFEST_FILE: &str = "manifest.json";
const JOURNAL_FILE: &str = "journal.json";
const PUBLISHING_MARKER: &str = "publishing.marker";
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileLayoutState {
    Ready,
    NeedsReconcile,
    Planning,
    Staging,
    Prepared,
    Publishing,
    Recovering,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedManifestEntry {
    pub logical_identity: String,
    pub source_hash: String,
    pub applied_hash: String,
}

impl ManagedManifestEntry {
    fn ownership_entry(&self) -> ManagedEntry {
        ManagedEntry::new(self.logical_identity.clone(), self.applied_hash.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileLayoutManifest {
    pub schema: u32,
    pub profile_uuid: Uuid,
    pub generation: u64,
    pub state: ProfileLayoutState,
    pub managed_input_fingerprint: String,
    #[serde(default)]
    pub sync_identity: String,
    #[serde(default)]
    pub sandbox_policy: String,
    #[serde(default)]
    pub managed_entries: BTreeMap<String, ManagedManifestEntry>,
    #[serde(default)]
    pub branch: ProfileBranchManifest,
    #[serde(default)]
    pub transaction_id: Option<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ProfileIdentity {
    schema: u32,
    profile_uuid: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalOpKind {
    Install,
    Replace,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct JournalOperation {
    path: String,
    kind: JournalOpKind,
    old_hash: Option<String>,
    new_hash: Option<String>,
    #[serde(default)]
    retain_conflict: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PublicationJournal {
    schema: u32,
    profile_uuid: Uuid,
    transaction_id: Uuid,
    from_generation: Option<u64>,
    target_generation: u64,
    previous_manifest_sha256: Option<String>,
    target_manifest_sha256: String,
    state: ProfileLayoutState,
    operations: Vec<JournalOperation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ConflictRecord {
    schema: u32,
    profile_uuid: Uuid,
    managed_input_fingerprint: String,
    conflicts: Vec<String>,
    requires_stock_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileLayoutStatus {
    pub profile_uuid: Uuid,
    pub state: ProfileLayoutState,
    pub generation: Option<u64>,
    pub stock_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesiredManagedFile {
    /// Canonical instance-relative path using `/` separators, e.g. `mods/example.jar`.
    pub path: String,
    /// Immutable source blob. This flow copies it into private staging; it is never renamed.
    pub source: PathBuf,
    pub logical_identity: String,
    /// Lower/upper case is accepted; comparisons normalize to lowercase.
    pub source_sha256: String,
}

impl DesiredManagedFile {
    pub fn new(
        path: impl Into<String>,
        source: impl Into<PathBuf>,
        logical_identity: impl Into<String>,
        source_sha256: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            source: source.into(),
            logical_identity: logical_identity.into(),
            source_sha256: source_sha256.into().to_ascii_lowercase(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconcileOutcome {
    Ready {
        profile_uuid: Uuid,
        generation: u64,
    },
    NeedsReconcile {
        profile_uuid: Uuid,
        generation: Option<u64>,
        conflicts: Vec<String>,
        stock_fallback: bool,
    },
}

#[derive(Debug, Error)]
pub enum ProfileLayoutFlowError {
    #[error("profile layout I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("profile layout serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unsafe filesystem object at {0:?}")]
    UnsafeFilesystem(PathBuf),
    #[error("unsafe managed relative path: {0}")]
    UnsafeRelativePath(String),
    #[error("profile layout identity mismatch")]
    IdentityMismatch,
    #[error("profile layout journal/manifest transaction is ambiguous")]
    AmbiguousTransaction,
    #[error("managed source hash mismatch for {0}")]
    SourceHashMismatch(String),
    #[error("managed destination changed after ownership proof: {0}")]
    OwnershipChanged(String),
    #[error(transparent)]
    Branch(#[from] ProfileBranchError),
    #[error("Repair modpack is unavailable for a purely local profile")]
    RepairUnavailable,
}

#[derive(Debug)]
pub struct PersistentProfileLayout {
    instance_root: PathBuf,
    control_root: PathBuf,
    profile_uuid: Uuid,
    status: ProfileLayoutStatus,
}

impl PersistentProfileLayout {
    /// Opens one profile namespace and recovers any durable transaction before exposing status.
    /// Callers performing legacy migration must restore `original_mods` before calling this.
    pub fn open(instance_root: &Path) -> Result<Self, ProfileLayoutFlowError> {
        ensure_plain_existing_directory(instance_root)?;
        let control_root = instance_root.join(CONTROL_DIR);
        ensure_plain_directory(&control_root)?;
        let identity = load_or_create_identity(&control_root)?;
        let mut this = Self {
            instance_root: instance_root.to_path_buf(),
            control_root,
            profile_uuid: identity.profile_uuid,
            status: ProfileLayoutStatus {
                profile_uuid: identity.profile_uuid,
                state: ProfileLayoutState::NeedsReconcile,
                generation: None,
                stock_fallback: true,
            },
        };
        this.recover_if_needed()?;
        this.refresh_status()?;
        Ok(this)
    }

    pub fn profile_uuid(&self) -> Uuid {
        self.profile_uuid
    }

    pub fn status(&self) -> &ProfileLayoutStatus {
        &self.status
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.control_root.join(MANIFEST_FILE)
    }

    pub fn journal_path(&self) -> PathBuf {
        self.control_root.join(JOURNAL_FILE)
    }

    fn staging_root(&self) -> PathBuf {
        self.control_root.join("staging")
    }

    fn backup_root(&self) -> PathBuf {
        self.control_root.join("backup")
    }

    fn conflicts_root(&self) -> PathBuf {
        self.control_root.join("conflicts")
    }

    fn conflict_copies_root(&self) -> PathBuf {
        self.control_root.join("conflict-copies")
    }

    fn staging_dir(&self, tx: Uuid) -> PathBuf {
        self.staging_root().join(tx.to_string())
    }

    fn backup_dir(&self, tx: Uuid) -> PathBuf {
        self.backup_root().join(tx.to_string())
    }

    fn live_root(&self) -> PathBuf {
        self.instance_root.join(".minecraft")
    }

    pub fn reconcile(&mut self, desired: &[DesiredManagedFile]) -> Result<ReconcileOutcome, ProfileLayoutFlowError> {
        self.recover_if_needed()?;
        self.status.state = ProfileLayoutState::Planning;

        let current = self.read_manifest_optional()?;
        if let Some(manifest) = &current {
            self.validate_manifest(manifest)?;
        }

        let desired_map = canonical_desired(desired)?;
        let fingerprint = managed_fingerprint(&desired_map);
        let previous_entries = current.as_ref().map(|manifest| manifest.managed_entries.clone()).unwrap_or_default();

        let mut paths = BTreeSet::new();
        paths.extend(previous_entries.keys().cloned());
        paths.extend(desired_map.keys().cloned());

        let mut inputs = Vec::with_capacity(paths.len());
        for path in &paths {
            let previous = previous_entries.get(path).map(ManagedManifestEntry::ownership_entry);
            let desired_entry = desired_map
                .get(path)
                .map(|entry| ManagedEntry::new(entry.logical_identity.clone(), entry.source_sha256.clone()));
            let live = observe_live(&self.live_root(), path)?;
            inputs.push(PathReconcileInput {
                path: path.clone(),
                previous,
                live,
                desired: desired_entry,
            });
        }

        let plan = plan_profile(self.profile_uuid.to_string(), inputs);
        let mut conflicts = Vec::new();
        for planned in &plan.paths {
            if let Some(reason) = planned.decision.conflict {
                conflicts.push(format!("{}:{}", planned.path, conflict_name(reason)));
            }
        }

        if !conflicts.is_empty() || plan.requires_stock_fallback {
            self.persist_conflict(&fingerprint, &conflicts, plan.requires_stock_fallback)?;
            self.status.state = ProfileLayoutState::NeedsReconcile;
            self.status.generation = current.as_ref().map(|manifest| manifest.generation);
            self.status.stock_fallback = true;
            return Ok(ReconcileOutcome::NeedsReconcile {
                profile_uuid: self.profile_uuid,
                generation: self.status.generation,
                conflicts,
                stock_fallback: true,
            });
        }

        self.status.state = ProfileLayoutState::Staging;
        let tx = new_uuid();
        let generation = current.as_ref().map_or(1, |manifest| manifest.generation.saturating_add(1));
        let mut managed_entries = BTreeMap::new();
        for (path, desired_entry) in &desired_map {
            managed_entries.insert(
                path.clone(),
                ManagedManifestEntry {
                    logical_identity: desired_entry.logical_identity.clone(),
                    source_hash: desired_entry.source_sha256.clone(),
                    applied_hash: desired_entry.source_sha256.clone(),
                },
            );
        }

        let target = ProfileLayoutManifest {
            schema: SCHEMA_VERSION,
            profile_uuid: self.profile_uuid,
            generation,
            state: ProfileLayoutState::Ready,
            managed_input_fingerprint: fingerprint,
            sync_identity: current.as_ref().map(|m| m.sync_identity.clone()).unwrap_or_default(),
            sandbox_policy: current.as_ref().map(|m| m.sandbox_policy.clone()).unwrap_or_default(),
            managed_entries,
            branch: current.as_ref().map(|m| m.branch.clone()).unwrap_or_default(),
            transaction_id: Some(tx),
        };

        let operations = build_operations(&plan.paths, &previous_entries, &desired_map);
        self.prepare_transaction(tx, current.as_ref(), &target, &operations, &desired_map)?;
        self.status.state = ProfileLayoutState::Prepared;
        self.publish_transaction(tx)?;
        self.clear_conflicts()?;
        self.refresh_status()?;
        Ok(ReconcileOutcome::Ready {
            profile_uuid: self.profile_uuid,
            generation,
        })
    }

    /// Returns the transactionally committed local branch metadata without walking live files.
    pub fn branch_manifest(&self) -> Result<ProfileBranchManifest, ProfileLayoutFlowError> {
        let manifest = self.read_manifest_optional()?;
        let branch = manifest.map(|m| m.branch).unwrap_or_default();
        branch.validate(self.profile_uuid)?;
        Ok(branch)
    }

    /// Changes only lineage metadata. No managed destination is inspected.
    pub fn configure_branch_lineage(
        &mut self,
        lineage: ProfileLineage,
    ) -> Result<ReconcileOutcome, ProfileLayoutFlowError> {
        let branch = self.branch_manifest()?;
        let delta = ProfileRevisionDelta {
            lineage,
            target_revision: branch.applied_revision,
            changes: Vec::new(),
        };
        self.reconcile_revision_delta(&delta)
    }

    /// Applies an already-resolved revision-history delta. Only paths listed in `delta.changes`
    /// are observed, and only enforced changed paths are hashed. A stable/no-op revision update
    /// performs no destination I/O.
    pub fn reconcile_revision_delta(
        &mut self,
        delta: &ProfileRevisionDelta,
    ) -> Result<ReconcileOutcome, ProfileLayoutFlowError> {
        self.reconcile_revision_delta_inner(delta, false)
    }

    /// Explicit full managed-tree parity for a global or globally derived profile.
    ///
    /// This still does not enumerate unrelated .minecraft files: it checks every resolved managed
    /// destination plus previously managed enforced destinations. Local additions remain untouched.
    pub fn repair_modpack(
        &mut self,
        effective: &[EffectiveProfileEntry],
    ) -> Result<ReconcileOutcome, ProfileLayoutFlowError> {
        let current = self.read_manifest_optional()?;
        let branch = current.as_ref().map(|m| m.branch.clone()).unwrap_or_default();
        branch.validate(self.profile_uuid)?;
        if !branch.lineage.can_repair_modpack() {
            return Err(ProfileLayoutFlowError::RepairUnavailable);
        }

        let mut target_paths = BTreeSet::new();
        let mut changes = Vec::with_capacity(effective.len());
        for entry in effective {
            entry.validate()?;
            target_paths.insert(entry.path.clone());
            changes.push(ProfileDeltaChange::Upsert(entry.clone()));
        }
        for (path, metadata) in &branch.entries {
            if !target_paths.contains(path) && metadata.policy == ProfileFilePolicy::Enforced {
                changes.push(ProfileDeltaChange::Remove {
                    path: path.clone(),
                    origin: metadata.origin.clone(),
                    ownership: metadata.ownership,
                    policy: metadata.policy,
                });
            }
        }

        // Schema-v1 manifests created before branch metadata was added can still contain valid
        // destructive ownership proof in managed_entries. Repair is the explicit full-parity path,
        // so complete removals from that persisted manifest state without enumerating .minecraft.
        if let Some(manifest) = &current {
            let legacy_origin = branch
                .applied_revision
                .clone()
                .or_else(|| branch.lineage.global_ancestor.clone())
                .ok_or(ProfileLayoutFlowError::RepairUnavailable)?;
            let already_removed = changes
                .iter()
                .filter_map(|change| match change {
                    ProfileDeltaChange::Remove { path, .. } => Some(path.clone()),
                    ProfileDeltaChange::Upsert(_) => None,
                })
                .collect::<BTreeSet<_>>();
            for path in manifest.managed_entries.keys() {
                if target_paths.contains(path)
                    || already_removed.contains(path)
                    || branch.entries.contains_key(path)
                    || branch.tombstones.contains_key(path)
                {
                    continue;
                }
                changes.push(ProfileDeltaChange::Remove {
                    path: path.clone(),
                    origin: ProfileEntryOrigin::GlobalRevision {
                        pin: legacy_origin.clone(),
                    },
                    ownership: ProfileEntryOwnership::Inherited,
                    policy: ProfileFilePolicy::Enforced,
                });
            }
        }
        let delta = ProfileRevisionDelta {
            lineage: branch.lineage.clone(),
            target_revision: branch.applied_revision.clone(),
            changes,
        };
        self.reconcile_revision_delta_inner(&delta, true)
    }

    fn reconcile_revision_delta_inner(
        &mut self,
        delta: &ProfileRevisionDelta,
        repair: bool,
    ) -> Result<ReconcileOutcome, ProfileLayoutFlowError> {
        self.recover_if_needed()?;
        delta.validate(self.profile_uuid)?;
        self.status.state = ProfileLayoutState::Planning;

        let current = self.read_manifest_optional()?;
        if let Some(manifest) = &current {
            self.validate_manifest(manifest)?;
            manifest.branch.validate(self.profile_uuid)?;
        }
        let current_generation = current.as_ref().map(|m| m.generation);
        let mut branch = current.as_ref().map(|m| m.branch.clone()).unwrap_or_default();

        if !repair
            && current.is_some()
            && delta.changes.is_empty()
            && branch.lineage == delta.lineage
            && branch.applied_revision == delta.target_revision
        {
            self.status.state = ProfileLayoutState::Ready;
            self.status.generation = current_generation;
            self.status.stock_fallback = false;
            return Ok(ReconcileOutcome::Ready {
                profile_uuid: self.profile_uuid,
                generation: current_generation.unwrap_or(0),
            });
        }

        let mut managed_entries = current.as_ref().map(|m| m.managed_entries.clone()).unwrap_or_default();
        let mut operations = Vec::<JournalOperation>::new();
        let mut desired = BTreeMap::<String, CanonicalDesired>::new();
        let mut blocking_conflicts = Vec::<String>::new();

        for change in &delta.changes {
            let path = change.path().to_owned();
            let current_metadata = branch.entries.get(&path).cloned();
            let has_local_tombstone = branch.tombstones.contains_key(&path);

            let incoming_is_inherited = match change {
                ProfileDeltaChange::Upsert(entry) => entry.metadata.ownership == ProfileEntryOwnership::Inherited,
                ProfileDeltaChange::Remove { ownership, .. } => *ownership == ProfileEntryOwnership::Inherited,
            };
            if incoming_is_inherited
                && (has_local_tombstone
                    || current_metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.ownership != ProfileEntryOwnership::Inherited))
            {
                // A local override/tombstone owns this destination; ancestor history no longer
                // flows through it.
                continue;
            }

            match change {
                ProfileDeltaChange::Upsert(entry) => {
                    let mut metadata = entry.metadata.clone();
                    metadata.validate()?;
                    if metadata.ownership != ProfileEntryOwnership::Inherited {
                        branch.tombstones.remove(&path);
                    }
                    match metadata.policy {
                        ProfileFilePolicy::Enforced => {
                            let live = observe_live(&self.live_root(), &path)?;
                            match live {
                                LiveEntry::Missing => {
                                    operations.push(JournalOperation {
                                        path: path.clone(),
                                        kind: JournalOpKind::Install,
                                        old_hash: None,
                                        new_hash: Some(metadata.source_sha256.to_ascii_lowercase()),
                                        retain_conflict: false,
                                    });
                                    desired.insert(
                                        path.clone(),
                                        CanonicalDesired {
                                            source: entry.source.clone(),
                                            logical_identity: metadata.logical_identity.clone(),
                                            source_sha256: metadata.source_sha256.to_ascii_lowercase(),
                                        },
                                    );
                                },
                                LiveEntry::File { hash } => {
                                    if hash != metadata.source_sha256.to_ascii_lowercase() {
                                        let previous_hash =
                                            managed_entries.get(&path).map(|old| old.applied_hash.as_str());
                                        operations.push(JournalOperation {
                                            path: path.clone(),
                                            kind: JournalOpKind::Replace,
                                            old_hash: Some(hash.clone()),
                                            new_hash: Some(metadata.source_sha256.to_ascii_lowercase()),
                                            retain_conflict: previous_hash != Some(hash.as_str()),
                                        });
                                        desired.insert(
                                            path.clone(),
                                            CanonicalDesired {
                                                source: entry.source.clone(),
                                                logical_identity: metadata.logical_identity.clone(),
                                                source_sha256: metadata.source_sha256.to_ascii_lowercase(),
                                            },
                                        );
                                    }
                                },
                                LiveEntry::Directory | LiveEntry::ReparsePoint | LiveEntry::OtherType => {
                                    blocking_conflicts.push(format!("{path}:unexpected_type"));
                                    continue;
                                },
                            }
                            managed_entries.insert(
                                path.clone(),
                                ManagedManifestEntry {
                                    logical_identity: metadata.logical_identity.clone(),
                                    source_hash: metadata.source_sha256.to_ascii_lowercase(),
                                    applied_hash: metadata.source_sha256.to_ascii_lowercase(),
                                },
                            );
                            branch.entries.insert(path, metadata);
                        },
                        ProfileFilePolicy::DefaultOnce | ProfileFilePolicy::UserOwned => {
                            if current_metadata.is_none() {
                                match observe_live_kind(&self.live_root(), &path)? {
                                    LiveEntryKind::Missing => {
                                        operations.push(JournalOperation {
                                            path: path.clone(),
                                            kind: JournalOpKind::Install,
                                            old_hash: None,
                                            new_hash: Some(metadata.source_sha256.to_ascii_lowercase()),
                                            retain_conflict: false,
                                        });
                                        desired.insert(
                                            path.clone(),
                                            CanonicalDesired {
                                                source: entry.source.clone(),
                                                logical_identity: metadata.logical_identity.clone(),
                                                source_sha256: metadata.source_sha256.to_ascii_lowercase(),
                                            },
                                        );
                                    },
                                    LiveEntryKind::File => {},
                                    LiveEntryKind::UnexpectedType => {
                                        blocking_conflicts.push(format!("{path}:unexpected_type"));
                                        continue;
                                    },
                                }
                                metadata.ownership = ProfileEntryOwnership::UserOwned;
                                branch.entries.insert(path.clone(), metadata);
                            }
                            // Once initialized, default_once/user_owned is local and never used as
                            // destructive ownership proof.
                            managed_entries.remove(&path);
                        },
                    }
                },
                ProfileDeltaChange::Remove {
                    path,
                    origin: _,
                    ownership,
                    policy,
                } => {
                    if *ownership != ProfileEntryOwnership::Inherited {
                        match observe_live(&self.live_root(), path)? {
                            LiveEntry::Missing => {},
                            LiveEntry::File { hash } => {
                                operations.push(JournalOperation {
                                    path: path.clone(),
                                    kind: JournalOpKind::Remove,
                                    old_hash: Some(hash),
                                    new_hash: None,
                                    retain_conflict: false,
                                });
                            },
                            LiveEntry::Directory | LiveEntry::ReparsePoint | LiveEntry::OtherType => {
                                blocking_conflicts.push(format!("{path}:unexpected_type"));
                                continue;
                            },
                        }
                        managed_entries.remove(path);
                        branch.entries.remove(path);
                        branch.tombstones.insert(path.clone(), ProfileEntryTombstone { policy: *policy });
                        continue;
                    }

                    match policy {
                        ProfileFilePolicy::Enforced => {
                            if let Some(previous) = managed_entries.get(path) {
                                match observe_live(&self.live_root(), path)? {
                                    LiveEntry::Missing => {},
                                    LiveEntry::File { hash } => {
                                        operations.push(JournalOperation {
                                            path: path.clone(),
                                            kind: JournalOpKind::Remove,
                                            old_hash: Some(hash.clone()),
                                            new_hash: None,
                                            retain_conflict: hash != previous.applied_hash,
                                        });
                                    },
                                    LiveEntry::Directory | LiveEntry::ReparsePoint | LiveEntry::OtherType => {
                                        blocking_conflicts.push(format!("{path}:unexpected_type"));
                                        continue;
                                    },
                                }
                            }
                            managed_entries.remove(path);
                            branch.entries.remove(path);
                        },
                        ProfileFilePolicy::DefaultOnce | ProfileFilePolicy::UserOwned => {
                            if let Some(mut existing) = branch.entries.get(path).cloned() {
                                existing.origin = ProfileEntryOrigin::LocalProfile {
                                    profile_uuid: self.profile_uuid,
                                };
                                existing.ownership = ProfileEntryOwnership::UserOwned;
                                branch.entries.insert(path.clone(), existing);
                            }
                            managed_entries.remove(path);
                        },
                    }
                },
            }
        }

        if !blocking_conflicts.is_empty() {
            let fingerprint = sha256_bytes(&serde_json::to_vec(&branch)?);
            self.persist_conflict(&fingerprint, &blocking_conflicts, true)?;
            self.status.state = ProfileLayoutState::NeedsReconcile;
            self.status.generation = current_generation;
            self.status.stock_fallback = true;
            return Ok(ReconcileOutcome::NeedsReconcile {
                profile_uuid: self.profile_uuid,
                generation: current_generation,
                conflicts: blocking_conflicts,
                stock_fallback: true,
            });
        }

        branch.lineage = delta.lineage.clone();
        branch.applied_revision = delta.target_revision.clone();
        branch.validate(self.profile_uuid)?;

        let tx = new_uuid();
        let generation = current_generation.map_or(1, |g| g.saturating_add(1));
        let target = ProfileLayoutManifest {
            schema: SCHEMA_VERSION,
            profile_uuid: self.profile_uuid,
            generation,
            state: ProfileLayoutState::Ready,
            managed_input_fingerprint: sha256_bytes(&serde_json::to_vec(&branch)?),
            sync_identity: current.as_ref().map(|m| m.sync_identity.clone()).unwrap_or_default(),
            sandbox_policy: current.as_ref().map(|m| m.sandbox_policy.clone()).unwrap_or_default(),
            managed_entries,
            branch,
            transaction_id: Some(tx),
        };

        self.prepare_transaction(tx, current.as_ref(), &target, &operations, &desired)?;
        self.status.state = ProfileLayoutState::Prepared;
        self.publish_transaction(tx)?;
        self.clear_conflicts()?;
        self.refresh_status()?;
        Ok(ReconcileOutcome::Ready {
            profile_uuid: self.profile_uuid,
            generation,
        })
    }

    fn prepare_transaction(
        &self,
        tx: Uuid,
        current: Option<&ProfileLayoutManifest>,
        target: &ProfileLayoutManifest,
        operations: &[JournalOperation],
        desired: &BTreeMap<String, CanonicalDesired>,
    ) -> Result<(), ProfileLayoutFlowError> {
        if self.journal_path().exists() {
            return Err(ProfileLayoutFlowError::AmbiguousTransaction);
        }
        ensure_plain_directory(&self.staging_dir(tx))?;
        ensure_plain_directory(&self.backup_dir(tx))?;

        for operation in operations {
            if matches!(operation.kind, JournalOpKind::Install | JournalOpKind::Replace) {
                let desired_entry = desired.get(&operation.path).ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?;
                stage_source(
                    &desired_entry.source,
                    &self.staging_live_path(tx, &operation.path)?,
                    &desired_entry.source_sha256,
                )?;
            }
        }

        let target_bytes = serde_json::to_vec_pretty(target)?;
        write_new_synced(&self.staging_dir(tx).join(MANIFEST_FILE), &target_bytes)?;
        let previous_manifest_sha256 = match current {
            Some(_) => Some(hash_regular_file(&self.manifest_path())?),
            None => None,
        };
        let journal = PublicationJournal {
            schema: SCHEMA_VERSION,
            profile_uuid: self.profile_uuid,
            transaction_id: tx,
            from_generation: current.map(|manifest| manifest.generation),
            target_generation: target.generation,
            previous_manifest_sha256,
            target_manifest_sha256: sha256_bytes(&target_bytes),
            state: ProfileLayoutState::Prepared,
            operations: operations.to_vec(),
        };
        write_new_synced(&self.journal_path(), &serde_json::to_vec_pretty(&journal)?)?;
        Ok(())
    }

    fn publish_transaction(&mut self, tx: Uuid) -> Result<(), ProfileLayoutFlowError> {
        let journal = self.read_journal()?;
        self.validate_journal(&journal)?;
        if journal.transaction_id != tx {
            return Err(ProfileLayoutFlowError::AmbiguousTransaction);
        }
        verify_transaction_inputs(self, &journal)?;

        write_new_synced(&self.staging_dir(tx).join(PUBLISHING_MARKER), b"publishing\n")?;
        self.status.state = ProfileLayoutState::Publishing;

        for operation in &journal.operations {
            self.apply_operation(tx, operation)?;
        }
        verify_target_live(self, &journal)?;
        self.commit_manifest(&journal)?;
        verify_target_live(self, &journal)?;
        self.cleanup_committed(&journal)?;
        Ok(())
    }

    fn apply_operation(&self, tx: Uuid, operation: &JournalOperation) -> Result<(), ProfileLayoutFlowError> {
        let live = self.live_path(&operation.path)?;
        ensure_plain_live_parent(&self.live_root(), &operation.path)?;
        match operation.kind {
            JournalOpKind::Install => {
                if live.exists() {
                    return Err(ProfileLayoutFlowError::OwnershipChanged(operation.path.clone()));
                }
                let staged = self.staging_live_path(tx, &operation.path)?;
                verify_hash(
                    &staged,
                    operation.new_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?,
                )?;
                fs::rename(&staged, &live)?;
                sync_parent(&live)?;
            },
            JournalOpKind::Replace => {
                verify_hash(&live, operation.old_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?)?;
                let backup = self.backup_live_path(tx, &operation.path)?;
                create_plain_parent(&backup)?;
                if backup.exists() {
                    return Err(ProfileLayoutFlowError::AmbiguousTransaction);
                }
                fs::rename(&live, &backup)?;
                sync_parent(&live)?;
                let staged = self.staging_live_path(tx, &operation.path)?;
                verify_hash(
                    &staged,
                    operation.new_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?,
                )?;
                fs::rename(&staged, &live)?;
                sync_parent(&live)?;
            },
            JournalOpKind::Remove => {
                verify_hash(&live, operation.old_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?)?;
                let backup = self.backup_live_path(tx, &operation.path)?;
                create_plain_parent(&backup)?;
                if backup.exists() {
                    return Err(ProfileLayoutFlowError::AmbiguousTransaction);
                }
                fs::rename(&live, &backup)?;
                sync_parent(&live)?;
            },
        }
        Ok(())
    }

    fn commit_manifest(&self, journal: &PublicationJournal) -> Result<(), ProfileLayoutFlowError> {
        let staged_manifest = self.staging_dir(journal.transaction_id).join(MANIFEST_FILE);
        verify_hash(&staged_manifest, &journal.target_manifest_sha256)?;
        let manifest = self.manifest_path();
        let backup_manifest = self.backup_dir(journal.transaction_id).join(MANIFEST_FILE);

        match &journal.previous_manifest_sha256 {
            Some(expected) => {
                verify_hash(&manifest, expected)?;
                if backup_manifest.exists() {
                    return Err(ProfileLayoutFlowError::AmbiguousTransaction);
                }
                fs::rename(&manifest, &backup_manifest)?;
                sync_parent(&manifest)?;
            },
            None => {
                if manifest.exists() {
                    return Err(ProfileLayoutFlowError::AmbiguousTransaction);
                }
            },
        }

        fs::rename(&staged_manifest, &manifest)?;
        sync_parent(&manifest)?;
        verify_hash(&manifest, &journal.target_manifest_sha256)
    }

    fn recover_if_needed(&mut self) -> Result<(), ProfileLayoutFlowError> {
        if !self.journal_path().exists() {
            return Ok(());
        }
        self.status.state = ProfileLayoutState::Recovering;
        self.status.stock_fallback = true;
        let journal = self.read_journal()?;
        self.validate_journal(&journal)?;

        if file_hash_matches(&self.manifest_path(), &journal.target_manifest_sha256)? {
            verify_target_live(self, &journal)?;
            self.cleanup_committed(&journal)?;
            return Ok(());
        }

        if let Some(previous) = &journal.previous_manifest_sha256 {
            let manifest_matches_previous = file_hash_matches(&self.manifest_path(), previous)?;
            let backup_matches_previous =
                file_hash_matches(&self.backup_dir(journal.transaction_id).join(MANIFEST_FILE), previous)?;
            if !manifest_matches_previous && !backup_matches_previous {
                return Err(ProfileLayoutFlowError::AmbiguousTransaction);
            }
        } else if self.manifest_path().exists() {
            return Err(ProfileLayoutFlowError::AmbiguousTransaction);
        }

        self.rollback_live(&journal)?;
        self.restore_previous_manifest(&journal)?;
        self.cleanup_uncommitted(&journal)?;
        Ok(())
    }

    fn rollback_live(&self, journal: &PublicationJournal) -> Result<(), ProfileLayoutFlowError> {
        for operation in journal.operations.iter().rev() {
            let live = self.live_path(&operation.path)?;
            let backup = self.backup_live_path(journal.transaction_id, &operation.path)?;
            match operation.kind {
                JournalOpKind::Install => {
                    if !live.exists() {
                        continue;
                    }
                    let expected = operation.new_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?;
                    verify_hash(&live, expected)?;
                    fs::remove_file(&live)?;
                    sync_parent(&live)?;
                },
                JournalOpKind::Replace => {
                    let old = operation.old_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?;
                    let new = operation.new_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?;
                    if backup.exists() {
                        verify_hash(&backup, old)?;
                        if live.exists() {
                            verify_hash(&live, new)?;
                            fs::remove_file(&live)?;
                        }
                        fs::rename(&backup, &live)?;
                        sync_parent(&live)?;
                    } else {
                        verify_hash(&live, old)?;
                    }
                },
                JournalOpKind::Remove => {
                    let old = operation.old_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?;
                    if backup.exists() {
                        verify_hash(&backup, old)?;
                        if live.exists() {
                            return Err(ProfileLayoutFlowError::AmbiguousTransaction);
                        }
                        fs::rename(&backup, &live)?;
                        sync_parent(&live)?;
                    } else {
                        verify_hash(&live, old)?;
                    }
                },
            }
        }
        Ok(())
    }

    fn restore_previous_manifest(&self, journal: &PublicationJournal) -> Result<(), ProfileLayoutFlowError> {
        let manifest = self.manifest_path();
        match &journal.previous_manifest_sha256 {
            Some(expected) => {
                if file_hash_matches(&manifest, expected)? {
                    return Ok(());
                }
                if manifest.exists() {
                    return Err(ProfileLayoutFlowError::AmbiguousTransaction);
                }
                let backup = self.backup_dir(journal.transaction_id).join(MANIFEST_FILE);
                verify_hash(&backup, expected)?;
                fs::rename(&backup, &manifest)?;
                sync_parent(&manifest)?;
                Ok(())
            },
            None => {
                if manifest.exists() {
                    return Err(ProfileLayoutFlowError::AmbiguousTransaction);
                }
                Ok(())
            },
        }
    }

    fn cleanup_committed(&self, journal: &PublicationJournal) -> Result<(), ProfileLayoutFlowError> {
        self.archive_retained_conflicts(journal)?;
        self.remove_owned_transaction_dir(&self.staging_dir(journal.transaction_id))?;
        self.remove_owned_transaction_dir(&self.backup_dir(journal.transaction_id))?;
        remove_regular_file(&self.journal_path())
    }

    fn archive_retained_conflicts(&self, journal: &PublicationJournal) -> Result<(), ProfileLayoutFlowError> {
        for operation in &journal.operations {
            if !operation.retain_conflict {
                continue;
            }
            let expected = operation.old_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?;
            let backup = self.backup_live_path(journal.transaction_id, &operation.path)?;
            let target_root = self.conflict_copies_root().join(journal.transaction_id.to_string());
            let target = safe_join(&target_root, &operation.path)?;

            let backup_present = verified_regular_file_presence(&backup, expected)?;
            let target_present = verified_regular_file_presence(&target, expected)?;
            match (backup_present, target_present) {
                (true, false) => {
                    create_plain_parent(&target)?;
                    fs::rename(&backup, &target)?;
                    sync_parent(&target)?;
                    verify_hash(&target, expected)?;
                },
                (false, true) => {
                    // Idempotent retry after the rename was already durable but before journal /
                    // transaction cleanup completed.
                },
                (false, false) | (true, true) => {
                    // Missing both copies can hide loss; two copies is an ambiguous transaction.
                    // Keep journal/transaction state intact for explicit recovery/inspection.
                    return Err(ProfileLayoutFlowError::AmbiguousTransaction);
                },
            }
        }
        Ok(())
    }

    fn cleanup_uncommitted(&self, journal: &PublicationJournal) -> Result<(), ProfileLayoutFlowError> {
        self.remove_owned_transaction_dir(&self.staging_dir(journal.transaction_id))?;
        self.remove_owned_transaction_dir(&self.backup_dir(journal.transaction_id))?;
        remove_regular_file(&self.journal_path())
    }

    fn remove_owned_transaction_dir(&self, path: &Path) -> Result<(), ProfileLayoutFlowError> {
        if !path.exists() {
            return Ok(());
        }
        ensure_plain_existing_directory(path)?;
        fs::remove_dir_all(path)?;
        sync_parent(path)
    }

    fn refresh_status(&mut self) -> Result<(), ProfileLayoutFlowError> {
        if self.journal_path().exists() {
            self.status.state = if self.staging_root().exists() {
                ProfileLayoutState::Recovering
            } else {
                ProfileLayoutState::NeedsReconcile
            };
            self.status.stock_fallback = true;
            return Ok(());
        }
        if has_conflicts(&self.conflicts_root())? {
            let manifest = self.read_manifest_optional()?;
            self.status.state = ProfileLayoutState::NeedsReconcile;
            self.status.generation = manifest.as_ref().map(|m| m.generation);
            self.status.stock_fallback = true;
            return Ok(());
        }
        match self.read_manifest_optional()? {
            Some(manifest) => {
                self.validate_manifest(&manifest)?;
                self.status.state = manifest.state;
                self.status.generation = Some(manifest.generation);
                self.status.stock_fallback = manifest.state != ProfileLayoutState::Ready;
            },
            None => {
                self.status.state = ProfileLayoutState::NeedsReconcile;
                self.status.generation = None;
                self.status.stock_fallback = true;
            },
        }
        Ok(())
    }

    fn read_manifest_optional(&self) -> Result<Option<ProfileLayoutManifest>, ProfileLayoutFlowError> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(None);
        }
        ensure_regular_file(&path)?;
        Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
    }

    fn read_journal(&self) -> Result<PublicationJournal, ProfileLayoutFlowError> {
        let path = self.journal_path();
        ensure_regular_file(&path)?;
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    fn validate_manifest(&self, manifest: &ProfileLayoutManifest) -> Result<(), ProfileLayoutFlowError> {
        if manifest.schema != SCHEMA_VERSION || manifest.profile_uuid != self.profile_uuid {
            return Err(ProfileLayoutFlowError::IdentityMismatch);
        }
        manifest.branch.validate(self.profile_uuid)?;
        Ok(())
    }

    fn validate_journal(&self, journal: &PublicationJournal) -> Result<(), ProfileLayoutFlowError> {
        if journal.schema != SCHEMA_VERSION || journal.profile_uuid != self.profile_uuid {
            return Err(ProfileLayoutFlowError::IdentityMismatch);
        }
        if journal.target_generation != journal.from_generation.map_or(1, |g| g.saturating_add(1)) {
            return Err(ProfileLayoutFlowError::AmbiguousTransaction);
        }
        Ok(())
    }

    fn persist_conflict(
        &self,
        fingerprint: &str,
        conflicts: &[String],
        stock_fallback: bool,
    ) -> Result<(), ProfileLayoutFlowError> {
        let root = self.conflicts_root();
        ensure_plain_directory(&root)?;
        let record = ConflictRecord {
            schema: SCHEMA_VERSION,
            profile_uuid: self.profile_uuid,
            managed_input_fingerprint: fingerprint.to_string(),
            conflicts: conflicts.to_vec(),
            requires_stock_fallback: stock_fallback,
        };
        write_new_synced(&root.join(format!("{}.json", new_uuid())), &serde_json::to_vec_pretty(&record)?)
    }

    fn clear_conflicts(&self) -> Result<(), ProfileLayoutFlowError> {
        let root = self.conflicts_root();
        if !root.exists() {
            return Ok(());
        }
        ensure_plain_existing_directory(&root)?;
        fs::remove_dir_all(&root)?;
        sync_parent(&root)
    }

    fn live_path(&self, relative: &str) -> Result<PathBuf, ProfileLayoutFlowError> {
        safe_join(&self.live_root(), relative)
    }

    fn staging_live_path(&self, tx: Uuid, relative: &str) -> Result<PathBuf, ProfileLayoutFlowError> {
        safe_join(&self.staging_dir(tx).join("live"), relative)
    }

    fn backup_live_path(&self, tx: Uuid, relative: &str) -> Result<PathBuf, ProfileLayoutFlowError> {
        safe_join(&self.backup_dir(tx).join("live"), relative)
    }

    #[cfg(test)]
    fn prepare_only_for_test(&mut self, desired: &[DesiredManagedFile]) -> Result<Uuid, ProfileLayoutFlowError> {
        self.recover_if_needed()?;
        let current = self.read_manifest_optional()?;
        let desired_map = canonical_desired(desired)?;
        let previous_entries = current.as_ref().map(|m| m.managed_entries.clone()).unwrap_or_default();
        let mut paths = BTreeSet::new();
        paths.extend(previous_entries.keys().cloned());
        paths.extend(desired_map.keys().cloned());
        let mut inputs = Vec::new();
        for path in paths {
            inputs.push(PathReconcileInput {
                previous: previous_entries.get(&path).map(ManagedManifestEntry::ownership_entry),
                live: observe_live(&self.live_root(), &path)?,
                desired: desired_map
                    .get(&path)
                    .map(|d| ManagedEntry::new(d.logical_identity.clone(), d.source_sha256.clone())),
                path,
            });
        }
        let plan = plan_profile(self.profile_uuid.to_string(), inputs);
        assert!(plan.paths.iter().all(|p| p.decision.conflict.is_none()));
        let tx = new_uuid();
        let generation = current.as_ref().map_or(1, |m| m.generation + 1);
        let target = ProfileLayoutManifest {
            schema: SCHEMA_VERSION,
            profile_uuid: self.profile_uuid,
            generation,
            state: ProfileLayoutState::Ready,
            managed_input_fingerprint: managed_fingerprint(&desired_map),
            sync_identity: String::new(),
            sandbox_policy: String::new(),
            managed_entries: desired_map
                .iter()
                .map(|(p, d)| {
                    (
                        p.clone(),
                        ManagedManifestEntry {
                            logical_identity: d.logical_identity.clone(),
                            source_hash: d.source_sha256.clone(),
                            applied_hash: d.source_sha256.clone(),
                        },
                    )
                })
                .collect(),
            branch: current.as_ref().map(|m| m.branch.clone()).unwrap_or_default(),
            transaction_id: Some(tx),
        };
        let operations = build_operations(&plan.paths, &previous_entries, &desired_map);
        self.prepare_transaction(tx, current.as_ref(), &target, &operations, &desired_map)?;
        Ok(tx)
    }

    #[cfg(test)]
    fn apply_live_only_for_test(&mut self, tx: Uuid) -> Result<(), ProfileLayoutFlowError> {
        let journal = self.read_journal()?;
        write_new_synced(&self.staging_dir(tx).join(PUBLISHING_MARKER), b"publishing\n")?;
        for operation in &journal.operations {
            self.apply_operation(tx, operation)?;
        }
        verify_target_live(self, &journal)
    }

    #[cfg(test)]
    fn commit_manifest_only_for_test(&self, tx: Uuid) -> Result<(), ProfileLayoutFlowError> {
        let journal = self.read_journal()?;
        if journal.transaction_id != tx {
            return Err(ProfileLayoutFlowError::AmbiguousTransaction);
        }
        self.commit_manifest(&journal)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LiveEntryKind {
    Missing,
    File,
    UnexpectedType,
}

fn observe_live_kind(root: &Path, relative: &str) -> Result<LiveEntryKind, ProfileLayoutFlowError> {
    let path = safe_join(root, relative)?;
    ensure_plain_live_parent(root, relative)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            #[cfg(windows)]
            if junction::exists(&path).unwrap_or(false) {
                return Ok(LiveEntryKind::UnexpectedType);
            }
            if metadata.file_type().is_symlink() {
                return Ok(LiveEntryKind::UnexpectedType);
            }
            if metadata.is_file() {
                return Ok(LiveEntryKind::File);
            }
            Ok(LiveEntryKind::UnexpectedType)
        },
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(LiveEntryKind::Missing),
        Err(err) => Err(err.into()),
    }
}

struct CanonicalDesired {
    source: PathBuf,
    logical_identity: String,
    source_sha256: String,
}

fn canonical_desired(
    desired: &[DesiredManagedFile],
) -> Result<BTreeMap<String, CanonicalDesired>, ProfileLayoutFlowError> {
    let mut map = BTreeMap::new();
    for entry in desired {
        validate_relative(&entry.path)?;
        if entry.source_sha256.len() != 64 || !entry.source_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ProfileLayoutFlowError::SourceHashMismatch(entry.path.clone()));
        }
        if map
            .insert(
                entry.path.clone(),
                CanonicalDesired {
                    source: entry.source.clone(),
                    logical_identity: entry.logical_identity.clone(),
                    source_sha256: entry.source_sha256.to_ascii_lowercase(),
                },
            )
            .is_some()
        {
            return Err(ProfileLayoutFlowError::AmbiguousTransaction);
        }
    }
    Ok(map)
}

fn managed_fingerprint(desired: &BTreeMap<String, CanonicalDesired>) -> String {
    let mut hasher = Sha256::new();
    for (path, entry) in desired {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(entry.logical_identity.as_bytes());
        hasher.update([0]);
        hasher.update(entry.source_sha256.as_bytes());
        hasher.update([b'\n']);
    }
    hex::encode(hasher.finalize())
}

fn build_operations(
    planned: &[crate::profile_layout_ownership::PlannedPath],
    previous: &BTreeMap<String, ManagedManifestEntry>,
    desired: &BTreeMap<String, CanonicalDesired>,
) -> Vec<JournalOperation> {
    let mut operations = Vec::new();
    for item in planned {
        let operation = match item.decision.action {
            ReconcileAction::InstallManaged => Some(JournalOperation {
                path: item.path.clone(),
                kind: JournalOpKind::Install,
                old_hash: None,
                new_hash: desired.get(&item.path).map(|d| d.source_sha256.clone()),
                retain_conflict: false,
            }),
            ReconcileAction::ReplaceManaged => Some(JournalOperation {
                path: item.path.clone(),
                kind: JournalOpKind::Replace,
                old_hash: previous.get(&item.path).map(|p| p.applied_hash.clone()),
                new_hash: desired.get(&item.path).map(|d| d.source_sha256.clone()),
                retain_conflict: false,
            }),
            ReconcileAction::RemoveManaged => Some(JournalOperation {
                path: item.path.clone(),
                kind: JournalOpKind::Remove,
                old_hash: previous.get(&item.path).map(|p| p.applied_hash.clone()),
                new_hash: None,
                retain_conflict: false,
            }),
            ReconcileAction::NoOp | ReconcileAction::PreserveLocal => None,
        };
        if let Some(operation) = operation {
            operations.push(operation);
        }
    }
    operations
}

fn conflict_name(reason: ConflictReason) -> &'static str {
    match reason {
        ConflictReason::LocalCollision => "local_collision",
        ConflictReason::LocalOverride => "local_override",
        ConflictReason::Tombstone => "tombstone",
        ConflictReason::UnexpectedType => "unexpected_type",
    }
}

fn verify_transaction_inputs(
    layout: &PersistentProfileLayout,
    journal: &PublicationJournal,
) -> Result<(), ProfileLayoutFlowError> {
    if let Some(previous) = &journal.previous_manifest_sha256 {
        verify_hash(&layout.manifest_path(), previous)?;
    } else if layout.manifest_path().exists() {
        return Err(ProfileLayoutFlowError::AmbiguousTransaction);
    }
    for operation in &journal.operations {
        if matches!(operation.kind, JournalOpKind::Install | JournalOpKind::Replace) {
            let staged = layout.staging_live_path(journal.transaction_id, &operation.path)?;
            verify_hash(
                &staged,
                operation.new_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?,
            )?;
        }
        let live = layout.live_path(&operation.path)?;
        match operation.kind {
            JournalOpKind::Install => {
                if live.exists() {
                    return Err(ProfileLayoutFlowError::OwnershipChanged(operation.path.clone()));
                }
            },
            JournalOpKind::Replace | JournalOpKind::Remove => {
                verify_hash(&live, operation.old_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?)?;
            },
        }
    }
    Ok(())
}

fn verify_target_live(
    layout: &PersistentProfileLayout,
    journal: &PublicationJournal,
) -> Result<(), ProfileLayoutFlowError> {
    for operation in &journal.operations {
        let live = layout.live_path(&operation.path)?;
        match operation.kind {
            JournalOpKind::Install | JournalOpKind::Replace => {
                verify_hash(&live, operation.new_hash.as_deref().ok_or(ProfileLayoutFlowError::AmbiguousTransaction)?)?
            },
            JournalOpKind::Remove => {
                if live.exists() {
                    return Err(ProfileLayoutFlowError::AmbiguousTransaction);
                }
            },
        }
    }
    Ok(())
}

fn observe_live(root: &Path, relative: &str) -> Result<LiveEntry, ProfileLayoutFlowError> {
    let path = safe_join(root, relative)?;
    ensure_plain_live_parent(root, relative)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            #[cfg(windows)]
            if junction::exists(&path).unwrap_or(false) {
                return Ok(LiveEntry::ReparsePoint);
            }
            if metadata.file_type().is_symlink() {
                return Ok(LiveEntry::ReparsePoint);
            }
            if metadata.is_file() {
                return Ok(LiveEntry::file(hash_regular_file(&path)?));
            }
            if metadata.is_dir() {
                return Ok(LiveEntry::Directory);
            }
            Ok(LiveEntry::OtherType)
        },
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(LiveEntry::Missing),
        Err(err) => Err(err.into()),
    }
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, ProfileLayoutFlowError> {
    validate_relative(relative)?;
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        path.push(segment);
    }
    Ok(path)
}

fn validate_relative(relative: &str) -> Result<(), ProfileLayoutFlowError> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.ends_with('/')
        || relative.contains('\\')
        || relative.contains(':')
        || relative.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(ProfileLayoutFlowError::UnsafeRelativePath(relative.to_string()));
    }
    Ok(())
}

fn ensure_plain_live_parent(root: &Path, relative: &str) -> Result<(), ProfileLayoutFlowError> {
    validate_relative(relative)?;
    ensure_plain_existing_directory(root)?;
    let mut current = root.to_path_buf();
    let segments: Vec<_> = relative.split('/').collect();
    for segment in &segments[..segments.len().saturating_sub(1)] {
        current.push(segment);
        ensure_plain_existing_directory(&current)?;
    }
    Ok(())
}

fn stage_source(source: &Path, target: &Path, expected: &str) -> Result<(), ProfileLayoutFlowError> {
    ensure_regular_file(source)?;
    create_plain_parent(target)?;
    if target.exists() {
        return Err(ProfileLayoutFlowError::AmbiguousTransaction);
    }
    fs::copy(source, target)?;
    let file = fs::OpenOptions::new().read(true).write(true).open(target)?;
    file.sync_all()?;
    verify_hash(target, expected)
}

fn create_plain_parent(path: &Path) -> Result<(), ProfileLayoutFlowError> {
    if let Some(parent) = path.parent() {
        ensure_plain_directory(parent)?;
    }
    Ok(())
}

fn load_or_create_identity(control_root: &Path) -> Result<ProfileIdentity, ProfileLayoutFlowError> {
    let path = control_root.join(IDENTITY_FILE);
    if path.exists() {
        ensure_regular_file(&path)?;
        let identity: ProfileIdentity = serde_json::from_slice(&fs::read(path)?)?;
        if identity.schema != SCHEMA_VERSION {
            return Err(ProfileLayoutFlowError::IdentityMismatch);
        }
        return Ok(identity);
    }
    let identity = ProfileIdentity {
        schema: SCHEMA_VERSION,
        profile_uuid: new_uuid(),
    };
    write_new_synced(&path, &serde_json::to_vec_pretty(&identity)?)?;
    Ok(identity)
}

fn ensure_plain_directory(path: &Path) -> Result<(), ProfileLayoutFlowError> {
    if path.exists() {
        return ensure_plain_existing_directory(path);
    }
    fs::create_dir_all(path)?;
    sync_parent(path)?;
    ensure_plain_existing_directory(path)
}

fn ensure_plain_existing_directory(path: &Path) -> Result<(), ProfileLayoutFlowError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ProfileLayoutFlowError::UnsafeFilesystem(path.to_path_buf()));
    }
    #[cfg(windows)]
    if junction::exists(path).unwrap_or(false) {
        return Err(ProfileLayoutFlowError::UnsafeFilesystem(path.to_path_buf()));
    }
    Ok(())
}

fn ensure_regular_file(path: &Path) -> Result<(), ProfileLayoutFlowError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ProfileLayoutFlowError::UnsafeFilesystem(path.to_path_buf()));
    }
    Ok(())
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), ProfileLayoutFlowError> {
    create_plain_parent(path)?;
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    sync_parent(path)
}

fn remove_regular_file(path: &Path) -> Result<(), ProfileLayoutFlowError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(ProfileLayoutFlowError::UnsafeFilesystem(path.to_path_buf()));
            }
            fs::remove_file(path)?;
            sync_parent(path)
        },
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn sync_parent(path: &Path) -> Result<(), ProfileLayoutFlowError> {
    if let Some(parent) = path.parent() {
        let dir = fs::File::open(parent)?;
        dir.sync_all()?;
    }
    Ok(())
}

fn hash_regular_file(path: &Path) -> Result<String, ProfileLayoutFlowError> {
    ensure_regular_file(path)?;
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn verify_hash(path: &Path, expected: &str) -> Result<(), ProfileLayoutFlowError> {
    let actual = hash_regular_file(path)?;
    if actual != expected.to_ascii_lowercase() {
        return Err(ProfileLayoutFlowError::OwnershipChanged(path.to_string_lossy().into_owned()));
    }
    Ok(())
}

fn verified_regular_file_presence(path: &Path, expected: &str) -> Result<bool, ProfileLayoutFlowError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            verify_hash(path, expected)?;
            Ok(true)
        },
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn file_hash_matches(path: &Path, expected: &str) -> Result<bool, ProfileLayoutFlowError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(ProfileLayoutFlowError::UnsafeFilesystem(path.to_path_buf()));
            }
            Ok(hash_regular_file(path)? == expected.to_ascii_lowercase())
        },
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn has_conflicts(root: &Path) -> Result<bool, ProfileLayoutFlowError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(ProfileLayoutFlowError::UnsafeFilesystem(root.to_path_buf()));
            }
            #[cfg(windows)]
            if junction::exists(root).unwrap_or(false) {
                return Err(ProfileLayoutFlowError::UnsafeFilesystem(root.to_path_buf()));
            }
            Ok(fs::read_dir(root)?.next().transpose()?.is_some())
        },
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn new_uuid() -> Uuid {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!("pandora-profile-flow-{label}-{}", new_uuid()));
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join(".minecraft")).unwrap();
            fs::create_dir(root.join(".minecraft/mods")).unwrap();
            Self(root)
        }

        fn source(&self, name: &str, bytes: &[u8]) -> DesiredManagedFile {
            let source = self.0.join(format!("source-{name}"));
            fs::write(&source, bytes).unwrap();
            DesiredManagedFile::new(format!("mods/{name}"), source, format!("identity-{name}"), sha256_bytes(bytes))
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn two_profiles_are_isolated_by_uuid_and_generation() {
        let a = TestRoot::new("a");
        let b = TestRoot::new("b");
        let mut layout_a = PersistentProfileLayout::open(&a.0).unwrap();
        let mut layout_b = PersistentProfileLayout::open(&b.0).unwrap();
        assert_ne!(layout_a.profile_uuid(), layout_b.profile_uuid());

        let a1 = a.source("example.jar", b"a-v1");
        let b1 = b.source("example.jar", b"b-v1");
        layout_a.reconcile(&[a1]).unwrap();
        layout_b.reconcile(&[b1]).unwrap();
        let b_bytes = fs::read(b.0.join(".minecraft/mods/example.jar")).unwrap();

        let a2 = a.source("example.jar", b"a-v2");
        layout_a.reconcile(&[a2]).unwrap();
        let reopened_b = PersistentProfileLayout::open(&b.0).unwrap();
        assert_eq!(reopened_b.status().generation, Some(1));
        assert_eq!(fs::read(b.0.join(".minecraft/mods/example.jar")).unwrap(), b_bytes);
    }

    #[test]
    fn proven_managed_replaces_but_local_override_is_preserved() {
        let root = TestRoot::new("ownership");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let v1 = root.source("managed.jar", b"managed-v1");
        layout.reconcile(&[v1]).unwrap();
        let v2 = root.source("managed.jar", b"managed-v2");
        layout.reconcile(&[v2]).unwrap();
        assert_eq!(fs::read(root.0.join(".minecraft/mods/managed.jar")).unwrap(), b"managed-v2");

        fs::write(root.0.join(".minecraft/mods/managed.jar"), b"local-edit").unwrap();
        let v3 = root.source("managed.jar", b"managed-v3");
        let outcome = layout.reconcile(&[v3]).unwrap();
        assert!(matches!(outcome, ReconcileOutcome::NeedsReconcile { .. }));
        assert_eq!(fs::read(root.0.join(".minecraft/mods/managed.jar")).unwrap(), b"local-edit");
    }

    #[test]
    fn tombstone_and_new_local_collision_are_preserved() {
        let root = TestRoot::new("local");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let v1 = root.source("managed.jar", b"managed-v1");
        layout.reconcile(&[v1]).unwrap();
        fs::remove_file(root.0.join(".minecraft/mods/managed.jar")).unwrap();
        let v2 = root.source("managed.jar", b"managed-v2");
        let outcome = layout.reconcile(&[v2]).unwrap();
        assert!(matches!(outcome, ReconcileOutcome::NeedsReconcile { .. }));
        assert!(!root.0.join(".minecraft/mods/managed.jar").exists());

        let second = TestRoot::new("collision");
        fs::write(second.0.join(".minecraft/mods/local.jar"), b"local").unwrap();
        let mut second_layout = PersistentProfileLayout::open(&second.0).unwrap();
        let managed = second.source("local.jar", b"managed");
        let outcome = second_layout.reconcile(&[managed]).unwrap();
        assert!(matches!(outcome, ReconcileOutcome::NeedsReconcile { .. }));
        assert_eq!(fs::read(second.0.join(".minecraft/mods/local.jar")).unwrap(), b"local");
    }

    #[test]
    fn crash_before_manifest_commit_rolls_live_bytes_back_idempotently() {
        let root = TestRoot::new("before-commit");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let v1 = root.source("managed.jar", b"v1");
        layout.reconcile(&[v1]).unwrap();
        let v2 = root.source("managed.jar", b"v2");
        let tx = layout.prepare_only_for_test(&[v2]).unwrap();
        layout.apply_live_only_for_test(tx).unwrap();
        assert_eq!(fs::read(root.0.join(".minecraft/mods/managed.jar")).unwrap(), b"v2");
        drop(layout);

        let recovered = PersistentProfileLayout::open(&root.0).unwrap();
        assert_eq!(recovered.status().generation, Some(1));
        assert_eq!(fs::read(root.0.join(".minecraft/mods/managed.jar")).unwrap(), b"v1");
        let recovered_again = PersistentProfileLayout::open(&root.0).unwrap();
        assert_eq!(recovered_again.status().generation, Some(1));
    }

    #[test]
    fn crash_after_manifest_commit_finishes_verified_cleanup_idempotently() {
        let root = TestRoot::new("after-commit");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let v1 = root.source("managed.jar", b"v1");
        layout.reconcile(&[v1]).unwrap();
        let v2 = root.source("managed.jar", b"v2");
        let tx = layout.prepare_only_for_test(&[v2]).unwrap();
        layout.apply_live_only_for_test(tx).unwrap();
        layout.commit_manifest_only_for_test(tx).unwrap();
        assert!(layout.journal_path().exists());
        drop(layout);

        let recovered = PersistentProfileLayout::open(&root.0).unwrap();
        assert_eq!(recovered.status().generation, Some(2));
        assert_eq!(fs::read(root.0.join(".minecraft/mods/managed.jar")).unwrap(), b"v2");
        assert!(!recovered.journal_path().exists());
        let recovered_again = PersistentProfileLayout::open(&root.0).unwrap();
        assert_eq!(recovered_again.status().generation, Some(2));
    }

    #[test]
    fn corrupt_journal_never_changes_live_layout() {
        let root = TestRoot::new("corrupt");
        fs::write(root.0.join(".minecraft/mods/local.jar"), b"local").unwrap();
        let layout = PersistentProfileLayout::open(&root.0).unwrap();
        fs::write(layout.journal_path(), b"not-json").unwrap();
        let before = fs::read(root.0.join(".minecraft/mods/local.jar")).unwrap();
        assert!(PersistentProfileLayout::open(&root.0).is_err());
        assert_eq!(fs::read(root.0.join(".minecraft/mods/local.jar")).unwrap(), before);
    }

    #[test]
    fn corrupt_staging_is_rejected_before_live_mutation() {
        let root = TestRoot::new("corrupt-staging");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let v1 = root.source("managed.jar", b"v1");
        layout.reconcile(&[v1]).unwrap();
        let v2 = root.source("managed.jar", b"v2");
        let tx = layout.prepare_only_for_test(&[v2]).unwrap();
        let staged = layout.staging_live_path(tx, "mods/managed.jar").unwrap();
        fs::write(&staged, b"corrupt").unwrap();
        let before = fs::read(root.0.join(".minecraft/mods/managed.jar")).unwrap();

        assert!(layout.publish_transaction(tx).is_err());
        assert_eq!(fs::read(root.0.join(".minecraft/mods/managed.jar")).unwrap(), before);
        assert!(layout.journal_path().exists());
        drop(layout);

        let recovered = PersistentProfileLayout::open(&root.0).unwrap();
        assert_eq!(recovered.status().generation, Some(1));
        assert_eq!(fs::read(root.0.join(".minecraft/mods/managed.jar")).unwrap(), b"v1");
        assert!(!recovered.journal_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn reparse_destination_and_control_root_fail_without_live_mutation() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new("reparse-live");
        let outside = TestRoot::new("outside-live");
        symlink(outside.0.join(".minecraft/mods"), root.0.join(".minecraft/mods/link")).unwrap();
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let source = root.source("candidate.jar", b"managed");
        let desired = DesiredManagedFile::new("mods/link", source.source, "identity-link", source.source_sha256);
        let outcome = layout.reconcile(&[desired]).unwrap();
        assert!(matches!(
            outcome,
            ReconcileOutcome::NeedsReconcile {
                stock_fallback: true,
                ..
            }
        ));

        let control_root = TestRoot::new("reparse-control");
        let outside_control = TestRoot::new("outside-control");
        symlink(&outside_control.0, control_root.0.join(CONTROL_DIR)).unwrap();
        assert!(PersistentProfileLayout::open(&control_root.0).is_err());
        assert!(!outside_control.0.join(IDENTITY_FILE).exists());
    }

    fn global_pin(revision: &str, digit: char) -> crate::profile_branch::GlobalRevisionPin {
        crate::profile_branch::GlobalRevisionPin::new("global-test", revision, digit.to_string().repeat(64)).unwrap()
    }

    fn effective_entry(
        root: &TestRoot,
        name: &str,
        bytes: &[u8],
        pin: crate::profile_branch::GlobalRevisionPin,
        policy: crate::profile_branch::ProfileFilePolicy,
    ) -> crate::profile_branch::EffectiveProfileEntry {
        let source = root.0.join(format!("branch-source-{name}-{}", sha256_bytes(bytes)));
        fs::write(&source, bytes).unwrap();
        crate::profile_branch::EffectiveProfileEntry {
            path: format!("mods/{name}"),
            source,
            metadata: crate::profile_branch::ProfileEntryMetadata {
                logical_identity: format!("identity-{name}"),
                source_sha256: sha256_bytes(bytes),
                origin: crate::profile_branch::ProfileEntryOrigin::GlobalRevision { pin },
                ownership: crate::profile_branch::ProfileEntryOwnership::Inherited,
                policy,
            },
        }
    }

    fn global_delta(
        pin: crate::profile_branch::GlobalRevisionPin,
        changes: Vec<crate::profile_branch::ProfileDeltaChange>,
    ) -> crate::profile_branch::ProfileRevisionDelta {
        crate::profile_branch::ProfileRevisionDelta {
            lineage: crate::profile_branch::ProfileLineage::from_global(pin.clone()).unwrap(),
            target_revision: Some(pin),
            changes,
        }
    }

    #[test]
    fn stable_revision_noop_does_not_hash_live_managed_destination() {
        let root = TestRoot::new("delta-noop");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let r1 = global_pin("r1", 'a');
        let entry = effective_entry(
            &root,
            "managed.jar",
            b"managed-v1",
            r1.clone(),
            crate::profile_branch::ProfileFilePolicy::Enforced,
        );
        layout
            .reconcile_revision_delta(&global_delta(
                r1.clone(),
                vec![crate::profile_branch::ProfileDeltaChange::Upsert(entry)],
            ))
            .unwrap();

        fs::write(root.0.join(".minecraft/mods/managed.jar"), b"corrupt-but-unchanged").unwrap();
        let outcome = layout.reconcile_revision_delta(&global_delta(r1, Vec::new())).unwrap();

        assert!(matches!(outcome, ReconcileOutcome::Ready { generation: 1, .. }));
        assert_eq!(fs::read(root.0.join(".minecraft/mods/managed.jar")).unwrap(), b"corrupt-but-unchanged");
    }

    #[test]
    fn delta_only_verifies_changed_destinations() {
        let root = TestRoot::new("delta-only-changed");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let r1 = global_pin("r1", 'a');
        let a1 =
            effective_entry(&root, "a.jar", b"a-v1", r1.clone(), crate::profile_branch::ProfileFilePolicy::Enforced);
        let b1 =
            effective_entry(&root, "b.jar", b"b-v1", r1.clone(), crate::profile_branch::ProfileFilePolicy::Enforced);
        layout
            .reconcile_revision_delta(&global_delta(
                r1,
                vec![
                    crate::profile_branch::ProfileDeltaChange::Upsert(a1),
                    crate::profile_branch::ProfileDeltaChange::Upsert(b1),
                ],
            ))
            .unwrap();

        fs::write(root.0.join(".minecraft/mods/b.jar"), b"local-b").unwrap();
        let r2 = global_pin("r2", 'b');
        let a2 =
            effective_entry(&root, "a.jar", b"a-v2", r2.clone(), crate::profile_branch::ProfileFilePolicy::Enforced);
        layout
            .reconcile_revision_delta(&global_delta(r2, vec![crate::profile_branch::ProfileDeltaChange::Upsert(a2)]))
            .unwrap();

        assert_eq!(fs::read(root.0.join(".minecraft/mods/a.jar")).unwrap(), b"a-v2");
        assert_eq!(fs::read(root.0.join(".minecraft/mods/b.jar")).unwrap(), b"local-b");
    }

    #[test]
    fn default_once_becomes_user_owned_after_initial_seed() {
        let root = TestRoot::new("default-once");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let r1 = global_pin("r1", 'a');
        let v1 = effective_entry(
            &root,
            "defaults.cfg",
            b"default-v1",
            r1.clone(),
            crate::profile_branch::ProfileFilePolicy::DefaultOnce,
        );
        layout
            .reconcile_revision_delta(&global_delta(r1, vec![crate::profile_branch::ProfileDeltaChange::Upsert(v1)]))
            .unwrap();

        fs::write(root.0.join(".minecraft/mods/defaults.cfg"), b"local-choice").unwrap();
        let r2 = global_pin("r2", 'b');
        let v2 = effective_entry(
            &root,
            "defaults.cfg",
            b"default-v2",
            r2.clone(),
            crate::profile_branch::ProfileFilePolicy::DefaultOnce,
        );
        layout
            .reconcile_revision_delta(&global_delta(r2, vec![crate::profile_branch::ProfileDeltaChange::Upsert(v2)]))
            .unwrap();

        assert_eq!(fs::read(root.0.join(".minecraft/mods/defaults.cfg")).unwrap(), b"local-choice");
        let branch = layout.branch_manifest().unwrap();
        assert_eq!(
            branch.entries["mods/defaults.cfg"].ownership,
            crate::profile_branch::ProfileEntryOwnership::UserOwned
        );
    }

    #[test]
    fn enforced_override_is_applied_and_old_local_bytes_are_recoverable() {
        let root = TestRoot::new("enforced-conflict-copy");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let r1 = global_pin("r1", 'a');
        let v1 = effective_entry(
            &root,
            "forced.cfg",
            b"forced-v1",
            r1.clone(),
            crate::profile_branch::ProfileFilePolicy::Enforced,
        );
        layout
            .reconcile_revision_delta(&global_delta(r1, vec![crate::profile_branch::ProfileDeltaChange::Upsert(v1)]))
            .unwrap();

        fs::write(root.0.join(".minecraft/mods/forced.cfg"), b"local-edit").unwrap();
        let r2 = global_pin("r2", 'b');
        let v2 = effective_entry(
            &root,
            "forced.cfg",
            b"forced-v2",
            r2.clone(),
            crate::profile_branch::ProfileFilePolicy::Enforced,
        );
        layout
            .reconcile_revision_delta(&global_delta(r2, vec![crate::profile_branch::ProfileDeltaChange::Upsert(v2)]))
            .unwrap();

        assert_eq!(fs::read(root.0.join(".minecraft/mods/forced.cfg")).unwrap(), b"forced-v2");
        let copies = root.0.join(CONTROL_DIR).join("conflict-copies");
        let tx_dir = fs::read_dir(&copies).unwrap().next().unwrap().unwrap().path();
        assert_eq!(fs::read(tx_dir.join("mods/forced.cfg")).unwrap(), b"local-edit");
    }

    #[test]
    fn local_tombstone_survives_parent_revision_and_masks_resolver() {
        let root = TestRoot::new("local-tombstone");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        let profile_uuid = layout.profile_uuid();
        let r1 = global_pin("r1", 'a');
        let v1 = effective_entry(
            &root,
            "removed.jar",
            b"parent-v1",
            r1.clone(),
            crate::profile_branch::ProfileFilePolicy::Enforced,
        );
        layout
            .reconcile_revision_delta(&global_delta(
                r1.clone(),
                vec![crate::profile_branch::ProfileDeltaChange::Upsert(v1)],
            ))
            .unwrap();

        layout
            .reconcile_revision_delta(&crate::profile_branch::ProfileRevisionDelta {
                lineage: crate::profile_branch::ProfileLineage::from_global(r1.clone()).unwrap(),
                target_revision: Some(r1),
                changes: vec![crate::profile_branch::ProfileDeltaChange::Remove {
                    path: "mods/removed.jar".to_owned(),
                    origin: crate::profile_branch::ProfileEntryOrigin::LocalProfile { profile_uuid },
                    ownership: crate::profile_branch::ProfileEntryOwnership::Local,
                    policy: crate::profile_branch::ProfileFilePolicy::Enforced,
                }],
            })
            .unwrap();
        assert!(!root.0.join(".minecraft/mods/removed.jar").exists());
        drop(layout);

        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        assert!(layout.branch_manifest().unwrap().tombstones.contains_key("mods/removed.jar"));

        let r2 = global_pin("r2", 'b');
        let v2 = effective_entry(
            &root,
            "removed.jar",
            b"parent-v2",
            r2.clone(),
            crate::profile_branch::ProfileFilePolicy::Enforced,
        );
        let resolver_parent = v2.clone();
        layout
            .reconcile_revision_delta(&global_delta(
                r2.clone(),
                vec![crate::profile_branch::ProfileDeltaChange::Upsert(v2)],
            ))
            .unwrap();

        assert!(!root.0.join(".minecraft/mods/removed.jar").exists());
        let branch = layout.branch_manifest().unwrap();
        assert!(branch.tombstones.contains_key("mods/removed.jar"));
        assert_eq!(branch.applied_revision.as_ref(), Some(&r2));
        let resolved =
            crate::profile_branch::resolve_effective_entries_for_branch([resolver_parent], profile_uuid, &branch, &[])
                .unwrap();
        assert!(resolved.is_empty());
    }

    #[test]
    fn retained_conflict_cleanup_is_idempotent_after_archive() {
        let root = TestRoot::new("conflict-cleanup-idempotent");
        let layout = PersistentProfileLayout::open(&root.0).unwrap();
        let tx = new_uuid();
        let old = b"local-overwrite";
        let old_hash = sha256_bytes(old);
        let backup = layout.backup_live_path(tx, "mods/forced.cfg").unwrap();
        write_new_synced(&backup, old).unwrap();
        let journal = PublicationJournal {
            schema: SCHEMA_VERSION,
            profile_uuid: layout.profile_uuid(),
            transaction_id: tx,
            from_generation: Some(1),
            target_generation: 2,
            previous_manifest_sha256: None,
            target_manifest_sha256: "0".repeat(64),
            state: ProfileLayoutState::Prepared,
            operations: vec![JournalOperation {
                path: "mods/forced.cfg".to_owned(),
                kind: JournalOpKind::Replace,
                old_hash: Some(old_hash),
                new_hash: Some("1".repeat(64)),
                retain_conflict: true,
            }],
        };
        write_new_synced(&layout.journal_path(), &serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

        layout.archive_retained_conflicts(&journal).unwrap();
        let archived = layout.conflict_copies_root().join(tx.to_string()).join("mods/forced.cfg");
        assert_eq!(fs::read(&archived).unwrap(), old);
        assert!(!backup.exists());

        layout.cleanup_committed(&journal).unwrap();
        layout.cleanup_committed(&journal).unwrap();
        assert_eq!(fs::read(&archived).unwrap(), old);
        assert!(!layout.journal_path().exists());
    }

    #[test]
    fn retained_conflict_cleanup_blocks_when_recoverable_copy_is_missing_or_corrupt() {
        let root = TestRoot::new("conflict-cleanup-fail-closed");
        let layout = PersistentProfileLayout::open(&root.0).unwrap();
        let tx = new_uuid();
        ensure_plain_directory(&layout.staging_dir(tx)).unwrap();
        ensure_plain_directory(&layout.backup_dir(tx)).unwrap();
        let expected = sha256_bytes(b"expected-local");
        let journal = PublicationJournal {
            schema: SCHEMA_VERSION,
            profile_uuid: layout.profile_uuid(),
            transaction_id: tx,
            from_generation: Some(1),
            target_generation: 2,
            previous_manifest_sha256: None,
            target_manifest_sha256: "0".repeat(64),
            state: ProfileLayoutState::Prepared,
            operations: vec![JournalOperation {
                path: "mods/forced.cfg".to_owned(),
                kind: JournalOpKind::Replace,
                old_hash: Some(expected),
                new_hash: Some("1".repeat(64)),
                retain_conflict: true,
            }],
        };
        write_new_synced(&layout.journal_path(), &serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

        assert!(matches!(
            layout.cleanup_committed(&journal),
            Err(ProfileLayoutFlowError::AmbiguousTransaction)
        ));
        assert!(layout.journal_path().exists());
        assert!(layout.staging_dir(tx).exists());
        assert!(layout.backup_dir(tx).exists());

        let archived = layout.conflict_copies_root().join(tx.to_string()).join("mods/forced.cfg");
        write_new_synced(&archived, b"corrupt-local").unwrap();
        assert!(matches!(
            layout.cleanup_committed(&journal),
            Err(ProfileLayoutFlowError::OwnershipChanged(_))
        ));
        assert!(layout.journal_path().exists());
        assert!(layout.staging_dir(tx).exists());
        assert!(layout.backup_dir(tx).exists());
    }

    #[test]
    fn repair_uses_legacy_managed_entries_without_touching_unmanaged_local_files() {
        let root = TestRoot::new("legacy-repair");
        let layout = PersistentProfileLayout::open(&root.0).unwrap();
        let legacy_path = root.0.join(".minecraft/mods/legacy.jar");
        fs::write(&legacy_path, b"legacy-managed").unwrap();
        let unrelated = root.0.join(".minecraft/mods/local.jar");
        fs::write(&unrelated, b"local-only").unwrap();
        let legacy_hash = sha256_bytes(b"legacy-managed");
        let mut managed_entries = BTreeMap::new();
        managed_entries.insert(
            "mods/legacy.jar".to_owned(),
            ManagedManifestEntry {
                logical_identity: "legacy-managed".to_owned(),
                source_hash: legacy_hash.clone(),
                applied_hash: legacy_hash,
            },
        );
        let legacy_manifest = ProfileLayoutManifest {
            schema: SCHEMA_VERSION,
            profile_uuid: layout.profile_uuid(),
            generation: 1,
            state: ProfileLayoutState::Ready,
            managed_input_fingerprint: "legacy-v1".to_owned(),
            sync_identity: String::new(),
            sandbox_policy: String::new(),
            managed_entries,
            branch: ProfileBranchManifest::default(),
            transaction_id: None,
        };
        let mut legacy_json = serde_json::to_value(&legacy_manifest).unwrap();
        legacy_json.as_object_mut().unwrap().remove("branch");
        write_new_synced(&layout.manifest_path(), &serde_json::to_vec_pretty(&legacy_json).unwrap()).unwrap();
        drop(layout);

        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        assert!(layout.branch_manifest().unwrap().entries.is_empty());
        let r1 = global_pin("r1", 'c');
        layout
            .configure_branch_lineage(crate::profile_branch::ProfileLineage::from_global(r1).unwrap())
            .unwrap();
        layout.repair_modpack(&[]).unwrap();

        assert!(!legacy_path.exists());
        assert_eq!(fs::read(&unrelated).unwrap(), b"local-only");
        assert!(layout.read_manifest_optional().unwrap().unwrap().managed_entries.is_empty());
    }

    #[test]
    fn repair_modpack_is_unavailable_to_pure_local_profile() {
        let root = TestRoot::new("pure-local-repair");
        let mut layout = PersistentProfileLayout::open(&root.0).unwrap();
        layout
            .configure_branch_lineage(crate::profile_branch::ProfileLineage::pure_local())
            .unwrap();
        assert!(matches!(layout.repair_modpack(&[]), Err(ProfileLayoutFlowError::RepairUnavailable)));
    }
}
