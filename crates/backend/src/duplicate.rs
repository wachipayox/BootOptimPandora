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
    save_groups_dir: &Path,
    preserve_external_saves_link: bool,
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
    directories_to_visit.push((from.to_path_buf(), 0, PathBuf::new()));

    while let Some((directory, depth, relative_directory)) = directories_to_visit.pop() {
        check_cancel()?;
        let read_dir = fs::read_dir(directory)?;
        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            let relative = relative_directory.join(entry.file_name());
            // Persistent profile control state is identity/transaction state, not instance
            // payload. The destination namespace is created separately with a fresh UUID.
            if relative == Path::new(CONTROL_DIR_NAME) {
                continue;
            }
            #[cfg(windows)]
            if let Ok(target) = junction::get_target(&path) {
                if relative == Path::new(".minecraft/saves")
                    && !preserve_external_saves_link
                    && is_managed_group_saves(&target, save_groups_dir)
                {
                    directories.push(relative.to_path_buf());
                    directories_to_visit.push((target, depth + 1, relative.to_path_buf()));
                    continue;
                }
                if let Ok(internal) = target.strip_prefix(&from) {
                    internal_junctions.push((relative.to_path_buf(), internal.to_path_buf()));
                } else {
                    external_junctions.push((relative.to_path_buf(), target));
                }
                continue;
            }
            if file_type.is_symlink() {
                let target = fs::read_link(&path)?;
                if relative == Path::new(".minecraft/saves")
                    && !preserve_external_saves_link
                    && is_managed_group_saves(&path, save_groups_dir)
                {
                    directories.push(relative.to_path_buf());
                    directories_to_visit.push((path.canonicalize()?, depth + 1, relative.to_path_buf()));
                    continue;
                }
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
                directories_to_visit.push((path, depth + 1, relative.to_path_buf()));
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

fn is_managed_group_saves(link: &Path, save_groups_dir: &Path) -> bool {
    let Ok(target) = fs::canonicalize(link) else {
        return false;
    };
    let Ok(groups_root) = fs::canonicalize(save_groups_dir) else {
        return false;
    };
    target.file_name().is_some_and(|name| name == "saves") && target.starts_with(groups_root)
}

pub async fn duplicate_instance(backend: Arc<BackendState>, id: InstanceID, name: &str, modal_action: ModalAction) {
    duplicate_instance_inner(backend, id, name, modal_action, false, true, 0).await;
}

pub async fn create_local_branch(backend: Arc<BackendState>, id: InstanceID, name: &str, create_save_group: bool, reuse_parent_icon: bool, icon_hue_degrees: i32, modal_action: ModalAction) {
    // Validate before moving any worlds; failed/cancelled creation restores a newly made group.
    if !crate::fs::is_single_component_path_str(name)
        || !sanitize_filename::is_sanitized_with_options(name, sanitize_filename::OptionsForCheck { windows: true, ..Default::default() })
        || backend.instance_state.read().instances.iter().any(|instance| instance.name == name)
    {
        modal_action.set_finished_with_error("Choose a valid, unused instance name".into());
        return;
    }
    let mut created_group = false;
    if create_save_group {
        match backend.list_save_groups(id) {
            Ok(groups) if groups.iter().any(|group| group.selected) => {},
            Ok(_) => match backend.create_save_group(id, format!("Worlds of {name}")) {
                Ok(()) => created_group = true,
                Err(error) => { modal_action.set_finished_with_error(error.into()); return; }
            },
            Err(error) => { modal_action.set_finished_with_error(error.into()); return; }
        }
    }
    duplicate_instance_inner(backend.clone(), id, name, modal_action.clone(), true, reuse_parent_icon, icon_hue_degrees).await;
    if created_group && (modal_action.get_error_message().is_some() || modal_action.has_requested_cancel()) {
        if let Err(error) = backend.leave_save_group(id) {
            backend.send.send_error(format!("Branch creation stopped; worlds remain in their save group: {error}"));
        }
    }
}

async fn duplicate_instance_inner(
    backend: Arc<BackendState>,
    id: InstanceID,
    name: &str,
    modal_action: ModalAction,
    as_branch: bool,
    reuse_parent_icon: bool,
    icon_hue_degrees: i32,
) {
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

    let parent_branch = if as_branch {
        let depth = match backend.persistent_profile_branch_depth(id) {
            Ok(depth) => depth,
            Err(error) => {
                modal_action.set_finished_with_error(format!("Unable to inspect branch ancestry: {error}").into());
                return;
            },
        };
        if depth >= crate::profile_branch::MAX_PROFILE_BRANCH_DEPTH {
            modal_action.set_finished_with_error("A profile cannot inherit more than 8 levels".to_string().into());
            return;
        }
        match backend.ensure_persistent_profile_branch(id) {
            Ok(snapshot) => Some((snapshot.profile_uuid, snapshot.branch)),
            Err(error) => {
                modal_action.set_finished_with_error(format!("Unable to prepare parent profile: {error}").into());
                return;
            },
        }
    } else {
        None
    };

    let source = {
        let state = backend.instance_state.read();
        let Some(instance) = state.instances.get(id) else {
            modal_action.set_finished_with_error("Unable to duplicate instance, unknown id".to_string().into());
            return;
        };
        instance.root_path.clone()
    };

    // Persistent sources must have a committed Ready identity and no pending transaction. The
    // source guard stays alive through copying; ordinary live edits are captured into the clone
    // snapshot rather than treated as transaction corruption. Legacy sources have no control
    // state; ambiguous/non-Ready persistent state is rejected rather than guessed here.
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
    let clone_destination = match begin_profile_clone_destination(&dest, &clone_source, as_branch) {
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
        &backend.directories.save_groups_dir,
        as_branch,
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
        Ok(()) => {
            let icon_result = if as_branch { apply_branch_icon(&dest, reuse_parent_icon, icon_hue_degrees) } else { Ok(()) };
            let normalized = icon_result.and_then(|()| if as_branch {
                normalize_disabled_mods_for_branch(&dest)
            } else {
                Ok(())
            });
            normalized.and_then(|()| {
                clone_destination
                    .finish()
                    .map(|_| ())
                    .map_err(|error| Error::new(ErrorKind::Other, error.to_string()))
            })
        },
        Err(error) => Err(error),
    };

    let mut clone_succeeded = false;
    match result {
        Ok(()) => {
            tracker.set_finished(ProgressTrackerFinishType::Normal);
            clone_succeeded = true;
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

    if clone_succeeded && let Some((parent_uuid, parent_branch)) = parent_branch {
        let parent_root = {
            let state = backend.instance_state.read();
            state
                .instances
                .get(id)
                .map(|instance| instance.dot_minecraft_path.to_path_buf())
                .ok_or_else(|| "The parent instance is no longer available".to_owned())
        };
        let setup_result = match parent_root {
            Ok(parent_root) => {
                let child_root = dest.clone();
                // This is a one-time full-tree snapshot (reflinked where possible), not per-start
                // sandbox copying. The lineage update rebases cloned ownership as inherited while
                // preserving the new UUID and the already-copied bytes.
                match tokio::task::spawn_blocking(move || {
                    let mut child_layout = crate::profile_layout_flow::PersistentProfileLayout::open(&child_root)
                        .map_err(|error| error.to_string())?;
                    let child_branch = child_layout.branch_manifest().map_err(|error| error.to_string())?;
                    let parent_entries = crate::local_profile::snapshot_parent_tree(
                        &parent_root,
                        parent_uuid,
                        &parent_branch,
                        Some(&child_branch.entries),
                    )?;
                    let delta = crate::local_profile::local_parent_delta(
                        &child_branch,
                        parent_uuid,
                        &parent_branch.lineage,
                        parent_branch.applied_revision.clone(),
                        &parent_branch.config_settings,
                        parent_entries,
                    )?;
                    match child_layout.initialize_local_child_branch(&delta).map_err(|error| error.to_string())? {
                        crate::profile_layout_flow::ReconcileOutcome::Ready { .. } => Ok(()),
                        crate::profile_layout_flow::ReconcileOutcome::NeedsReconcile { conflicts, .. } => {
                            Err(format!("Child profile reconciliation needs attention: {}", conflicts.join(", ")))
                        },
                    }
                })
                .await
                {
                    Ok(result) => result,
                    Err(error) => Err(format!("Local branch initialization task failed: {error}")),
                }
            },
            Err(error) => Err(error),
        };
        if let Err(error) = setup_result {
            let _ = fs::remove_dir_all(&dest);
            modal_action.set_finished_with_error(format!("Instance copied, but branch setup failed: {error}").into());
        } else {
            if backend.load_instance_from_path(&dest, false, false) {
                let child_id = backend.instance_state.read().instances.iter().find(|i| i.root_path.as_ref() == dest.as_path()).map(|i| i.id);
                if let Some(child_id) = child_id {
                    match backend.persistent_profile_branch_status(id, None).map_err(|e| e.to_string()).and_then(|parent| backend.record_parent_version(child_id, &parent)) {
                        Ok(()) => {},
                        Err(error) => backend.send.send_error(format!("Branch created; parent version could not be recorded: {error}")),
                    }
                }
                backend.send.send_success(format!("Local branch '{name}' is ready"));
            } else {
                let _ = fs::remove_dir_all(&dest);
                modal_action.set_finished_with_error("The local branch was created but could not be loaded".into());
            }
        }
    }

    modal_action.set_finished();
}

fn apply_branch_icon(instance_root: &Path, reuse: bool, hue_degrees: i32) -> Result<()> {
    let icon_path = instance_root.join("icon.png");
    if !reuse {
        match fs::remove_file(&icon_path) {
            Ok(()) => {},
            Err(error) if error.kind() == ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
        return Ok(());
    }
    let hue_degrees = hue_degrees.clamp(-180, 180);
    if hue_degrees == 0 || !icon_path.is_file() { return Ok(()); }
    let image = image::open(&icon_path).map_err(|error| Error::new(ErrorKind::InvalidData, format!("cannot read parent icon: {error}")))?;
    let rotated = image.huerotate(hue_degrees);
    let mut output = std::io::BufWriter::new(fs::File::create(&icon_path)?);
    rotated.write_to(&mut output, image::ImageFormat::Png)
        .map_err(|error| Error::new(ErrorKind::Other, format!("cannot recolor branch icon: {error}")))?;
    output.flush()
}

/// A child branch starts with its own enabled mods, regardless of the parent's local toggle state.
fn normalize_disabled_mods_for_branch(instance_root: &Path) -> Result<()> {
    let mods = instance_root.join(".minecraft").join("mods");
    match fs::symlink_metadata(&mods) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(Error::new(ErrorKind::InvalidData, "mods path is not a plain directory"));
        },
        Ok(_) => {},
    }
    #[cfg(windows)]
    if junction::exists(&mods).unwrap_or(false) {
        return Err(Error::new(ErrorKind::InvalidData, "mods path is a junction"));
    }
    let entries = match fs::read_dir(&mods) {
        Ok(entries) => entries,
        Err(error) => return Err(error),
    };

    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.file_name().and_then(|filename| filename.to_str()).is_none() {
            continue;
        }
        let relative = path
            .strip_prefix(instance_root.join(".minecraft"))
            .map_err(|error| Error::new(ErrorKind::InvalidInput, error.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        let canonical = crate::profile_branch::canonical_mod_path(&relative);
        if canonical == relative {
            continue;
        }
        let Some(enabled_filename) = canonical.rsplit('/').next() else {
            continue;
        };
        let enabled_path = path.with_file_name(enabled_filename);
        match fs::symlink_metadata(&enabled_path) {
            Ok(_) => {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("cannot create branch: both enabled and disabled copies exist for {enabled_filename}"),
                ));
            },
            Err(error) if error.kind() == ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
        fs::rename(path, enabled_path)?;
    }
    Ok(())
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
    fn derived_branch_icon_reuse_hue_rotation_and_opt_out_are_child_only() {
        let parent = TestRoot::new("icon-parent", false);
        let child = TestRoot::new("icon-child", false);
        let mut image = image::RgbaImage::new(2, 1);
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        image.put_pixel(1, 0, image::Rgba([25, 80, 140, 72]));
        image.save(parent.0.join("icon.png")).unwrap();
        fs::copy(parent.0.join("icon.png"), child.0.join("icon.png")).unwrap();
        let original = fs::read(parent.0.join("icon.png")).unwrap();
        apply_branch_icon(&child.0, true, 0).unwrap();
        assert_eq!(fs::read(parent.0.join("icon.png")).unwrap(), original);
        apply_branch_icon(&child.0, true, 120).unwrap();
        assert_eq!(fs::read(parent.0.join("icon.png")).unwrap(), original);
        let recolored = image::open(child.0.join("icon.png")).unwrap().to_rgba8();
        assert_ne!(recolored.get_pixel(0, 0), image.get_pixel(0, 0));
        assert_eq!(recolored.get_pixel(1, 0)[3], 72);
        apply_branch_icon(&child.0, false, 0).unwrap();
        assert!(!child.0.join("icon.png").exists());
        assert_eq!(fs::read(parent.0.join("icon.png")).unwrap(), original);
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
        let clone_destination = begin_profile_clone_destination(&destination.0, &clone_source, false).unwrap();
        let clone_uuid = clone_destination.profile_uuid();
        assert_ne!(clone_uuid, original_uuid);

        duplicate_with_content_library(
            &source.0,
            &destination.0,
            &content_library.0,
            &content_library.0,
            false,
            &|_, _| {},
            &|| Ok(()),
        )
        .unwrap();
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

    #[test]
    fn child_branch_starts_parent_disabled_mod_enabled_and_refuses_collisions() {
        let root = TestRoot::new("disabled-mod", true);
        let mods = root.0.join(".minecraft/mods");
        fs::write(mods.join("child.jar.disabled"), b"mod").unwrap();

        normalize_disabled_mods_for_branch(&root.0).unwrap();
        assert!(mods.join("child.jar").is_file());
        assert!(!mods.join("child.jar.disabled").exists());

        fs::write(mods.join("collision.jar"), b"enabled").unwrap();
        fs::write(mods.join("collision.jar.disabled"), b"disabled").unwrap();
        assert!(normalize_disabled_mods_for_branch(&root.0).is_err());
        assert!(mods.join("collision.jar").is_file());
        assert!(mods.join("collision.jar.disabled").is_file());
    }
}
