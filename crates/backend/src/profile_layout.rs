use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

const CONTROL_DIR: &str = ".pandora-layout-v1";
const IDENTITY_FILE: &str = "identity.json";
const MANIFEST_FILE: &str = "manifest.json";
const JOURNAL_FILE: &str = "journal.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileLayoutManifest {
    pub schema: u32,
    pub profile_uuid: Uuid,
    pub generation: u64,
    pub state: ProfileLayoutState,
    pub managed_input_fingerprint: String,
    pub sync_identity: String,
    pub sandbox_policy: String,
    #[serde(default)]
    pub transaction_id: Option<Uuid>,
}

impl ProfileLayoutManifest {
    pub fn new_ready(profile_uuid: Uuid, generation: u64) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            profile_uuid,
            generation,
            state: ProfileLayoutState::Ready,
            managed_input_fingerprint: String::new(),
            sync_identity: String::new(),
            sandbox_policy: String::new(),
            transaction_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProfileLayoutIdentity {
    schema: u32,
    profile_uuid: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProfileLayoutJournal {
    schema: u32,
    profile_uuid: Uuid,
    transaction_id: Uuid,
    from_generation: u64,
    target_generation: u64,
    previous_manifest_sha256: String,
    target_manifest_sha256: String,
    state: ProfileLayoutState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileLayoutStatus {
    pub profile_uuid: Uuid,
    pub state: ProfileLayoutState,
    pub generation: Option<u64>,
    pub stock_fallback: bool,
    pub fallback_reason: Option<String>,
}

#[derive(Debug)]
pub struct ProfileLayout {
    instance_root: PathBuf,
    control_root: PathBuf,
    pub status: ProfileLayoutStatus,
}

#[derive(Debug, Error)]
pub enum ProfileLayoutError {
    #[error("profile layout I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("profile layout serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unsafe profile layout filesystem object at {0:?}")]
    UnsafeFilesystem(PathBuf),
    #[error("profile layout identity mismatch")]
    IdentityMismatch,
    #[error("profile layout generation mismatch")]
    GenerationMismatch,
    #[error("profile layout transaction is ambiguous")]
    AmbiguousTransaction,
    #[error("profile layout transaction is already committed")]
    AlreadyCommitted,
    #[error("no committed profile layout manifest")]
    MissingManifest,
    #[error("no matching profile layout transaction")]
    MissingTransaction,
}

impl ProfileLayout {
    /// Load the control plane for one Instance. Failure is deliberately fail-open
    /// to the stock launcher path: live `.minecraft` data is never repaired or
    /// deleted from here.
    pub fn load(instance_root: &Path) -> Self {
        let control_root = instance_root.join(CONTROL_DIR);
        match Self::load_inner(instance_root, &control_root) {
            Ok(layout) => layout,
            Err(err) => Self {
                instance_root: instance_root.to_path_buf(),
                control_root,
                status: ProfileLayoutStatus {
                    profile_uuid: new_uuid(),
                    state: ProfileLayoutState::NeedsReconcile,
                    generation: None,
                    stock_fallback: true,
                    fallback_reason: Some(err.to_string()),
                },
            },
        }
    }

    fn load_inner(instance_root: &Path, control_root: &Path) -> Result<Self, ProfileLayoutError> {
        ensure_plain_directory(control_root)?;
        let identity = load_or_create_identity(control_root)?;
        let mut layout = Self {
            instance_root: instance_root.to_path_buf(),
            control_root: control_root.to_path_buf(),
            status: ProfileLayoutStatus {
                profile_uuid: identity.profile_uuid,
                state: ProfileLayoutState::NeedsReconcile,
                generation: None,
                stock_fallback: true,
                fallback_reason: None,
            },
        };

        let journal_path = layout.journal_path();
        if journal_path.exists() {
            match layout.read_journal() {
                Ok(_) => {
                    if let Err(err) = layout.recover() {
                        layout.mark_fallback(ProfileLayoutState::Recovering, err.to_string());
                        return Ok(layout);
                    }
                },
                Err(err) => {
                    layout.mark_fallback(ProfileLayoutState::Recovering, err.to_string());
                    return Ok(layout);
                },
            }
        }

        match layout.read_manifest() {
            Ok(Some(manifest)) => layout.apply_manifest_status(&manifest),
            Ok(None) => layout.mark_fallback(
                ProfileLayoutState::NeedsReconcile,
                "profile layout has no committed manifest".to_string(),
            ),
            Err(err) => layout.mark_fallback(ProfileLayoutState::NeedsReconcile, err.to_string()),
        }
        Ok(layout)
    }

    pub fn profile_uuid(&self) -> Uuid {
        self.status.profile_uuid
    }

    pub fn instance_root(&self) -> &Path {
        &self.instance_root
    }

    pub fn control_root(&self) -> &Path {
        &self.control_root
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.control_root.join(MANIFEST_FILE)
    }

    pub fn journal_path(&self) -> PathBuf {
        self.control_root.join(JOURNAL_FILE)
    }

    pub fn staging_root(&self) -> PathBuf {
        self.control_root.join("staging")
    }

    pub fn backup_root(&self) -> PathBuf {
        self.control_root.join("backup")
    }

    /// Atomic single-file bootstrap commit used by the future stopped-profile
    /// reconciler. It does not touch `.minecraft` and cannot replace an existing
    /// generation.
    pub fn commit_initial_manifest(
        &mut self,
        mut manifest: ProfileLayoutManifest,
    ) -> Result<(), ProfileLayoutError> {
        ensure_control_paths(&self.control_root)?;
        if self.manifest_path().exists() || self.journal_path().exists() {
            return Err(ProfileLayoutError::GenerationMismatch);
        }
        if manifest.profile_uuid != self.profile_uuid() || manifest.schema != SCHEMA_VERSION || manifest.generation != 1 {
            return Err(ProfileLayoutError::GenerationMismatch);
        }
        manifest.transaction_id = None;
        write_json_safe(&self.manifest_path(), &manifest)?;
        self.apply_manifest_status(&manifest);
        Ok(())
    }

    /// Prepare the next metadata generation. Staging and backup live below the
    /// Instance root, so later publication uses only same-profile renames.
    pub fn prepare_manifest(
        &mut self,
        mut target: ProfileLayoutManifest,
    ) -> Result<Uuid, ProfileLayoutError> {
        ensure_control_paths(&self.control_root)?;
        if self.journal_path().exists() {
            return Err(ProfileLayoutError::AmbiguousTransaction);
        }

        let (current, current_bytes) = self
            .read_manifest_bytes()?
            .ok_or(ProfileLayoutError::MissingManifest)?;
        self.validate_manifest(&current)?;
        if target.profile_uuid != self.profile_uuid()
            || target.schema != SCHEMA_VERSION
            || target.generation != current.generation.saturating_add(1)
            || target.state != ProfileLayoutState::Ready
        {
            return Err(ProfileLayoutError::GenerationMismatch);
        }

        let transaction_id = new_uuid();
        target.transaction_id = Some(transaction_id);
        let target_bytes = serde_json::to_vec_pretty(&target)?;
        let mut journal = ProfileLayoutJournal {
            schema: SCHEMA_VERSION,
            profile_uuid: self.profile_uuid(),
            transaction_id,
            from_generation: current.generation,
            target_generation: target.generation,
            previous_manifest_sha256: sha256_hex(&current_bytes),
            target_manifest_sha256: sha256_hex(&target_bytes),
            state: ProfileLayoutState::Planning,
        };
        self.write_journal(&journal)?;
        self.status.state = ProfileLayoutState::Planning;

        let staging = self.staging_dir(transaction_id);
        let backup = self.backup_dir(transaction_id);
        ensure_plain_directory(&staging)?;
        ensure_plain_directory(&backup)?;
        journal.state = ProfileLayoutState::Staging;
        self.write_journal(&journal)?;
        self.status.state = ProfileLayoutState::Staging;

        write_bytes_safe(&staging.join(MANIFEST_FILE), &target_bytes)?;
        journal.state = ProfileLayoutState::Prepared;
        self.write_journal(&journal)?;
        self.status.state = ProfileLayoutState::Prepared;
        Ok(transaction_id)
    }

    pub fn publish_prepared(&mut self, transaction_id: Uuid) -> Result<(), ProfileLayoutError> {
        self.publish_inner(transaction_id, PublishFault::None)
    }

    pub fn rollback(&mut self, transaction_id: Uuid) -> Result<(), ProfileLayoutError> {
        let mut journal = self.read_journal()?;
        self.validate_journal(&journal)?;
        if journal.transaction_id != transaction_id {
            return Err(ProfileLayoutError::MissingTransaction);
        }
        if self.current_manifest_matches_hash(&journal.target_manifest_sha256)? {
            return Err(ProfileLayoutError::AlreadyCommitted);
        }
        journal.state = ProfileLayoutState::Recovering;
        self.write_journal(&journal)?;
        self.status.state = ProfileLayoutState::Recovering;
        self.rollback_journal(&journal)
    }

    /// Idempotent startup recovery. A committed target generation is completed;
    /// an uncommitted publication is rolled back only when the recorded hashes
    /// prove which manifest is old/new. Otherwise all evidence is preserved.
    pub fn recover(&mut self) -> Result<(), ProfileLayoutError> {
        let mut journal = self.read_journal()?;
        self.validate_journal(&journal)?;

        if self.current_manifest_matches_hash(&journal.target_manifest_sha256)? {
            self.cleanup_committed(&journal)?;
            let manifest = self.read_manifest()?.ok_or(ProfileLayoutError::MissingManifest)?;
            self.apply_manifest_status(&manifest);
            return Ok(());
        }

        match journal.state {
            ProfileLayoutState::Planning | ProfileLayoutState::Staging | ProfileLayoutState::Prepared => {
                if self.backup_manifest_path(journal.transaction_id).exists() {
                    journal.state = ProfileLayoutState::Recovering;
                    self.write_journal(&journal)?;
                    self.rollback_journal(&journal)
                } else if self.current_manifest_matches_hash(&journal.previous_manifest_sha256)? {
                    self.cleanup_uncommitted(&journal)?;
                    let manifest = self.read_manifest()?.ok_or(ProfileLayoutError::MissingManifest)?;
                    self.apply_manifest_status(&manifest);
                    Ok(())
                } else {
                    Err(ProfileLayoutError::AmbiguousTransaction)
                }
            },
            ProfileLayoutState::Publishing | ProfileLayoutState::Recovering => {
                journal.state = ProfileLayoutState::Recovering;
                self.write_journal(&journal)?;
                self.rollback_journal(&journal)
            },
            ProfileLayoutState::Ready | ProfileLayoutState::NeedsReconcile => {
                Err(ProfileLayoutError::AmbiguousTransaction)
            },
        }
    }

    fn publish_inner(
        &mut self,
        transaction_id: Uuid,
        fault: PublishFault,
    ) -> Result<(), ProfileLayoutError> {
        ensure_control_paths(&self.control_root)?;
        let mut journal = self.read_journal()?;
        self.validate_journal(&journal)?;
        if journal.transaction_id != transaction_id || journal.state != ProfileLayoutState::Prepared {
            return Err(ProfileLayoutError::MissingTransaction);
        }

        let staged_manifest = self.staging_dir(transaction_id).join(MANIFEST_FILE);
        let staged_bytes = read_regular_file(&staged_manifest)?;
        if sha256_hex(&staged_bytes) != journal.target_manifest_sha256 {
            return Err(ProfileLayoutError::AmbiguousTransaction);
        }
        let staged: ProfileLayoutManifest = serde_json::from_slice(&staged_bytes)?;
        self.validate_manifest(&staged)?;
        if staged.transaction_id != Some(transaction_id) || staged.generation != journal.target_generation {
            return Err(ProfileLayoutError::AmbiguousTransaction);
        }

        journal.state = ProfileLayoutState::Publishing;
        self.write_journal(&journal)?;
        self.status.state = ProfileLayoutState::Publishing;

        let manifest_path = self.manifest_path();
        let backup_manifest = self.backup_manifest_path(transaction_id);
        if backup_manifest.exists() {
            return Err(ProfileLayoutError::AmbiguousTransaction);
        }
        let current_bytes = read_regular_file(&manifest_path)?;
        if sha256_hex(&current_bytes) != journal.previous_manifest_sha256 {
            return Err(ProfileLayoutError::AmbiguousTransaction);
        }
        fs::rename(&manifest_path, &backup_manifest)?;

        if fault == PublishFault::AfterBackupBeforeManifestCommit {
            return Err(ProfileLayoutError::Io(std::io::Error::other("simulated crash before manifest commit")));
        }

        fs::rename(&staged_manifest, &manifest_path)?;
        if !self.current_manifest_matches_hash(&journal.target_manifest_sha256)? {
            return Err(ProfileLayoutError::AmbiguousTransaction);
        }

        if fault == PublishFault::AfterManifestCommit {
            return Err(ProfileLayoutError::Io(std::io::Error::other("simulated crash after manifest commit")));
        }

        self.cleanup_committed(&journal)?;
        let manifest = self.read_manifest()?.ok_or(ProfileLayoutError::MissingManifest)?;
        self.apply_manifest_status(&manifest);
        Ok(())
    }

    fn rollback_journal(&mut self, journal: &ProfileLayoutJournal) -> Result<(), ProfileLayoutError> {
        if self.current_manifest_matches_hash(&journal.target_manifest_sha256)? {
            return Err(ProfileLayoutError::AlreadyCommitted);
        }

        let manifest_path = self.manifest_path();
        let backup_manifest = self.backup_manifest_path(journal.transaction_id);
        let current_is_old = self.current_manifest_matches_hash(&journal.previous_manifest_sha256)?;

        if manifest_path.exists() {
            if !current_is_old {
                return Err(ProfileLayoutError::AmbiguousTransaction);
            }
        } else {
            if !backup_manifest.exists() {
                return Err(ProfileLayoutError::AmbiguousTransaction);
            }
            let backup_bytes = read_regular_file(&backup_manifest)?;
            if sha256_hex(&backup_bytes) != journal.previous_manifest_sha256 {
                return Err(ProfileLayoutError::AmbiguousTransaction);
            }
            fs::rename(&backup_manifest, &manifest_path)?;
        }

        self.cleanup_uncommitted(journal)?;
        let manifest = self.read_manifest()?.ok_or(ProfileLayoutError::MissingManifest)?;
        self.apply_manifest_status(&manifest);
        Ok(())
    }

    fn cleanup_committed(&self, journal: &ProfileLayoutJournal) -> Result<(), ProfileLayoutError> {
        self.remove_owned_transaction_dir(&self.staging_dir(journal.transaction_id))?;
        self.remove_owned_transaction_dir(&self.backup_dir(journal.transaction_id))?;
        remove_regular_file_if_exists(&self.journal_path())?;
        Ok(())
    }

    fn cleanup_uncommitted(&self, journal: &ProfileLayoutJournal) -> Result<(), ProfileLayoutError> {
        self.remove_owned_transaction_dir(&self.staging_dir(journal.transaction_id))?;
        self.remove_owned_transaction_dir(&self.backup_dir(journal.transaction_id))?;
        remove_regular_file_if_exists(&self.journal_path())?;
        Ok(())
    }

    fn remove_owned_transaction_dir(&self, path: &Path) -> Result<(), ProfileLayoutError> {
        if !path.exists() {
            return Ok(());
        }
        ensure_plain_existing_directory(path)?;
        fs::remove_dir_all(path)?;
        Ok(())
    }

    fn staging_dir(&self, transaction_id: Uuid) -> PathBuf {
        self.staging_root().join(transaction_id.to_string())
    }

    fn backup_dir(&self, transaction_id: Uuid) -> PathBuf {
        self.backup_root().join(transaction_id.to_string())
    }

    fn backup_manifest_path(&self, transaction_id: Uuid) -> PathBuf {
        self.backup_dir(transaction_id).join(MANIFEST_FILE)
    }

    fn read_manifest(&self) -> Result<Option<ProfileLayoutManifest>, ProfileLayoutError> {
        let Some((manifest, _)) = self.read_manifest_bytes()? else {
            return Ok(None);
        };
        self.validate_manifest(&manifest)?;
        Ok(Some(manifest))
    }

    fn read_manifest_bytes(&self) -> Result<Option<(ProfileLayoutManifest, Vec<u8>)>, ProfileLayoutError> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(None);
        }
        let bytes = read_regular_file(&path)?;
        let manifest = serde_json::from_slice(&bytes)?;
        Ok(Some((manifest, bytes)))
    }

    fn read_journal(&self) -> Result<ProfileLayoutJournal, ProfileLayoutError> {
        let bytes = read_regular_file(&self.journal_path())?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn write_journal(&self, journal: &ProfileLayoutJournal) -> Result<(), ProfileLayoutError> {
        self.validate_journal(journal)?;
        write_json_safe(&self.journal_path(), journal)
    }

    fn validate_manifest(&self, manifest: &ProfileLayoutManifest) -> Result<(), ProfileLayoutError> {
        if manifest.schema != SCHEMA_VERSION || manifest.profile_uuid != self.profile_uuid() {
            return Err(ProfileLayoutError::IdentityMismatch);
        }
        Ok(())
    }

    fn validate_journal(&self, journal: &ProfileLayoutJournal) -> Result<(), ProfileLayoutError> {
        if journal.schema != SCHEMA_VERSION || journal.profile_uuid != self.profile_uuid() {
            return Err(ProfileLayoutError::IdentityMismatch);
        }
        if journal.target_generation != journal.from_generation.saturating_add(1) {
            return Err(ProfileLayoutError::GenerationMismatch);
        }
        Ok(())
    }

    fn current_manifest_matches_hash(&self, expected: &str) -> Result<bool, ProfileLayoutError> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(false);
        }
        let bytes = read_regular_file(&path)?;
        Ok(sha256_hex(&bytes) == expected)
    }

    fn apply_manifest_status(&mut self, manifest: &ProfileLayoutManifest) {
        self.status.state = manifest.state;
        self.status.generation = Some(manifest.generation);
        self.status.stock_fallback = manifest.state != ProfileLayoutState::Ready;
        self.status.fallback_reason = None;
    }

    fn mark_fallback(&mut self, state: ProfileLayoutState, reason: String) {
        self.status.state = state;
        self.status.stock_fallback = true;
        self.status.fallback_reason = Some(reason);
    }

    #[cfg(test)]
    fn publish_with_fault(
        &mut self,
        transaction_id: Uuid,
        fault: PublishFault,
    ) -> Result<(), ProfileLayoutError> {
        self.publish_inner(transaction_id, fault)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishFault {
    None,
    AfterBackupBeforeManifestCommit,
    AfterManifestCommit,
}

fn load_or_create_identity(control_root: &Path) -> Result<ProfileLayoutIdentity, ProfileLayoutError> {
    let path = control_root.join(IDENTITY_FILE);
    if path.exists() {
        let bytes = read_regular_file(&path)?;
        let identity: ProfileLayoutIdentity = serde_json::from_slice(&bytes)?;
        if identity.schema != SCHEMA_VERSION {
            return Err(ProfileLayoutError::IdentityMismatch);
        }
        return Ok(identity);
    }

    let identity = ProfileLayoutIdentity {
        schema: SCHEMA_VERSION,
        profile_uuid: new_uuid(),
    };
    write_json_safe(&path, &identity)?;
    Ok(identity)
}

fn ensure_control_paths(control_root: &Path) -> Result<(), ProfileLayoutError> {
    ensure_plain_existing_directory(control_root)?;
    for path in [
        control_root.join(IDENTITY_FILE),
        control_root.join(MANIFEST_FILE),
        control_root.join(JOURNAL_FILE),
    ] {
        if path.exists() {
            ensure_regular_file(&path)?;
        }
    }
    Ok(())
}

fn ensure_plain_directory(path: &Path) -> Result<(), ProfileLayoutError> {
    if path.exists() {
        return ensure_plain_existing_directory(path);
    }
    fs::create_dir_all(path)?;
    sync_parent(path)?;
    ensure_plain_existing_directory(path)
}

fn ensure_plain_existing_directory(path: &Path) -> Result<(), ProfileLayoutError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ProfileLayoutError::UnsafeFilesystem(path.to_path_buf()));
    }
    #[cfg(windows)]
    match junction::exists(path) {
        Ok(true) => return Err(ProfileLayoutError::UnsafeFilesystem(path.to_path_buf())),
        Ok(false) => {},
        Err(err) => return Err(ProfileLayoutError::Io(err)),
    }
    Ok(())
}

fn ensure_regular_file(path: &Path) -> Result<(), ProfileLayoutError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ProfileLayoutError::UnsafeFilesystem(path.to_path_buf()));
    }
    Ok(())
}

fn read_regular_file(path: &Path) -> Result<Vec<u8>, ProfileLayoutError> {
    ensure_regular_file(path)?;
    Ok(fs::read(path)?)
}

fn remove_regular_file_if_exists(path: &Path) -> Result<(), ProfileLayoutError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(ProfileLayoutError::UnsafeFilesystem(path.to_path_buf()));
            }
            fs::remove_file(path)?;
        },
        Err(err) if err.kind() == ErrorKind::NotFound => {},
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

fn write_json_safe<T: Serialize>(path: &Path, value: &T) -> Result<(), ProfileLayoutError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_bytes_safe(path, &bytes)
}

fn write_bytes_safe(path: &Path, bytes: &[u8]) -> Result<(), ProfileLayoutError> {
    if let Some(parent) = path.parent() {
        ensure_plain_directory(parent)?;
    }
    if path.exists() {
        ensure_regular_file(path)?;
    }

    let mut temp = path.to_path_buf();
    temp.set_extension(format!("{}.new", new_uuid()));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);

    if let Err(err) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(err.into());
    }
    sync_parent(path)?;
    Ok(())
}

fn sync_parent(path: &Path) -> Result<(), ProfileLayoutError> {
    if let Some(parent) = path.parent() {
        let dir = fs::File::open(parent)?;
        dir.sync_all()?;
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
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
            let path = std::env::temp_dir().join(format!("pandora-profile-layout-{label}-{}", new_uuid()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn ready_layout(root: &Path) -> ProfileLayout {
        let mut layout = ProfileLayout::load(root);
        assert!(layout.status.stock_fallback);
        let manifest = ProfileLayoutManifest::new_ready(layout.profile_uuid(), 1);
        layout.commit_initial_manifest(manifest).unwrap();
        assert_eq!(layout.status.state, ProfileLayoutState::Ready);
        layout
    }

    #[test]
    fn profile_state_is_isolated_per_instance_root() {
        let a = TestRoot::new("a");
        let b = TestRoot::new("b");
        let mut layout_a = ready_layout(&a.0);
        let layout_b = ready_layout(&b.0);

        assert_ne!(layout_a.profile_uuid(), layout_b.profile_uuid());
        let tx = layout_a
            .prepare_manifest(ProfileLayoutManifest::new_ready(layout_a.profile_uuid(), 2))
            .unwrap();
        assert_eq!(layout_a.status.state, ProfileLayoutState::Prepared);

        let reloaded_b = ProfileLayout::load(&b.0);
        assert_eq!(reloaded_b.status.state, ProfileLayoutState::Ready);
        assert_eq!(reloaded_b.status.generation, Some(1));
        assert!(!reloaded_b.journal_path().exists());
        assert!(layout_a.staging_dir(tx).exists());
    }

    #[test]
    fn corrupt_manifest_and_journal_fail_to_stock_without_overwrite() {
        let manifest_root = TestRoot::new("corrupt-manifest");
        let layout = ProfileLayout::load(&manifest_root.0);
        fs::write(layout.manifest_path(), b"{ definitely-not-json").unwrap();
        let manifest_bytes = fs::read(layout.manifest_path()).unwrap();

        let reloaded = ProfileLayout::load(&manifest_root.0);
        assert!(reloaded.status.stock_fallback);
        assert_eq!(reloaded.status.state, ProfileLayoutState::NeedsReconcile);
        assert_eq!(fs::read(reloaded.manifest_path()).unwrap(), manifest_bytes);

        let journal_root = TestRoot::new("corrupt-journal");
        let layout = ready_layout(&journal_root.0);
        fs::write(layout.journal_path(), b"not-json").unwrap();
        let journal_bytes = fs::read(layout.journal_path()).unwrap();
        let reloaded = ProfileLayout::load(&journal_root.0);
        assert!(reloaded.status.stock_fallback);
        assert_eq!(reloaded.status.state, ProfileLayoutState::Recovering);
        assert_eq!(fs::read(reloaded.journal_path()).unwrap(), journal_bytes);
    }

    #[test]
    fn crash_before_manifest_commit_rolls_back_idempotently() {
        let root = TestRoot::new("before-commit");
        let mut layout = ready_layout(&root.0);
        let tx = layout
            .prepare_manifest(ProfileLayoutManifest::new_ready(layout.profile_uuid(), 2))
            .unwrap();
        assert!(layout
            .publish_with_fault(tx, PublishFault::AfterBackupBeforeManifestCommit)
            .is_err());
        assert!(!layout.manifest_path().exists());
        assert!(layout.backup_manifest_path(tx).exists());

        let recovered = ProfileLayout::load(&root.0);
        assert_eq!(recovered.status.state, ProfileLayoutState::Ready);
        assert_eq!(recovered.status.generation, Some(1));
        assert!(!recovered.journal_path().exists());

        let recovered_again = ProfileLayout::load(&root.0);
        assert_eq!(recovered_again.status.generation, Some(1));
        assert_eq!(recovered_again.profile_uuid(), recovered.profile_uuid());
    }

    #[test]
    fn crash_after_manifest_commit_finishes_cleanup_idempotently() {
        let root = TestRoot::new("after-commit");
        let mut layout = ready_layout(&root.0);
        let tx = layout
            .prepare_manifest(ProfileLayoutManifest::new_ready(layout.profile_uuid(), 2))
            .unwrap();
        assert!(layout.publish_with_fault(tx, PublishFault::AfterManifestCommit).is_err());
        assert!(layout.manifest_path().exists());
        assert!(layout.journal_path().exists());

        let recovered = ProfileLayout::load(&root.0);
        assert_eq!(recovered.status.state, ProfileLayoutState::Ready);
        assert_eq!(recovered.status.generation, Some(2));
        assert!(!recovered.journal_path().exists());
        assert!(!recovered.staging_dir(tx).exists());
        assert!(!recovered.backup_dir(tx).exists());

        let recovered_again = ProfileLayout::load(&root.0);
        assert_eq!(recovered_again.status.generation, Some(2));
    }

    #[test]
    fn explicit_rollback_restores_previous_generation() {
        let root = TestRoot::new("rollback");
        let mut layout = ready_layout(&root.0);
        let tx = layout
            .prepare_manifest(ProfileLayoutManifest::new_ready(layout.profile_uuid(), 2))
            .unwrap();
        assert!(layout
            .publish_with_fault(tx, PublishFault::AfterBackupBeforeManifestCommit)
            .is_err());

        layout.rollback(tx).unwrap();
        assert_eq!(layout.status.state, ProfileLayoutState::Ready);
        assert_eq!(layout.status.generation, Some(1));
        assert!(!layout.journal_path().exists());
        assert!(!layout.staging_dir(tx).exists());
        assert!(!layout.backup_dir(tx).exists());
    }

    #[cfg(unix)]
    #[test]
    fn control_reparse_fails_to_stock_without_following_it() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new("reparse");
        let outside = TestRoot::new("outside");
        symlink(&outside.0, root.0.join(CONTROL_DIR)).unwrap();

        let layout = ProfileLayout::load(&root.0);
        assert!(layout.status.stock_fallback);
        assert_eq!(layout.status.state, ProfileLayoutState::NeedsReconcile);
        assert!(!outside.0.join(IDENTITY_FILE).exists());
    }
}
