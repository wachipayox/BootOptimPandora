//! Durable profile identity hardening for instance duplication and cross-process layout work.
//!
//! The OS lock, not the presence or contents of the lock file, is the liveness authority.
//! Lock files are intentionally persistent across crashes and are never deleted as "stale".

use std::{
    collections::BTreeMap,
    fs,
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::profile_layout_flow::{ManagedManifestEntry, ProfileLayoutManifest, ProfileLayoutState};

pub(crate) const CONTROL_DIR_NAME: &str = ".pandora-layout-v1";
const IDENTITY_FILE: &str = "identity.json";
const MANIFEST_FILE: &str = "manifest.json";
const JOURNAL_FILE: &str = "journal.json";
const LOCKS_DIR: &str = "locks";
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ProfileIdentity {
    schema: u32,
    profile_uuid: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LockOwner {
    schema: u32,
    profile_uuid: Uuid,
    pid: u32,
    acquired_unix_ms: u128,
    nonce: Uuid,
}

#[derive(Debug, Error)]
pub(crate) enum ProfileIdentityError {
    #[error("profile identity/lock I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("profile identity serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unsafe profile control filesystem object at {0:?}")]
    UnsafeFilesystem(PathBuf),
    #[error("persistent profile identity does not match its control state")]
    IdentityMismatch,
    #[error("persistent profile {0} is busy in another process")]
    Busy(Uuid),
    #[error("instance clone source is not a demonstrably Ready persistent profile: {0}")]
    CloneSourceNotReady(String),
    #[error("managed Ready snapshot changed while cloning: {0}")]
    CloneSnapshotChanged(String),
    #[error("OS-backed profile locking is unsupported on this platform")]
    UnsupportedLockPlatform,
}

#[derive(Debug)]
pub(crate) struct ProfileLayoutLockGuard {
    profile_uuid: Uuid,
    file: fs::File,
}

impl ProfileLayoutLockGuard {
    pub(crate) fn profile_uuid(&self) -> Uuid {
        self.profile_uuid
    }
}

impl Drop for ProfileLayoutLockGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            let _ = libc::flock(std::os::fd::AsRawFd::as_raw_fd(&self.file), libc::LOCK_UN);
        }
        // Windows exclusivity is tied to the File handle and is released when `file` drops.
    }
}

#[derive(Debug)]
pub(crate) enum ProfileCloneSource {
    Legacy,
    Ready {
        manifest: ProfileLayoutManifest,
        _lock: ProfileLayoutLockGuard,
    },
}

impl ProfileCloneSource {
    pub(crate) fn source_uuid(&self) -> Option<Uuid> {
        match self {
            Self::Legacy => None,
            Self::Ready { manifest, .. } => Some(manifest.profile_uuid),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ProfileCloneDestination {
    root: PathBuf,
    profile_uuid: Uuid,
    ready_manifest: Option<ProfileLayoutManifest>,
    _lock: ProfileLayoutLockGuard,
}

impl ProfileCloneDestination {
    pub(crate) fn profile_uuid(&self) -> Uuid {
        self.profile_uuid
    }

    /// Commits clone metadata only after all Ready managed bytes were copied and re-proven.
    pub(crate) fn finish(self) -> Result<Uuid, ProfileIdentityError> {
        if let Some(manifest) = &self.ready_manifest {
            verify_ready_live_snapshot(&self.root, manifest)?;
            write_new_synced(&control_root(&self.root).join(MANIFEST_FILE), &serde_json::to_vec_pretty(manifest)?)?;
        }
        Ok(self.profile_uuid)
    }
}

/// Creates/loads the durable UUID, then takes the per-UUID OS lock non-blockingly.
///
/// This must run before opening the journal/manifest, so recovery, reconcile, publication and
/// cleanup all execute while the guard is held.
pub(crate) fn acquire_or_initialize_profile_lock(
    instance_root: &Path,
) -> Result<ProfileLayoutLockGuard, ProfileIdentityError> {
    ensure_plain_existing_directory(instance_root)?;
    let control = control_root(instance_root);
    ensure_plain_directory(&control)?;
    let identity = load_or_create_identity(&control)?;
    acquire_lock_for_identity(&control, identity.profile_uuid)
}

fn acquire_existing_profile_lock(instance_root: &Path) -> Result<ProfileLayoutLockGuard, ProfileIdentityError> {
    ensure_plain_existing_directory(instance_root)?;
    let control = control_root(instance_root);
    ensure_plain_existing_directory(&control)?;
    let identity = load_existing_identity(&control)?;
    acquire_lock_for_identity(&control, identity.profile_uuid)
}

/// Classifies a clone source without mutating/recovering it. A persistent source must be Ready,
/// transaction-free and hash-proven. The source lock remains held for the entire filesystem copy.
pub(crate) fn prepare_profile_clone_source(instance_root: &Path) -> Result<ProfileCloneSource, ProfileIdentityError> {
    ensure_plain_existing_directory(instance_root)?;
    let control = control_root(instance_root);
    match fs::symlink_metadata(&control) {
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(ProfileCloneSource::Legacy),
        Err(err) => return Err(err.into()),
        Ok(_) => ensure_plain_existing_directory(&control)?,
    }

    let lock = acquire_existing_profile_lock(instance_root)?;
    reject_transaction_evidence(&control)?;
    let manifest = read_manifest(&control)?;
    if manifest.schema != SCHEMA_VERSION
        || manifest.profile_uuid != lock.profile_uuid()
        || manifest.state != ProfileLayoutState::Ready
    {
        return Err(ProfileIdentityError::CloneSourceNotReady(
            "identity/schema/state is not committed Ready".to_string(),
        ));
    }
    verify_ready_live_snapshot(instance_root, &manifest)?;

    Ok(ProfileCloneSource::Ready { manifest, _lock: lock })
}

/// Creates a fresh UUID namespace in an already-created empty destination before file copying.
/// The source control directory must be skipped by the copier; no journal/staging/backup is copied.
pub(crate) fn begin_profile_clone_destination(
    destination_root: &Path,
    source: &ProfileCloneSource,
) -> Result<ProfileCloneDestination, ProfileIdentityError> {
    ensure_plain_existing_directory(destination_root)?;
    let control = control_root(destination_root);
    ensure_plain_directory(&control)?;
    if control.join(IDENTITY_FILE).exists() {
        return Err(ProfileIdentityError::CloneSourceNotReady(
            "destination already has persistent identity state".to_string(),
        ));
    }

    let profile_uuid = new_uuid();
    let identity = ProfileIdentity {
        schema: SCHEMA_VERSION,
        profile_uuid,
    };
    write_new_synced(&control.join(IDENTITY_FILE), &serde_json::to_vec_pretty(&identity)?)?;
    let lock = acquire_lock_for_identity(&control, profile_uuid)?;

    let ready_manifest = match source {
        ProfileCloneSource::Legacy => None,
        ProfileCloneSource::Ready { manifest, .. } => Some(ProfileLayoutManifest {
            schema: SCHEMA_VERSION,
            profile_uuid,
            generation: 1,
            state: ProfileLayoutState::Ready,
            managed_input_fingerprint: manifest.managed_input_fingerprint.clone(),
            sync_identity: manifest.sync_identity.clone(),
            sandbox_policy: manifest.sandbox_policy.clone(),
            managed_entries: manifest.managed_entries.clone(),
            transaction_id: None,
        }),
    };

    Ok(ProfileCloneDestination {
        root: destination_root.to_path_buf(),
        profile_uuid,
        ready_manifest,
        _lock: lock,
    })
}

fn reject_transaction_evidence(control: &Path) -> Result<(), ProfileIdentityError> {
    if path_exists_no_follow(&control.join(JOURNAL_FILE))? {
        return Err(ProfileIdentityError::CloneSourceNotReady("journal is present".to_string()));
    }
    for name in ["staging", "backup", "conflicts"] {
        let path = control.join(name);
        if directory_has_entries(&path)? {
            return Err(ProfileIdentityError::CloneSourceNotReady(format!(
                "{name} contains unresolved transaction/conflict evidence"
            )));
        }
    }
    Ok(())
}

fn directory_has_entries(path: &Path) -> Result<bool, ProfileIdentityError> {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
        Ok(_) => {
            ensure_plain_existing_directory(path)?;
            Ok(fs::read_dir(path)?.next().transpose()?.is_some())
        },
    }
}

fn read_manifest(control: &Path) -> Result<ProfileLayoutManifest, ProfileIdentityError> {
    let path = control.join(MANIFEST_FILE);
    match fs::symlink_metadata(&path) {
        Err(err) if err.kind() == ErrorKind::NotFound => {
            Err(ProfileIdentityError::CloneSourceNotReady("manifest is missing".to_string()))
        },
        Err(err) => Err(err.into()),
        Ok(_) => {
            ensure_regular_file(&path)?;
            Ok(serde_json::from_slice(&fs::read(path)?)?)
        },
    }
}

fn verify_ready_live_snapshot(
    instance_root: &Path,
    manifest: &ProfileLayoutManifest,
) -> Result<(), ProfileIdentityError> {
    let live_root = instance_root.join(".minecraft");
    ensure_plain_existing_directory(&live_root)?;
    for (relative, entry) in &manifest.managed_entries {
        let path = safe_managed_path(&live_root, relative)?;
        ensure_plain_managed_parents(&live_root, relative)?;
        ensure_regular_file(&path)?;
        let actual = hash_regular_file(&path)?;
        if actual != entry.applied_hash.to_ascii_lowercase() {
            return Err(ProfileIdentityError::CloneSnapshotChanged(relative.clone()));
        }
    }
    Ok(())
}

fn safe_managed_path(root: &Path, relative: &str) -> Result<PathBuf, ProfileIdentityError> {
    validate_managed_relative(relative)?;
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        path.push(segment);
    }
    Ok(path)
}

fn validate_managed_relative(relative: &str) -> Result<(), ProfileIdentityError> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.ends_with('/')
        || relative.contains('\\')
        || relative.contains(':')
        || relative.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(ProfileIdentityError::CloneSourceNotReady(format!("unsafe managed path: {relative}")));
    }
    Ok(())
}

fn ensure_plain_managed_parents(root: &Path, relative: &str) -> Result<(), ProfileIdentityError> {
    validate_managed_relative(relative)?;
    ensure_plain_existing_directory(root)?;
    let mut current = root.to_path_buf();
    let segments: Vec<_> = relative.split('/').collect();
    for segment in &segments[..segments.len().saturating_sub(1)] {
        current.push(segment);
        ensure_plain_existing_directory(&current)?;
    }
    Ok(())
}

fn hash_regular_file(path: &Path) -> Result<String, ProfileIdentityError> {
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

fn control_root(instance_root: &Path) -> PathBuf {
    instance_root.join(CONTROL_DIR_NAME)
}

fn load_or_create_identity(control: &Path) -> Result<ProfileIdentity, ProfileIdentityError> {
    let path = control.join(IDENTITY_FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => return load_existing_identity(control),
        Err(err) if err.kind() == ErrorKind::NotFound => {},
        Err(err) => return Err(err.into()),
    }

    let identity = ProfileIdentity {
        schema: SCHEMA_VERSION,
        profile_uuid: new_uuid(),
    };
    let bytes = serde_json::to_vec_pretty(&identity)?;
    match write_new_synced(&path, &bytes) {
        Ok(()) => Ok(identity),
        Err(ProfileIdentityError::Io(err)) if err.kind() == ErrorKind::AlreadyExists => load_existing_identity(control),
        Err(err) => Err(err),
    }
}

fn load_existing_identity(control: &Path) -> Result<ProfileIdentity, ProfileIdentityError> {
    let path = control.join(IDENTITY_FILE);
    ensure_regular_file(&path)?;
    let identity: ProfileIdentity = serde_json::from_slice(&fs::read(path)?)?;
    if identity.schema != SCHEMA_VERSION {
        return Err(ProfileIdentityError::IdentityMismatch);
    }
    Ok(identity)
}

fn acquire_lock_for_identity(
    control: &Path,
    profile_uuid: Uuid,
) -> Result<ProfileLayoutLockGuard, ProfileIdentityError> {
    let locks = control.join(LOCKS_DIR);
    ensure_plain_directory(&locks)?;
    let path = locks.join(format!("{profile_uuid}.lock"));
    let mut file = open_os_exclusive_lock(&path, profile_uuid)?;
    write_lock_owner(&mut file, profile_uuid)?;
    Ok(ProfileLayoutLockGuard { profile_uuid, file })
}

#[cfg(unix)]
fn open_os_exclusive_lock(path: &Path, profile_uuid: Uuid) -> Result<fs::File, ProfileIdentityError> {
    use std::os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    };

    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        if matches!(err.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN) {
            return Err(ProfileIdentityError::Busy(profile_uuid));
        }
        return Err(err.into());
    }

    let path_metadata = fs::symlink_metadata(path)?;
    let file_metadata = file.metadata()?;
    if !path_metadata.is_file()
        || path_metadata.file_type().is_symlink()
        || path_metadata.dev() != file_metadata.dev()
        || path_metadata.ino() != file_metadata.ino()
    {
        return Err(ProfileIdentityError::UnsafeFilesystem(path.to_path_buf()));
    }
    Ok(file)
}

#[cfg(windows)]
fn open_os_exclusive_lock(path: &Path, profile_uuid: Uuid) -> Result<fs::File, ProfileIdentityError> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;

    let file = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if matches!(err.raw_os_error(), Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)) => {
            return Err(ProfileIdentityError::Busy(profile_uuid));
        },
        Err(err) => return Err(err.into()),
    };

    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(ProfileIdentityError::UnsafeFilesystem(path.to_path_buf()));
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
fn open_os_exclusive_lock(_path: &Path, _profile_uuid: Uuid) -> Result<fs::File, ProfileIdentityError> {
    Err(ProfileIdentityError::UnsupportedLockPlatform)
}

fn write_lock_owner(file: &mut fs::File, profile_uuid: Uuid) -> Result<(), ProfileIdentityError> {
    let acquired_unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let owner = LockOwner {
        schema: SCHEMA_VERSION,
        profile_uuid,
        pid: std::process::id(),
        acquired_unix_ms,
        nonce: new_uuid(),
    };
    let bytes = serde_json::to_vec_pretty(&owner)?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn path_exists_no_follow(path: &Path) -> Result<bool, ProfileIdentityError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn ensure_plain_directory(path: &Path) -> Result<(), ProfileIdentityError> {
    if path_exists_no_follow(path)? {
        return ensure_plain_existing_directory(path);
    }
    fs::create_dir_all(path)?;
    sync_parent(path)?;
    ensure_plain_existing_directory(path)
}

fn ensure_plain_existing_directory(path: &Path) -> Result<(), ProfileIdentityError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ProfileIdentityError::UnsafeFilesystem(path.to_path_buf()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ProfileIdentityError::UnsafeFilesystem(path.to_path_buf()));
        }
    }
    Ok(())
}

fn ensure_regular_file(path: &Path) -> Result<(), ProfileIdentityError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ProfileIdentityError::UnsafeFilesystem(path.to_path_buf()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ProfileIdentityError::UnsafeFilesystem(path.to_path_buf()));
        }
    }
    Ok(())
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), ProfileIdentityError> {
    if let Some(parent) = path.parent() {
        ensure_plain_directory(parent)?;
    }
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    sync_parent(path)
}

fn sync_parent(path: &Path) -> Result<(), ProfileIdentityError> {
    if let Some(parent) = path.parent() {
        let dir = fs::File::open(parent)?;
        dir.sync_all()?;
    }
    Ok(())
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
    use std::{
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!("pandora-profile-identity-{label}-{}", new_uuid()));
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join(".minecraft")).unwrap();
            fs::create_dir(root.join(".minecraft/mods")).unwrap();
            Self(root)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn profile_layout_lock_second_acquisition_fails_and_release_reacquires() {
        let root = TestRoot::new("second-acquire");
        let first = acquire_or_initialize_profile_lock(&root.0).unwrap();
        let uuid = first.profile_uuid();
        assert!(matches!(
            acquire_existing_profile_lock(&root.0),
            Err(ProfileIdentityError::Busy(id)) if id == uuid
        ));
        drop(first);
        let reacquired = acquire_existing_profile_lock(&root.0).unwrap();
        assert_eq!(reacquired.profile_uuid(), uuid);
    }

    #[test]
    fn profile_layout_locks_for_distinct_uuids_do_not_block_each_other() {
        let a = TestRoot::new("parallel-a");
        let b = TestRoot::new("parallel-b");
        let lock_a = acquire_or_initialize_profile_lock(&a.0).unwrap();
        let lock_b = acquire_or_initialize_profile_lock(&b.0).unwrap();
        assert_ne!(lock_a.profile_uuid(), lock_b.profile_uuid());
    }

    #[test]
    fn profile_layout_lock_child_helper() {
        let Some(root) = std::env::var_os("BOOTOPTIM_PROFILE_LOCK_CHILD_ROOT") else {
            return;
        };
        let ready = std::env::var_os("BOOTOPTIM_PROFILE_LOCK_CHILD_READY").unwrap();
        let _guard = acquire_existing_profile_lock(Path::new(&root)).unwrap();
        fs::write(ready, b"locked\n").unwrap();
        thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn profile_layout_lock_is_released_when_other_process_dies() {
        let root = TestRoot::new("child-crash");
        let initial = acquire_or_initialize_profile_lock(&root.0).unwrap();
        let uuid = initial.profile_uuid();
        drop(initial);

        let ready = root.0.join("child-ready");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("profile_layout_identity::tests::profile_layout_lock_child_helper")
            .arg("--nocapture")
            .env("BOOTOPTIM_PROFILE_LOCK_CHILD_ROOT", &root.0)
            .env("BOOTOPTIM_PROFILE_LOCK_CHILD_READY", &ready)
            .spawn()
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if !ready.exists() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child did not acquire profile lock");
        }
        assert!(matches!(
            acquire_existing_profile_lock(&root.0),
            Err(ProfileIdentityError::Busy(id)) if id == uuid
        ));

        child.kill().unwrap();
        child.wait().unwrap();
        let reacquired = acquire_existing_profile_lock(&root.0).unwrap();
        assert_eq!(reacquired.profile_uuid(), uuid);
    }

    #[cfg(unix)]
    #[test]
    fn profile_layout_lock_rejects_symlink_lock_path() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new("symlink-lock");
        let initial = acquire_or_initialize_profile_lock(&root.0).unwrap();
        let uuid = initial.profile_uuid();
        drop(initial);
        let lock_path = control_root(&root.0).join(LOCKS_DIR).join(format!("{uuid}.lock"));
        fs::remove_file(&lock_path).unwrap();
        let outside = root.0.join("outside-lock");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, &lock_path).unwrap();
        assert!(acquire_existing_profile_lock(&root.0).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"outside");
    }

    #[test]
    fn profile_layout_legacy_clone_mints_identity_without_guessing_ready_state() {
        let source = TestRoot::new("legacy-source");
        let destination = TestRoot::new("legacy-destination");
        let plan = prepare_profile_clone_source(&source.0).unwrap();
        assert!(matches!(plan, ProfileCloneSource::Legacy));
        let destination_state = begin_profile_clone_destination(&destination.0, &plan).unwrap();
        let clone_uuid = destination_state.finish().unwrap();
        assert!(!control_root(&source.0).exists());
        assert!(control_root(&destination.0).join(IDENTITY_FILE).is_file());
        assert!(!control_root(&destination.0).join(MANIFEST_FILE).exists());
        let identity = load_existing_identity(&control_root(&destination.0)).unwrap();
        assert_eq!(identity.profile_uuid, clone_uuid);
    }

    #[test]
    fn profile_layout_persistent_unknown_or_non_ready_clone_is_rejected() {
        let unknown = TestRoot::new("unknown-source");
        ensure_plain_directory(&control_root(&unknown.0)).unwrap();
        assert!(prepare_profile_clone_source(&unknown.0).is_err());

        let non_ready = TestRoot::new("non-ready-source");
        let guard = acquire_or_initialize_profile_lock(&non_ready.0).unwrap();
        let uuid = guard.profile_uuid();
        drop(guard);
        let manifest = ProfileLayoutManifest {
            schema: SCHEMA_VERSION,
            profile_uuid: uuid,
            generation: 1,
            state: ProfileLayoutState::Prepared,
            managed_input_fingerprint: String::new(),
            sync_identity: String::new(),
            sandbox_policy: String::new(),
            managed_entries: BTreeMap::<String, ManagedManifestEntry>::new(),
            transaction_id: Some(new_uuid()),
        };
        write_new_synced(
            &control_root(&non_ready.0).join(MANIFEST_FILE),
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            prepare_profile_clone_source(&non_ready.0),
            Err(ProfileIdentityError::CloneSourceNotReady(_))
        ));
    }
}
