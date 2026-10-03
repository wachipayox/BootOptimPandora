//! On-demand modpack comparison and guarded text editing for the Overwrites tab.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use bridge::{
    instance::{InstanceID, InstanceStatus},
    profile_overwrites::{
        OverwriteChange, ProfileFile, ProfileOverwrite, ProfileOverwritesReport, ProfileTextFile, RestoreSource,
    },
};
use schema::ignored_profile_paths::IgnoredProfilePaths;
use sha2::{Digest, Sha256};

use crate::{
    BackendState,
    distribution::DistributionClient,
    profile_branch::{
        GlobalRevisionPin, ProfileEntryOrigin, ProfileEntryOwnership, ProfileParentRef, canonical_mod_path,
    },
};

#[derive(Clone)]
struct FileEntry {
    path: PathBuf,
    size: u64,
    disabled: bool,
}

#[derive(Clone)]
struct SignedEntry {
    sha256: String,
    size: Option<u64>,
    disabled: bool,
}

impl BackendState {
    pub(crate) async fn profile_overwrites_report(&self, id: InstanceID) -> Result<ProfileOverwritesReport, String> {
        let (root, minecraft) = {
            let state = self.instance_state.read();
            let instance = state.instances.get(id).ok_or("Instance is no longer available")?;
            (instance.root_path.to_path_buf(), instance.dot_minecraft_path.to_path_buf())
        };
        let manifest = crate::profile_layout_flow::read_committed_manifest(&root)
            .map_err(|error| format!("Unable to read inherited profile information: {error}"))?;
        let ignored = self.config.lock().get().ignored_profile_paths.clone();
        let mut report = ProfileOverwritesReport {
            ignored_paths: ignored.0.clone(),
            files: Vec::new(),
            local_parent: None,
            local_changes: Vec::new(),
            local_error: None,
            global_profile: None,
            global_changes: Vec::new(),
            global_error: None,
            skipped_entries: 0,
        };
        let Some(manifest) = manifest else {
            let (files, skipped) = tokio::task::spawn_blocking(move || scan_tree(&minecraft, &ignored))
                .await
                .map_err(|error| error.to_string())??;
            report.files = file_summaries(&files);
            report.skipped_entries = skipped;
            return Ok(report);
        };
        let branch = manifest.branch;

        let local_parent = match &branch.lineage.parent {
            Some(ProfileParentRef::LocalProfile { profile_uuid }) => {
                match self.find_instance_id_for_profile_uuid(*profile_uuid) {
                    Some(parent_id) => {
                        let state = self.instance_state.read();
                        state
                            .instances
                            .get(parent_id)
                            .map(|parent| (parent.name.to_string(), parent.dot_minecraft_path.to_path_buf()))
                    },
                    None => None,
                }
            },
            _ => None,
        };
        if matches!(branch.lineage.parent, Some(ProfileParentRef::LocalProfile { .. })) {
            if let Some((name, _)) = &local_parent {
                report.local_parent = Some(name.clone());
            } else {
                report.local_parent = Some("Deleted local parent".into());
                report.local_error =
                    Some("The local parent no longer exists; its installed child remains usable.".into());
            }
        }

        let global_pin = branch.applied_revision.or(branch.lineage.global_ancestor);
        let signed = if let Some(pin) = global_pin {
            report.global_profile = Some(format!("{} · {}", pin.profile_id, pin.revision_id));
            match self.signed_overwrite_baseline(&pin, &ignored).await {
                Ok((name, mut entries, rule_paths)) => {
                    for path in rule_paths {
                        let Some(entry) = branch.entries.get(&path) else {
                            continue;
                        };
                        if entry.ownership != ProfileEntryOwnership::Inherited
                            || !matches!(entry.origin, ProfileEntryOrigin::GlobalRevision { .. })
                        {
                            continue;
                        }
                        let Some(managed) = manifest.managed_entries.get(&path) else {
                            continue;
                        };
                        if let Some(signed) = entries.get_mut(&canonical_mod_path(&path)) {
                            signed.sha256 = managed.applied_hash.clone();
                            signed.size = None;
                        }
                    }
                    report.global_profile = Some(format!("{name} · {}", pin.revision_id));
                    Some(entries)
                },
                Err(error) => {
                    report.global_error = Some(format!("Unable to load the pinned global revision: {error}"));
                    None
                },
            }
        } else {
            None
        };

        let local_parent_path = local_parent.map(|(_, path)| path);
        let compared = tokio::task::spawn_blocking(move || {
            let mut hashes = HashMap::new();
            let (child, mut skipped) = scan_tree(&minecraft, &ignored)?;
            let local_changes = if let Some(parent) = local_parent_path {
                let (parent, parent_skipped) = scan_tree(&parent, &ignored)?;
                skipped += parent_skipped;
                compare_local(&child, &parent, &mut hashes)?
            } else {
                Vec::new()
            };
            let global_changes = if let Some(signed) = signed {
                compare_global(&child, &signed, &mut hashes)?
            } else {
                Vec::new()
            };
            Ok::<_, String>((local_changes, global_changes, skipped, file_summaries(&child)))
        })
        .await
        .map_err(|error| format!("Overwrite scan failed: {error}"))??;
        report.local_changes = compared.0;
        report.global_changes = compared.1;
        report.skipped_entries = compared.2;
        report.files = compared.3;
        Ok(report)
    }

    pub(crate) async fn read_profile_text_file(&self, id: InstanceID, path: String) -> Result<ProfileTextFile, String> {
        let minecraft = {
            let state = self.instance_state.read();
            state
                .instances
                .get(id)
                .ok_or("Instance is no longer available")?
                .dot_minecraft_path
                .to_path_buf()
        };
        tokio::task::spawn_blocking(move || read_text(&minecraft, &path))
            .await
            .map_err(|error| error.to_string())?
    }

    pub(crate) async fn save_profile_text_file(
        &self,
        id: InstanceID,
        path: String,
        contents: String,
        expected_sha256: String,
    ) -> Result<ProfileTextFile, String> {
        let (root, minecraft) = {
            let state = self.instance_state.read();
            let instance = state.instances.get(id).ok_or("Instance is no longer available")?;
            if instance.status() != InstanceStatus::NotRunning {
                return Err("Close the game before editing modpack files.".into());
            }
            (instance.root_path.to_path_buf(), instance.dot_minecraft_path.to_path_buf())
        };
        tokio::task::spawn_blocking(move || {
            let _guard = profile_edit_lock(&root)?;
            save_text(&minecraft, &path, &contents, &expected_sha256)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub(crate) async fn toggle_profile_mod(&self, id: InstanceID, path: String) -> Result<String, String> {
        let (root, minecraft) = {
            let state = self.instance_state.read();
            let instance = state.instances.get(id).ok_or("Instance is no longer available")?;
            if instance.status() != InstanceStatus::NotRunning {
                return Err("Close the game before changing mods.".into());
            }
            (instance.root_path.to_path_buf(), instance.dot_minecraft_path.to_path_buf())
        };
        tokio::task::spawn_blocking(move || {
            let _guard = profile_edit_lock(&root)?;
            toggle_mod(&minecraft, &path)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub(crate) async fn restore_profile_file(
        &self,
        id: InstanceID,
        path: String,
        source: RestoreSource,
    ) -> Result<(), String> {
        crate::profile_branch::validate_profile_relative_path(&path).map_err(|error| error.to_string())?;
        let canonical = canonical_mod_path(&path);
        let ignored = self.config.lock().get().ignored_profile_paths.clone();
        if ignored.contains(&canonical) {
            return Err("This path is ignored by profile updates.".into());
        }
        let (root, minecraft) = {
            let state = self.instance_state.read();
            let instance = state.instances.get(id).ok_or("Instance is no longer available")?;
            if instance.status() != InstanceStatus::NotRunning {
                return Err("Close the game before restoring inherited files.".into());
            }
            (instance.root_path.to_path_buf(), instance.dot_minecraft_path.to_path_buf())
        };
        let manifest = crate::profile_layout_flow::read_committed_manifest(&root)
            .map_err(|error| error.to_string())?
            .ok_or("This instance has no inherited profile")?;
        let input = match source {
            RestoreSource::LocalParent => {
                let Some(ProfileParentRef::LocalProfile { profile_uuid }) = manifest.branch.lineage.parent else {
                    return Err("This instance has no direct local parent.".into());
                };
                let parent_id = self
                    .find_instance_id_for_profile_uuid(profile_uuid)
                    .ok_or("The local parent is no longer available")?;
                let state = self.instance_state.read();
                let parent = state.instances.get(parent_id).ok_or("The local parent is no longer available")?;
                RestoreInput::Local(parent.root_path.to_path_buf(), parent.dot_minecraft_path.to_path_buf())
            },
            RestoreSource::PinnedGlobal => {
                let pin = manifest
                    .branch
                    .applied_revision
                    .or(manifest.branch.lineage.global_ancestor)
                    .ok_or("This instance has no pinned global revision")?;
                let config = self.config.lock().get().distribution.clone();
                let client = DistributionClient::new(&config).map_err(|error| error.to_string())?;
                let cache = self.directories.root_launcher_dir.join("distribution-objects");
                let (file, relative) = client
                    .fetch_pinned_file(&pin, &canonical, &cache)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or("This file is absent from the pinned global revision")?;
                RestoreInput::Global(file, relative)
            },
        };
        tokio::task::spawn_blocking(move || {
            let _child_lock = profile_edit_lock(&root)?;
            match input {
                RestoreInput::Local(parent_root, parent_minecraft) => {
                    let _parent_lock = profile_edit_lock(&parent_root)?;
                    let (parent, _) = scan_tree(&parent_minecraft, &ignored)?;
                    let source = parent.get(&canonical).ok_or("The file is absent from the local parent")?;
                    let relative = if source.disabled {
                        format!("{canonical}.disabled")
                    } else {
                        canonical.clone()
                    };
                    install_restored_file(&minecraft, &canonical, &source.path, &relative, &ignored)
                },
                RestoreInput::Global(file, relative) => {
                    install_restored_file(&minecraft, &canonical, &file, &relative, &ignored)
                },
            }
        })
        .await
        .map_err(|error| format!("Restore task failed: {error}"))?
    }

    async fn signed_overwrite_baseline(
        &self,
        pin: &GlobalRevisionPin,
        ignored: &IgnoredProfilePaths,
    ) -> Result<(String, BTreeMap<String, SignedEntry>, BTreeSet<String>), String> {
        let config = self.config.lock().get().distribution.clone();
        let client = DistributionClient::new(&config).map_err(|error| error.to_string())?;
        let resolved = client.resolve_profile_metadata(pin).await.map_err(|error| error.to_string())?;
        let rule_paths = resolved.config_settings.iter().map(|rule| rule.path.clone()).collect();
        let mut files = BTreeMap::new();
        for entry in resolved.entries {
            if ignored.contains(&entry.path) {
                continue;
            }
            let key = canonical_mod_path(&entry.path);
            let signed = SignedEntry {
                sha256: entry.metadata.source_sha256,
                size: Some(entry.metadata.source_size_bytes),
                disabled: key != entry.path,
            };
            if files.insert(key.clone(), signed).is_some() {
                return Err(format!("Global revision contains both enabled and disabled forms of {key}"));
            }
        }
        Ok((resolved.name, files, rule_paths))
    }
}

enum RestoreInput {
    Local(PathBuf, PathBuf),
    Global(PathBuf, String),
}

fn install_restored_file(
    minecraft: &Path,
    canonical: &str,
    source: &Path,
    relative: &str,
    ignored: &IgnoredProfilePaths,
) -> Result<(), String> {
    if ignored.contains(relative) || canonical_mod_path(relative) != canonical {
        return Err("The source path does not match the selected file.".into());
    }
    let _ = checked_file_path(
        source.parent().ok_or("Invalid source")?,
        source.file_name().and_then(|name| name.to_str()).ok_or("Invalid source name")?,
    )?;
    let (child, _) = scan_tree(minecraft, ignored)?;
    let existing = child.get(canonical);
    let target = safe_destination(minecraft, relative)?;
    if fs::symlink_metadata(&target).is_ok() && existing.is_none_or(|entry| entry.path != target) {
        return Err("A different file already exists at the restore destination.".into());
    }
    let source_hash = file_hash(source, &mut HashMap::new())?;
    if let Some(existing) = existing
        && existing.path == target
        && file_hash(&existing.path, &mut HashMap::new())? == source_hash
    {
        return Err("This file already matches its selected parent.".into());
    }
    let directory = target.parent().ok_or("Invalid restore destination")?;
    let temporary = directory.join(format!(".restore-{}.tmp", uuid::Uuid::new_v4()));
    if let Err(error) = fs::copy(source, &temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Unable to stage parent file: {error}"));
    }
    if file_hash(&temporary, &mut HashMap::new())? != source_hash {
        let _ = fs::remove_file(&temporary);
        return Err("Parent file changed while it was being copied; nothing was restored.".into());
    }
    let backup = existing.map(|entry| entry.path.with_extension(format!("restore-{}.bak", uuid::Uuid::new_v4())));
    if let Some(existing) = existing {
        if let Err(error) = fs::rename(&existing.path, backup.as_ref().unwrap()) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Unable to back up the current file: {error}"));
        }
    }
    if let Err(error) = fs::rename(&temporary, &target) {
        if let (Some(existing), Some(backup)) = (existing, &backup) {
            let _ = fs::rename(backup, &existing.path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("Unable to install parent file: {error}"));
    }
    if let Some(backup) = backup {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn safe_destination(root: &Path, relative: &str) -> Result<PathBuf, String> {
    crate::profile_branch::validate_profile_relative_path(relative).map_err(|error| error.to_string())?;
    let mut path = root.to_path_buf();
    let parts: Vec<_> = relative.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err("Restore destination contains a linked or non-directory component.".into());
            },
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(|error| error.to_string())?;
            },
            Err(error) => return Err(error.to_string()),
        }
        #[cfg(windows)]
        if junction::exists(&path).unwrap_or(false) {
            return Err("Restore destination contains a junction.".into());
        }
    }
    path.push(parts.last().ok_or("Invalid restore path")?);
    Ok(path)
}

fn profile_edit_lock(root: &Path) -> Result<Option<crate::profile_layout_identity::ProfileLayoutLockGuard>, String> {
    if !root.join(crate::profile_layout_identity::CONTROL_DIR_NAME).exists() {
        return Ok(None);
    }
    crate::profile_layout_identity::acquire_or_initialize_profile_lock(root)
        .map(Some)
        .map_err(|error| format!("Inherited files are being updated or locked: {error}"))
}

fn file_summaries(files: &BTreeMap<String, FileEntry>) -> Vec<ProfileFile> {
    files
        .iter()
        .map(|(path, entry)| ProfileFile {
            path: if entry.disabled {
                format!("{path}.disabled")
            } else {
                path.clone()
            },
            comparison_path: path.clone(),
            size: entry.size,
            editable: text_extension(path),
        })
        .collect()
}

fn checked_file_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    crate::profile_branch::validate_profile_relative_path(relative).map_err(|error| error.to_string())?;
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        path.push(segment);
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("Linked paths cannot be changed here.".into());
        }
        #[cfg(windows)]
        if junction::exists(&path).unwrap_or(false) {
            return Err("Junction paths cannot be changed here.".into());
        }
    }
    if !path.is_file() {
        return Err("The selected file no longer exists.".into());
    }
    Ok(path)
}

fn toggle_mod(root: &Path, relative: &str) -> Result<String, String> {
    let lower = relative.to_ascii_lowercase();
    if !lower.starts_with("mods/") || !(lower.ends_with(".jar") || lower.ends_with(".jar.disabled")) {
        return Err("Only mod JARs can be activated or deactivated here.".into());
    }
    let source = checked_file_path(root, relative)?;
    let target_relative = if lower.ends_with(".jar.disabled") {
        relative.strip_suffix(".disabled").ok_or("Invalid disabled mod path")?.to_string()
    } else {
        format!("{relative}.disabled")
    };
    crate::profile_branch::validate_profile_relative_path(&target_relative).map_err(|error| error.to_string())?;
    let target = root.join(target_relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    if target.exists() {
        return Err("The other enabled/disabled variant already exists.".into());
    }
    fs::rename(&source, &target).map_err(|error| format!("Unable to change mod state: {error}"))?;
    Ok(target_relative)
}

fn text_extension(path: &str) -> bool {
    matches!(
        path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),
        "toml" | "properties" | "txt" | "cfg" | "ini" | "json" | "mcmeta" | "yaml" | "yml"
    )
}

fn checked_text_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if !text_extension(relative) {
        return Err("Only supported text files can be edited here.".into());
    }
    checked_file_path(root, relative)
}

fn read_text(root: &Path, relative: &str) -> Result<ProfileTextFile, String> {
    let path = checked_text_path(root, relative)?;
    if fs::metadata(&path).map_err(|error| error.to_string())?.len() > 1024 * 1024 {
        return Err("Text preview is limited to 1 MiB per file.".into());
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Text preview is limited to 1 MiB per file.".into());
    }
    let contents = String::from_utf8(bytes.clone()).map_err(|_| "The file is not UTF-8 text.".to_string())?;
    if contents.contains('\0') {
        return Err("Binary files cannot be edited here.".into());
    }
    Ok(ProfileTextFile {
        path: relative.into(),
        contents,
        sha256: hex::encode(Sha256::digest(&bytes)),
    })
}

fn save_text(root: &Path, relative: &str, contents: &str, expected_sha256: &str) -> Result<ProfileTextFile, String> {
    if contents.len() > 1024 * 1024 {
        return Err("Text files are limited to 1 MiB here.".into());
    }
    let current = read_text(root, relative)?;
    if current.sha256 != expected_sha256 {
        return Err("The file changed since it was opened. Refresh it before saving.".into());
    }
    let path = checked_text_path(root, relative)?;
    let parent = path.parent().ok_or("Invalid text file path")?;
    let temporary = parent.join(format!(".overwrites-{}.tmp", uuid::Uuid::new_v4()));
    let backup = parent.join(format!(".overwrites-{}.bak", uuid::Uuid::new_v4()));
    fs::write(&temporary, contents).map_err(|error| error.to_string())?;
    if let Err(error) = fs::rename(&path, &backup) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not back up the original file: {error}"));
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::rename(&backup, &path);
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not save the edited file: {error}"));
    }
    let _ = fs::remove_file(&backup);
    read_text(root, relative)
}

fn scan_tree(root: &Path, ignored: &IgnoredProfilePaths) -> Result<(BTreeMap<String, FileEntry>, usize), String> {
    if !root.exists() {
        return Ok((BTreeMap::new(), 0));
    }
    if !root.is_dir() {
        return Err(format!("Modpack directory is not a folder: {}", root.display()));
    }
    let mut pending = vec![root.to_path_buf()];
    let mut files = BTreeMap::new();
    let mut skipped = 0;
    while let Some(directory) = pending.pop() {
        for entry in
            fs::read_dir(&directory).map_err(|error| format!("Unable to read {}: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let relative_for_filter = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if ignored.contains(&relative_for_filter) {
                continue;
            }
            if directory == root
                && matches!(
                    entry.file_name().to_string_lossy().to_ascii_lowercase().as_str(),
                    "saves"
                        | "logs"
                        | "crash-reports"
                        | "screenshots"
                        | "session.lock"
                        | "usercache.json"
                        | "usernamecache.json"
                        | "realms_persistence.json"
                        | ".mixin.out"
                        | "unilog"
                        | "assets"
                        | "libraries"
                        | "versions"
                        | "natives"
                        | "runtime"
                        | "cache"
                        | "caches"
                        | "downloads"
                        | "backups"
                )
            {
                continue;
            }
            #[cfg(windows)]
            if junction::exists(&path).unwrap_or(false) {
                skipped += 1;
                continue;
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() {
                skipped += 1;
                continue;
            }
            if kind.is_dir() {
                pending.push(path);
                continue;
            }
            if !kind.is_file() {
                skipped += 1;
                continue;
            }
            let Some(relative) = path.strip_prefix(root).map_err(|error| error.to_string())?.to_str() else {
                skipped += 1;
                continue;
            };
            let relative = relative.replace('\\', "/");
            if crate::profile_branch::validate_profile_relative_path(&relative).is_err() {
                skipped += 1;
                continue;
            }
            let canonical = canonical_mod_path(&relative);
            let metadata = entry.metadata().map_err(|error| format!("Unable to inspect {relative}: {error}"))?;
            if files
                .insert(
                    canonical.clone(),
                    FileEntry {
                        path,
                        size: metadata.len(),
                        disabled: canonical != relative,
                    },
                )
                .is_some()
            {
                return Err(format!("Both enabled and disabled variants exist for {canonical}"));
            }
        }
    }
    Ok((files, skipped))
}

fn compare_local(
    child: &BTreeMap<String, FileEntry>,
    parent: &BTreeMap<String, FileEntry>,
    hashes: &mut HashMap<PathBuf, String>,
) -> Result<Vec<ProfileOverwrite>, String> {
    let mut changes = Vec::new();
    for path in child.keys().chain(parent.keys()).collect::<BTreeSet<_>>() {
        let change = match (child.get(path), parent.get(path)) {
            (Some(current), None) => Some(if current.disabled {
                OverwriteChange::AddedDisabled
            } else {
                OverwriteChange::Added
            }),
            (None, Some(_)) => Some(OverwriteChange::Removed),
            (Some(current), Some(base)) => {
                let modified =
                    current.size != base.size || file_hash(&current.path, hashes)? != file_hash(&base.path, hashes)?;
                classify_change(modified, base.disabled, current.disabled)
            },
            (None, None) => None,
        };
        if let Some(change) = change {
            changes.push(ProfileOverwrite {
                path: (*path).clone(),
                change,
            });
        }
    }
    Ok(changes)
}

fn compare_global(
    child: &BTreeMap<String, FileEntry>,
    signed: &BTreeMap<String, SignedEntry>,
    hashes: &mut HashMap<PathBuf, String>,
) -> Result<Vec<ProfileOverwrite>, String> {
    let mut changes = Vec::new();
    for path in child.keys().chain(signed.keys()).collect::<BTreeSet<_>>() {
        let change = match (child.get(path), signed.get(path)) {
            (Some(current), None) => Some(if current.disabled {
                OverwriteChange::AddedDisabled
            } else {
                OverwriteChange::Added
            }),
            (None, Some(_)) => Some(OverwriteChange::Removed),
            (Some(current), Some(base)) => {
                let modified = base.size.is_some_and(|size| current.size != size)
                    || file_hash(&current.path, hashes)? != base.sha256;
                classify_change(modified, base.disabled, current.disabled)
            },
            (None, None) => None,
        };
        if let Some(change) = change {
            changes.push(ProfileOverwrite {
                path: (*path).clone(),
                change,
            });
        }
    }
    Ok(changes)
}

fn classify_change(modified: bool, base_disabled: bool, current_disabled: bool) -> Option<OverwriteChange> {
    match (modified, base_disabled, current_disabled) {
        (false, before, after) if before == after => None,
        (false, false, true) => Some(OverwriteChange::Disabled),
        (false, true, false) => Some(OverwriteChange::Enabled),
        (true, before, after) if before == after => Some(OverwriteChange::Modified),
        (true, false, true) => Some(OverwriteChange::ModifiedAndDisabled),
        (true, true, false) => Some(OverwriteChange::ModifiedAndEnabled),
        _ => None,
    }
}

fn file_hash(path: &Path, hashes: &mut HashMap<PathBuf, String>) -> Result<String, String> {
    if let Some(digest) = hashes.get(path) {
        return Ok(digest.clone());
    }
    let mut file = fs::File::open(path).map_err(|error| format!("Unable to compare {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Unable to compare {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hex::encode(hasher.finalize());
    hashes.insert(path.to_path_buf(), digest.clone());
    Ok(digest)
}
