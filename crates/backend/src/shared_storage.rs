//! Filesystem boundary for local shared data. Never follow an unknown link.
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(crate) struct StorageLock(fs::File);
impl Drop for StorageLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            let _ = libc::flock(std::os::fd::AsRawFd::as_raw_fd(&self.0), libc::LOCK_UN);
        }
    }
}
pub(crate) fn lock(root: &Path) -> io::Result<StorageLock> {
    if is_link(root) {
        return Err(invalid("Shared storage owner root is a link"));
    }
    fs::create_dir_all(root)?;
    let path = root.join(".pandora-shared-data.lock");
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0).custom_flags(0x00200000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    if is_link(&path) {
        return Err(invalid("Shared storage lock is linked"));
    }
    let file = options.open(&path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file.metadata()?.file_attributes() & 0x400 != 0 {
            return Err(invalid("Shared lock is a reparse point"));
        }
    }
    #[cfg(unix)]
    {
        if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(StorageLock(file))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub(crate) fn is_link(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & 0x400 != 0);
    }
    #[cfg(not(windows))]
    {
        fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    }
}
fn plain(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link(path) || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(invalid("Shared data contains a link or unsupported filesystem object"));
    }
    Ok(metadata)
}
pub(crate) fn checked_path(root: &Path, relative: &str) -> io::Result<PathBuf> {
    if Path::new(relative).is_absolute() || relative.starts_with('/') {
        return Err(invalid("Absolute shared-data path"));
    }
    let safe = bridge::safe_path::SafePath::new(relative).ok_or_else(|| invalid("Unsafe shared-data path"))?;
    if safe.as_str().is_empty()
        || safe.as_str() == "."
        || safe.as_str().split('/').any(|part| part.starts_with(".pandora-"))
    {
        return Err(invalid("Reserved shared-data path"));
    }
    let mut parent = root.to_path_buf();
    if !plain(root)?.is_dir() {
        return Err(invalid("Shared-data root is not a plain directory"));
    }
    let components: Vec<_> = safe.as_str().split('/').collect();
    for component in &components[..components.len() - 1] {
        parent.push(component);
        match plain(&parent) {
            Ok(m) if m.is_dir() => {},
            Ok(_) => return Err(invalid("Shared-data parent is not a directory")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => break,
            Err(e) => return Err(e),
        }
    }
    Ok(safe.to_path(root))
}
pub(crate) fn targeting(path: &Path, target: &Path) -> bool {
    is_link(path)
        && fs::canonicalize(path)
            .ok()
            .zip(fs::canonicalize(target).ok())
            .is_some_and(|(a, b)| a == b)
}
/// Recognize only the exact configured target of a broken legacy link.
/// This handles Windows app-filesystem namespaces without claiming foreign links.
pub(crate) fn known_broken_link(local: &Path, target: &Path) -> bool {
    if !is_link(local) || fs::canonicalize(local).is_ok() || !target.is_dir() {
        return false;
    }
    #[cfg(windows)]
    let original = junction::get_target(local).ok();
    #[cfg(not(windows))]
    let original = fs::read_link(local).ok();
    fn key(path: &Path) -> String {
        let raw = path.to_string_lossy().replace('\\', "/");
        let raw = raw.strip_prefix("//?/").unwrap_or(&raw);
        #[cfg(windows)]
        {
            raw.to_lowercase()
        }
        #[cfg(not(windows))]
        {
            raw.to_owned()
        }
    }
    original.is_some_and(|original| key(&original) == key(target))
}
pub(crate) fn targeting_or_broken(local: &Path, target: &Path) -> bool {
    targeting(local, target) || known_broken_link(local, target)
}
fn repair_known_link(local: &Path, target: &Path) -> io::Result<()> {
    if known_broken_link(local, target) {
        unlink_directory(local)?;
        link_directory(target, local)?;
    }
    Ok(())
}

fn link_directory(target: &Path, local: &Path) -> io::Result<()> {
    let target = fs::canonicalize(target)?;
    #[cfg(windows)]
    {
        junction::create(target, local)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, local)
    }
}
fn unlink_directory(local: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        junction::delete(local)
    }
    #[cfg(unix)]
    {
        fs::remove_file(local)
    }
}
pub(crate) fn hash(path: &Path) -> io::Result<Option<String>> {
    match plain(path) {
        Ok(m) if m.is_file() => {},
        Ok(_) => return Err(invalid("Shared file is not a plain file")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    }
    let mut input = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(Some(hex::encode(digest.finalize())))
}
pub(crate) fn copy_file(source: &Path, target: &Path) -> io::Result<()> {
    if !plain(source)?.is_file() {
        return Err(invalid("Not a plain source file"));
    }
    if is_link(target) {
        return Err(invalid("Refusing to replace a linked shared file"));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let staged = target.with_extension(format!("{}.new", Uuid::new_v4()));
    fs::copy(source, &staged)?;
    fs::OpenOptions::new().write(true).open(&staged)?.sync_all()?;
    if let Err(e) = fs::rename(&staged, target) {
        let _ = fs::remove_file(&staged);
        return Err(e);
    }
    Ok(())
}
fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    if !plain(source)?.is_dir() {
        return Err(invalid("Not a plain source directory"));
    }
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if plain(&from)?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            copy_file(&from, &to)?;
        }
    }
    Ok(())
}
fn move_plain(source: &Path, target: &Path) -> io::Result<()> {
    let metadata = plain(source)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::symlink_metadata(target).is_ok() {
        return Err(invalid("Recovery destination already exists"));
    }
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            if metadata.is_dir() {
                copy_tree(source, target)?;
                fs::remove_dir_all(source)
            } else {
                copy_file(source, target)?;
                fs::remove_file(source)
            }
        },
        Err(e) => Err(e),
    }
}
fn merge_directory(local: &Path, shared: &Path, backup: &Path) -> io::Result<()> {
    if !plain(local)?.is_dir() || !plain(shared)?.is_dir() {
        return Err(invalid("Invalid shared-data merge directory"));
    }
    for entry in fs::read_dir(local)? {
        let entry = entry?;
        let source = entry.path();
        let target = shared.join(entry.file_name());
        let saved = backup.join(entry.file_name());
        let meta = plain(&source)?;
        match fs::symlink_metadata(&target) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => move_plain(&source, &target)?,
            Err(e) => return Err(e),
            Ok(_) if meta.is_dir() && plain(&target)?.is_dir() => {
                merge_directory(&source, &target, &saved)?;
                fs::remove_dir(&source)?;
            },
            Ok(_) => {
                plain(&target)?;
                // The shared copy wins; every differing local object remains recoverable.
                if meta.is_file() && target.is_file() && hash(&source)? == hash(&target)? {
                    fs::remove_file(&source)?;
                } else {
                    move_plain(&source, &saved)?;
                }
            },
        }
    }
    Ok(())
}

/// Existing correct links are O(1); merging is only the first attachment/migration.
pub(crate) fn attach_folder(local: &Path, shared: &Path, backups: &Path) -> io::Result<()> {
    repair_known_link(local, shared)?;
    if targeting(local, shared) {
        return Ok(());
    }
    if is_link(local) {
        return Err(invalid("Shared-data path is linked to another owner"));
    }
    if is_link(shared) {
        return Err(invalid("Shared storage must not be a link"));
    }
    let local_exists = fs::symlink_metadata(local).is_ok();
    if local_exists && !plain(local)?.is_dir() {
        return Err(invalid("Shared folder is a file"));
    }
    let recovery = backups.join(Uuid::new_v4().to_string());
    if !shared.exists() {
        if local_exists {
            move_plain(local, shared)?;
        } else {
            fs::create_dir_all(shared)?;
        }
    } else if local_exists {
        merge_directory(local, shared, &recovery)?;
        fs::remove_dir(local)?;
    }
    if let Some(parent) = local.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Err(error) = link_directory(shared, local) {
        // Data stays in shared storage; restore a usable local copy on link failure.
        let restore = copy_tree(shared, local);
        return Err(io::Error::new(
            error.kind(),
            format!("Unable to attach shared folder: {error}; restore={restore:?}"),
        ));
    }
    Ok(())
}
/// Materialize only a recognized owned link. The shared source is never removed.
pub(crate) fn detach_folder(local: &Path, shared: &Path, backups: &Path) -> io::Result<()> {
    repair_known_link(local, shared)?;
    if !targeting(local, shared) {
        return if is_link(local) {
            Err(invalid("Cannot detach an unknown shared-data link"))
        } else {
            Ok(())
        };
    }
    let staged = backups.join(Uuid::new_v4().to_string()).join("materialized");
    copy_tree(shared, &staged)?;
    unlink_directory(local)?;
    if let Err(error) = fs::rename(&staged, local) {
        let restore = link_directory(shared, local);
        return Err(io::Error::new(
            error.kind(),
            format!("Unable to materialize shared folder: {error}; relink={restore:?}; copy={staged:?}"),
        ));
    }
    Ok(())
}
/// First join uses the group's canonical file. Differing local bytes are backed up.
pub(crate) fn attach_file(local: &Path, shared: &Path, backups: &Path) -> io::Result<Option<String>> {
    let current = hash(local)?;
    let canonical = hash(shared)?;
    match (current.as_ref(), canonical.as_ref()) {
        (Some(_), None) => copy_file(local, shared)?,
        (Some(a), Some(b)) if a != b => {
            copy_file(
                local,
                &backups
                    .join(Uuid::new_v4().to_string())
                    .join(local.file_name().ok_or_else(|| invalid("Missing shared filename"))?),
            )?;
            copy_file(shared, local)?;
        },
        (None, Some(_)) => copy_file(shared, local)?,
        _ => {},
    }
    hash(shared)
}
/// Publish a changed game-written file only if another member has not changed the seed.
/// On a concurrent conflict the group wins and local bytes are retained in recovery.
pub(crate) fn publish_file(
    local: &Path,
    shared: &Path,
    seed: Option<&str>,
    backups: &Path,
) -> io::Result<Option<String>> {
    let current = hash(local)?;
    let canonical = hash(shared)?;
    if current == canonical {
        return Ok(canonical);
    }
    if current.as_deref() != seed && current.is_some() && canonical.as_deref() == seed {
        copy_file(local, shared)?;
        return Ok(current);
    }
    attach_file(local, shared, backups)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("pandora-sharing-{}", Uuid::new_v4()));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            assert!(self.0.starts_with(std::env::temp_dir()));
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn folder_join_preserves_shared_bytes_and_backs_up_local_conflict() {
        let t = Temp::new();
        let local = t.0.join("local");
        let shared = t.0.join("shared");
        let backup = t.0.join("backup");
        fs::create_dir(&local).unwrap();
        fs::create_dir(&shared).unwrap();
        fs::write(local.join("same"), b"local").unwrap();
        fs::write(local.join("new"), b"new").unwrap();
        fs::write(shared.join("same"), b"group").unwrap();
        attach_folder(&local, &shared, &backup).unwrap();
        assert!(targeting(&local, &shared));
        assert_eq!(fs::read(local.join("same")).unwrap(), b"group");
        assert_eq!(fs::read(shared.join("new")).unwrap(), b"new");
        let saved = fs::read_dir(&backup).unwrap().next().unwrap().unwrap().path().join("same");
        assert_eq!(fs::read(saved).unwrap(), b"local");
        detach_folder(&local, &shared, &backup).unwrap();
        assert!(!is_link(&local));
        fs::write(local.join("same"), b"independent").unwrap();
        assert_eq!(fs::read(shared.join("same")).unwrap(), b"group");
    }
    #[test]
    fn file_join_and_concurrent_write_keep_group_and_recover_local() {
        let t = Temp::new();
        let local = t.0.join("local");
        let shared = t.0.join("shared");
        let backup = t.0.join("backup");
        fs::write(&local, b"local").unwrap();
        fs::write(&shared, b"group").unwrap();
        let seed = attach_file(&local, &shared, &backup).unwrap();
        assert_eq!(fs::read(&local).unwrap(), b"group");
        fs::write(&local, b"game").unwrap();
        publish_file(&local, &shared, seed.as_deref(), &backup).unwrap();
        assert_eq!(fs::read(&shared).unwrap(), b"game");
        let seed = hash(&shared).unwrap();
        fs::write(&local, b"mine").unwrap();
        fs::write(&shared, b"other").unwrap();
        publish_file(&local, &shared, seed.as_deref(), &backup).unwrap();
        assert_eq!(fs::read(&local).unwrap(), b"other");
        assert!(fs::read_dir(&backup).unwrap().count() >= 2);
    }
    #[test]
    fn unknown_link_is_neither_followed_nor_detached() {
        let t = Temp::new();
        let foreign = t.0.join("foreign");
        let shared = t.0.join("shared");
        let local = t.0.join("local");
        fs::create_dir(&foreign).unwrap();
        fs::create_dir(&shared).unwrap();
        fs::write(foreign.join("keep"), b"safe").unwrap();
        link_directory(&foreign, &local).unwrap();
        assert!(attach_folder(&local, &shared, &t.0.join("backup")).is_err());
        assert!(detach_folder(&local, &shared, &t.0.join("backup")).is_err());
        assert_eq!(fs::read(foreign.join("keep")).unwrap(), b"safe");
    }
}
