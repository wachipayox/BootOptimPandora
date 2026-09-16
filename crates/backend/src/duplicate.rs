use sha1::Digest;
use std::{
    fs,
    io::{Error, ErrorKind, Read, Result, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use bridge::{
    instance::InstanceID,
    modal_action::{ModalAction, ProgressTrackerFinishType},
};

use crate::{
    BackendState,
    profile_layout_identity::{CONTROL_DIR_NAME, begin_profile_clone_destination, prepare_profile_clone_source},
};

fn find_content_library_path(content_library_dir: &Path, hash: [u8; 20], path: &Path) -> Option<PathBuf> {
    let extension = path.extension().and_then(|s| s.to_str());
    let lib_path = crate::fs::create_content_library_path(content_library_dir, hash, extension);
    if lib_path.exists() {
        return Some(lib_path);
    }

    let disabled_extension = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|filename| filename.strip_suffix(".disabled"))
        .and_then(|base| Path::new(base).extension())
        .and_then(|s| s.to_str());
    let lib_path = crate::fs::create_content_library_path(content_library_dir, hash, disabled_extension);
    lib_path.exists().then_some(lib_path)
}

fn hash_file(path: &Path, buf: &mut [u8], check_cancel: &dyn Fn() -> Result<()>) -> Result<[u8; 20]> {
    let mut file = fs::File::open(path)?;
    let mut hasher = sha1::Sha1::default();
    loop {
        check_cancel()?;
        let read = file.read(buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher.finalize().into())
}

fn copy_file(from: &Path, to: &Path, buf: &mut [u8], check_cancel: &dyn Fn() -> Result<()>) -> Result<u64> {
    let mut src = fs::File::open(from)?;
    let mut dst = fs::File::create(to)?;
    let mut total = 0_u64;
    loop {
        check_cancel()?;
        let read = src.read(buf)?;
        if read == 0 {
            break;
        }
        dst.write_all(&buf[..read])?;
        total += read as u64;
    }

    let metadata = fs::metadata(from)?;
    fs::set_permissions(to, metadata.permissions())?;
    if let Ok(modified) = metadata.modified() {
        let _ = dst.set_times(fs::FileTimes::new().set_modified(modified));
    }

    Ok(total)
}

fn duplicate_with_content_library(
    from: &Path,
    to: &Path,
    content_library_dir: &Path,
    progress: &dyn Fn(u64, u64),
    check_cancel: &dyn Fn() -> Result<()>,
) -> Result<()> {
    let from = from.canonicalize()?;
    if !from.is_dir() {
        return Err(ErrorKind::NotADirectory.into());
    }
    if !to.is_dir() {
        return Err(ErrorKind::AlreadyExists.into());
    }

    let mut directories = Vec::new();
    let mut files = Vec::new();
    let mut internal_symlinks = Vec::new();
    let mut external_symlinks = Vec::new();
    #[cfg(windows)]
    let mut internal_junctions = Vec::new();
    #[cfg(windows)]
    let mut external_junctions = Vec::new();

    let mut directories_to_visit = Vec::new();
    directories_to_visit.push((from.to_path_buf(), 0));

    while let Some((directory, depth)) = directories_to_visit.pop() {
        check_cancel()?;
        let read_dir = fs::read_dir(directory)?;
        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            let Ok(relative) = path.strip_prefix(&from) else {
                return Err(Error::new(ErrorKind::Other, format!("{path:?} is not a child of {from:?}")));
            };
            // Persistent profile control state is identity/transaction state, not instance
            // payload. The destination namespace is created separately with a fresh UUID.
            if relative == Path::new(CONTROL_DIR_NAME) {
                continue;
            }
            #[cfg(windows)]
            if let Ok(target) = junction::get_target(&path) {
                if let Ok(internal) = target.strip_prefix(&from) {
                    internal_junctions.push((relative.to_path_buf(), internal.to_path_buf()));
                } else {
                    external_junctions.push((relative.to_path_buf(), target));
                }
                continue;
            }
            if file_type.is_symlink() {
                let target = fs::read_link(&path)?;
                if let Ok(internal) = target.strip_prefix(&from) {
                    internal_symlinks.push((relative.to_path_buf(), internal.to_path_buf()));
                } else {
                    external_symlinks.push((relative.to_path_buf(), target));
                }
            } else if file_type.is_file() {
                files.push((relative.to_path_buf(), path));
            } else if file_type.is_dir() {
                if depth >= 256 {
                    return Err(ErrorKind::QuotaExceeded.into());
                }

                directories.push(relative.to_path_buf());
                directories_to_visit.push((path, depth + 1));
            }
        }
    }

    let total_files = files.len() as u64;
    progress(0, total_files);

    for directory in directories {
        check_cancel()?;
        _ = fs::create_dir(to.join(directory));
    }

    let mut files_done = 0_u64;
    let mut buf = vec![0_u8; 128 * 1024];
    for (relative, source_path) in &files {
        check_cancel()?;
        let dest = to.join(relative);

        if reflink_copy::reflink(source_path, &dest).is_ok() {
            files_done += 1;
            progress(files_done, total_files);
            continue;
        }

        // If the source_path was hard linked from the content library
        // We will make the duplicated file also hard linked
        if let Ok(source_metadata) = crate::fs::FileMetadata::new(source_path)
            && source_metadata.number_of_links() > 1
        {
            if let Ok(hash) = hash_file(source_path, &mut buf, check_cancel) {
                if let Some(lib_path) = find_content_library_path(content_library_dir, hash, source_path) {
                    if let Ok(lib_metadata) = crate::fs::FileMetadata::new(&lib_path)
                        && source_metadata.is_same(&lib_metadata)
                    {
                        if crate::fs::fastcopy(&lib_path, &dest, false, true).is_ok() {
                            files_done += 1;
                            progress(files_done, total_files);
                            continue;
                        }
                    }
                }
            }
        }

        copy_file(source_path, &dest, &mut buf, check_cancel)?;
        files_done += 1;
        progress(files_done, total_files);
    }

    for (relative, internal) in &internal_symlinks {
        let dest = to.join(relative);
        let target = to.join(internal);
        if let Err(err) = crate::fs::symlink_dir_or_file(&target, &dest) {
            return Err(err);
        }
    }
    for (relative, target) in &external_symlinks {
        let dest = to.join(relative);
        if let Err(err) = crate::fs::symlink_dir_or_file(&target, &dest) {
            return Err(err);
        }
    }
    #[cfg(windows)]
    for (relative, internal) in &internal_junctions {
        let dest = to.join(relative);
        let target = to.join(internal);
        if let Err(err) = junction::create(&target, &dest) {
            return Err(err);
        }
    }
    #[cfg(windows)]
    for (relative, target) in &external_junctions {
        let dest = to.join(relative);
        if let Err(err) = junction::create(&target, &dest) {
            return Err(err);
        }
    }

    Ok(())
}

pub async fn duplicate_instance(backend: Arc<BackendState>, id: InstanceID, name: &str, modal_action: ModalAction) {
    if !crate::fs::is_single_component_path_str(name) {
        modal_action
            .set_finished_with_error(format!("Unable to duplicate instance, name must not be a path: {name}").into());
        return;
    }
    if !sanitize_filename::is_sanitized_with_options(
        name,
        sanitize_filename::OptionsForCheck {
            windows: true,
            ..Default::default()
        },
    ) {
        modal_action.set_finished_with_error(format!("Unable to duplicate instance, name is invalid: {name}").into());
        return;
    }
    if backend.instance_state.read().instances.iter().any(|i| i.name == name) {
        modal_action.set_finished_with_error("Unable to duplicate instance, name is already used".to_string().into());
        return;
    }

    let source = {
        let state = backend.instance_state.read();
        let Some(instance) = state.instances.get(id) else {
            modal_action.set_finished_with_error("Unable to duplicate instance, unknown id".to_string().into());
            return;
        };
        instance.root_path.clone()
    };

    // Persistent sources are cloned only from a hash-proven Ready snapshot. The returned source
    // guard stays alive through the copy, so a cooperating second Pandora process cannot enter
    // reconcile/Publishing while the snapshot is being duplicated. Legacy has no control state;
    // ambiguous/non-Ready persistent state is rejected rather than recovered or guessed here.
    let clone_source = match prepare_profile_clone_source(&source) {
        Ok(source) => source,
        Err(error) => {
            modal_action.set_finished_with_error(format!("Unable to duplicate instance safely: {error}").into());
            return;
        },
    };

    let dest = backend.directories.instances_dir.join(name);

    if let Err(err) = fs::create_dir(&dest) {
        modal_action.set_finished_with_error(format!("Unable to create instance directory: {err}").into());
        return;
    }

    // Mint the destination identity before copying any instance payload. The destination guard
    // remains held until the clone is verified and its reminted Ready manifest is committed.
    let clone_destination = match begin_profile_clone_destination(&dest, &clone_source) {
        Ok(destination) => destination,
        Err(error) => {
            let _ = fs::remove_dir_all(&dest);
            modal_action
                .set_finished_with_error(format!("Unable to initialize duplicated profile identity: {error}").into());
            return;
        },
    };

    let tracker = modal_action.push_tracker("Copying instance files...".into());

    let result = duplicate_with_content_library(
        &source,
        &dest,
        &backend.directories.content_library_dir,
        &|current, total| {
            tracker.set_count(current as usize);
            tracker.set_total(total as usize);
        },
        &|| {
            if modal_action.has_requested_cancel() {
                tracker.set_title("Cancelling...".into());
                Err(Error::new(ErrorKind::Interrupted, "Operation cancelled"))
            } else {
                Ok(())
            }
        },
    );

    let result = match result {
        Ok(()) => clone_destination
            .finish()
            .map(|_| ())
            .map_err(|error| Error::new(ErrorKind::Other, error.to_string())),
        Err(error) => Err(error),
    };

    match result {
        Ok(()) => {
            tracker.set_finished(ProgressTrackerFinishType::Normal);
        },
        Err(error) => {
            let _ = fs::remove_dir_all(&dest);
            if modal_action.has_requested_cancel() {
                tracker.set_finished(ProgressTrackerFinishType::Fast);
            } else {
                tracker.set_finished(ProgressTrackerFinishType::Error);
                modal_action.set_finished_with_error(error.to_string().into());
            }
        },
    }

    modal_action.set_finished();
}

#[cfg(test)]
mod tests {
    use sha2::{Digest as Sha2Digest, Sha256};
    use uuid::Uuid;

    use super::*;
    use crate::{
        profile_layout_flow::{
            DesiredManagedFile, PersistentProfileLayout, ProfileLayoutFlowError, ProfileLayoutState,
        },
        profile_layout_identity::ProfileCloneSource,
    };

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str, minecraft: bool) -> Self {
            let root =
                std::env::temp_dir().join(format!("pandora-duplicate-{label}-{}", Uuid::from_bytes(rand::random())));
            fs::create_dir(&root).unwrap();
            if minecraft {
                fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
            }
            Self(root)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn desired(root: &Path, name: &str, bytes: &[u8]) -> DesiredManagedFile {
        let source = root.join(format!("source-{name}"));
        fs::write(&source, bytes).unwrap();
        DesiredManagedFile::new(
            format!("mods/{name}"),
            source,
            format!("identity-{name}"),
            hex::encode(Sha256::digest(bytes)),
        )
    }

    #[test]
    fn profile_layout_ready_duplicate_remints_namespace_and_cannot_use_original_journal() {
        let source = TestRoot::new("ready-source", true);
        let destination = TestRoot::new("ready-destination", false);
        let content_library = TestRoot::new("content-library", false);

        let mut original_layout = PersistentProfileLayout::open(&source.0).unwrap();
        original_layout.reconcile(&[desired(&source.0, "managed.jar", b"source-v1")]).unwrap();
        let original_uuid = original_layout.profile_uuid();
        drop(original_layout);

        let clone_source = prepare_profile_clone_source(&source.0).unwrap();
        assert!(matches!(clone_source, ProfileCloneSource::Ready { .. }));
        assert_eq!(clone_source.source_uuid(), Some(original_uuid));
        let clone_destination = begin_profile_clone_destination(&destination.0, &clone_source).unwrap();
        let clone_uuid = clone_destination.profile_uuid();
        assert_ne!(clone_uuid, original_uuid);

        duplicate_with_content_library(&source.0, &destination.0, &content_library.0, &|_, _| {}, &|| Ok(())).unwrap();
        clone_destination.finish().unwrap();
        drop(clone_source);

        assert!(!destination.0.join(CONTROL_DIR_NAME).join("journal.json").exists());
        assert!(!destination.0.join(CONTROL_DIR_NAME).join("backup").exists());
        assert!(!destination.0.join(CONTROL_DIR_NAME).join("staging").exists());
        let source_identity = fs::read(source.0.join(CONTROL_DIR_NAME).join("identity.json")).unwrap();
        let clone_identity = fs::read(destination.0.join(CONTROL_DIR_NAME).join("identity.json")).unwrap();
        assert_ne!(source_identity, clone_identity);

        let mut clone_layout = PersistentProfileLayout::open(&destination.0).unwrap();
        assert_eq!(clone_layout.profile_uuid(), clone_uuid);
        assert_eq!(clone_layout.status().state, ProfileLayoutState::Ready);
        clone_layout.reconcile(&[desired(&destination.0, "managed.jar", b"clone-v2")]).unwrap();
        drop(clone_layout);
        assert_eq!(fs::read(source.0.join(".minecraft/mods/managed.jar")).unwrap(), b"source-v1");
        assert_eq!(fs::read(destination.0.join(".minecraft/mods/managed.jar")).unwrap(), b"clone-v2");

        let forged_original_journal = serde_json::json!({
            "schema": 1,
            "profile_uuid": original_uuid,
            "transaction_id": Uuid::from_bytes(rand::random::<[u8; 16]>()),
            "from_generation": 2,
            "target_generation": 3,
            "previous_manifest_sha256": null,
            "target_manifest_sha256": "00",
            "state": "prepared",
            "operations": []
        });
        fs::write(
            destination.0.join(CONTROL_DIR_NAME).join("journal.json"),
            serde_json::to_vec_pretty(&forged_original_journal).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            PersistentProfileLayout::open(&destination.0),
            Err(ProfileLayoutFlowError::IdentityMismatch)
        ));
        assert_eq!(fs::read(source.0.join(".minecraft/mods/managed.jar")).unwrap(), b"source-v1");

        fs::remove_dir_all(&destination.0).unwrap();
        assert!(source.0.join(CONTROL_DIR_NAME).join("identity.json").is_file());
        assert_eq!(fs::read(source.0.join(".minecraft/mods/managed.jar")).unwrap(), b"source-v1");
    }
}
