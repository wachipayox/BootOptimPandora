//! Local-to-local profile branch snapshot and update operations.
//!
//! A branch starts from a copy-on-write optimized instance clone, then records the parent's
//! effective `.minecraft` tree. Explicit updates enumerate the parent and use size/mtime as a
//! cheap unchanged-file filter; only changed/new files are hashed. Start checks parent
//! file metadata, but never reads their contents or scans the child's tree.

use std::{collections::BTreeMap, fs, io::Read, path::{Path, PathBuf}, time::UNIX_EPOCH};

use bridge::instance::InstanceID;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    BackendState,
    distribution::ManifestConfigSetting,
    profile_branch::{
        EffectiveProfileEntry, GlobalRevisionPin, ProfileBranchManifest, ProfileDeltaChange, ProfileEntryMetadata,
        ProfileEntryOrigin, ProfileEntryOwnership, ProfileFilePolicy, ProfileLineage, ProfileRevisionDelta,
    },
};
use bridge::modal_action::{ModalAction, ProgressTrackerFinishType};

pub(crate) fn snapshot_parent_tree(
    root: &Path,
    parent_uuid: Uuid,
    parent_branch: &ProfileBranchManifest,
    previous_child_entries: Option<&BTreeMap<String, ProfileEntryMetadata>>,
) -> Result<Vec<EffectiveProfileEntry>, String> {
    if !root.is_dir() {
        return Err("The local parent has no .minecraft directory".into());
    }

    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| format!("Unable to read {directory:?}: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("Unable to read a parent file entry: {error}"))?;
            let path = entry.path();
            // Worlds are personal mutable data, not part of a modpack profile snapshot.
            // In particular, never traverse a save-group junction from a branched instance.
            if directory == root && entry.file_name().to_string_lossy().eq_ignore_ascii_case("saves") {
                continue;
            }
            let file_type = entry.file_type().map_err(|error| format!("Unable to inspect {path:?}: {error}"))?;
            #[cfg(windows)]
            if junction::exists(&path).unwrap_or(false) {
                return Err(format!("A parent entry is a junction and cannot be inherited safely: {path:?}"));
            }
            if file_type.is_symlink() {
                return Err(format!("A parent entry is a symbolic link and cannot be inherited safely: {path:?}"));
            }
            if file_type.is_dir() {
                pending.push(path);
                continue;
            }
            if !file_type.is_file() {
                return Err(format!("A parent entry is not a regular file: {path:?}"));
            }

            let relative = path
                .strip_prefix(root)
                .map_err(|_| format!("Parent path escaped its .minecraft directory: {path:?}"))?;
            let relative = relative
                .components()
                .map(|component| {
                    component
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| "A parent path is not valid UTF-8".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            let relative = crate::profile_branch::canonical_mod_path(&relative);
            crate::profile_branch::validate_profile_relative_path(&relative).map_err(|error| error.to_string())?;

            let file_metadata = fs::metadata(&path).map_err(|error| format!("Unable to inspect {path:?}: {error}"))?;
            let modified = file_metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_nanos());
            let disabled_alias = crate::profile_branch::disabled_mod_alias(&relative);
            let prior_child = previous_child_entries.and_then(|entries| {
                entries
                    .get(&relative)
                    .or_else(|| disabled_alias.as_ref().and_then(|alias| entries.get(alias)))
            });
            let prior_parent = parent_branch
                .entries
                .get(&relative)
                .or_else(|| disabled_alias.as_ref().and_then(|alias| parent_branch.entries.get(alias)));
            let config_from_global = prior_parent.is_some_and(|metadata| {
                metadata.logical_identity.starts_with("config:")
                    && matches!(&metadata.origin, ProfileEntryOrigin::GlobalRevision { .. })
            });
            let source_sha256 = if config_from_global {
                // A globally seeded config may have been merged into its parent instance, so the
                // parent manifest's signed source hash is not necessarily the live file hash.
                sha256_file(&path)?
            } else if let Some(prior) = prior_child
                && prior.source_size_bytes == file_metadata.len()
                && prior.source_modified_unix_nanos == modified
                && prior.source_size_bytes > 0
            {
                prior.source_sha256.clone()
            } else {
                sha256_file(&path)?
            };

            let logical_identity = prior_parent
                .map(|metadata| metadata.logical_identity.clone())
                .unwrap_or_else(|| format!("local:{parent_uuid}:{relative}"));
            let policy = prior_parent.map_or(ProfileFilePolicy::Enforced, |metadata| metadata.policy);
            let origin = if config_from_global {
                prior_parent.unwrap().origin.clone()
            } else {
                ProfileEntryOrigin::LocalProfile {
                    profile_uuid: parent_uuid,
                }
            };
            let metadata = ProfileEntryMetadata {
                logical_identity,
                source_sha256,
                source_size_bytes: file_metadata.len(),
                source_modified_unix_nanos: modified,
                origin,
                ownership: ProfileEntryOwnership::Inherited,
                // Each fork owns a stable snapshot. Future user edits in the child are detected
                // per changed path and become local overrides before parent data is reconciled.
                policy,
            };
            if result
                .insert(
                    relative.clone(),
                    EffectiveProfileEntry {
                        path: relative.clone(),
                        source: path,
                        metadata,
                    },
                )
                .is_some()
            {
                return Err(format!("The parent has both enabled and disabled files for the same mod: {relative}"));
            }
        }
    }
    Ok(result.into_values().collect())
}

pub(crate) fn local_parent_delta(
    child_branch: &ProfileBranchManifest,
    parent_uuid: Uuid,
    parent_lineage: &ProfileLineage,
    parent_applied_revision: Option<GlobalRevisionPin>,
    parent_config_settings: &[ManifestConfigSetting],
    parent_entries: Vec<EffectiveProfileEntry>,
) -> Result<ProfileRevisionDelta, String> {
    let lineage = ProfileLineage::from_local(parent_uuid, parent_lineage.global_ancestor.clone())
        .map_err(|error| error.to_string())?;
    let target = parent_entries
        .iter()
        .map(|entry| crate::profile_branch::canonical_mod_path(&entry.path))
        .collect::<std::collections::BTreeSet<_>>();
    let mut changes = Vec::new();

    for entry in parent_entries {
        let canonical_path = crate::profile_branch::canonical_mod_path(&entry.path);
        let child_entry = child_branch.entries.get(&canonical_path).or_else(|| {
            crate::profile_branch::disabled_mod_alias(&canonical_path)
                .as_ref()
                .and_then(|alias| child_branch.entries.get(alias))
        });
        let unchanged = child_entry.is_some_and(|current| {
            current.logical_identity == entry.metadata.logical_identity
                && current.source_sha256 == entry.metadata.source_sha256
                && current.source_size_bytes == entry.metadata.source_size_bytes
                && current.source_modified_unix_nanos == entry.metadata.source_modified_unix_nanos
                && current.policy == entry.metadata.policy
                && current.origin == entry.metadata.origin
        });
        if !unchanged {
            changes.push(ProfileDeltaChange::Upsert(entry));
        }
    }
    for (path, metadata) in &child_branch.entries {
        let canonical_path = crate::profile_branch::canonical_mod_path(path);
        if metadata.ownership == ProfileEntryOwnership::Inherited && !target.contains(&canonical_path) {
            changes.push(ProfileDeltaChange::Remove {
                path: path.clone(),
                origin: metadata.origin.clone(),
                ownership: ProfileEntryOwnership::Inherited,
                policy: metadata.policy,
            });
        }
    }

    Ok(ProfileRevisionDelta {
        lineage,
        target_revision: parent_applied_revision,
        config_settings: parent_config_settings.to_vec(),
        changes,
    })
}

// Only metadata is read here: no enumeration/hash of .minecraft on Start.
impl BackendState {
    pub(crate) fn parent_version_path(&self, id: InstanceID) -> Result<std::path::PathBuf, String> {
        let state = self.instance_state.read();
        let instance = state.instances.get(id).ok_or("Instance is no longer available")?;
        Ok(instance.root_path.join(crate::profile_layout_identity::CONTROL_DIR_NAME).join("local-parent-version.json"))
    }
    pub(crate) fn record_parent_version(&self, id: InstanceID, parent: &crate::ProfileBranchSnapshot) -> Result<(), String> {
        let bytes = serde_json::to_vec(&(parent.profile_uuid, parent.generation)).map_err(|e| e.to_string())?;
        let path = self.parent_version_path(id)?;
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        fs::rename(temporary, path).map_err(|e| e.to_string())?;
        if let Some(parent_id) = self.find_instance_id_for_profile_uuid(parent.profile_uuid) {
            let root = self.instance_state.read().instances.get(parent_id)
                .ok_or("Local parent disappeared")?.dot_minecraft_path.to_path_buf();
            let fingerprint = parent_tree_fingerprint(&root)?;
            let path = self.parent_fingerprint_path(id)?;
            fs::write(&path, fingerprint).map_err(|e| format!("Unable to record parent file state: {e}"))?;
        }
        Ok(())
    }
    fn parent_fingerprint_path(&self, id: InstanceID) -> Result<PathBuf, String> {
        Ok(self.parent_version_path(id)?.with_file_name("local-parent-tree.sha256"))
    }
    fn inherited_local_chain(&self, id: InstanceID) -> Result<Vec<(InstanceID, crate::ProfileBranchSnapshot)>, String> {
        let mut current = id;
        let mut seen = std::collections::HashSet::new();
        let mut chain = Vec::new();
        loop {
            let snapshot = self.persistent_profile_branch_status(current, None).map_err(|e| e.to_string())?;
            if !seen.insert(snapshot.profile_uuid) || chain.len() > crate::profile_branch::MAX_PROFILE_BRANCH_DEPTH {
                return Err("Invalid profile ancestry".into());
            }
            let parent = snapshot.branch.lineage.parent.clone();
            chain.push((current, snapshot));
            match parent {
                Some(crate::profile_branch::ProfileParentRef::LocalProfile { profile_uuid }) => {
                    // The installed child remains usable after deleting its parent.
                    // Keep checking descendants below that missing ancestor.
                    let Some(parent_id) = self.find_instance_id_for_profile_uuid(profile_uuid) else {
                        return Ok(chain);
                    };
                    current = parent_id;
                },
                _ => return Ok(chain),
            }
        }
    }
    pub async fn check_inherited_updates(&self, id: InstanceID) -> Result<Vec<String>, String> {
        let chain = self.inherited_local_chain(id)?;
        let mut updates = Vec::new();
        for pair in chain.windows(2) {
            let (child_id, _) = &pair[0];
            let (_, parent) = &pair[1];
            let stored = fs::read(self.parent_version_path(*child_id)?).ok()
                .and_then(|bytes| serde_json::from_slice::<(Uuid, Option<u64>)>(&bytes).ok());
            // Old branches get a single explicit synchronization, not an expensive scan here.
            if stored != Some((parent.profile_uuid, parent.generation)) {
                updates.push("Local parent revision".into());
            } else {
                let parent_id = pair[1].0;
                let root = self.instance_state.read().instances.get(parent_id)
                    .ok_or("Local parent disappeared")?.dot_minecraft_path.to_path_buf();
                let stored_fingerprint = fs::read_to_string(self.parent_fingerprint_path(*child_id)?).ok();
                let current = tokio::task::spawn_blocking(move || parent_tree_fingerprint(&root))
                    .await.map_err(|error| format!("Parent file scan failed: {error}"))??;
                if stored_fingerprint.as_deref() != Some(current.as_str()) {
                    updates.push("Local parent files".into());
                }
            }
        }
        if let Some((_, root)) = chain.last()
            && let Some(crate::profile_branch::ProfileParentRef::GlobalRevision { pin }) = &root.branch.lineage.parent
        {
            match tokio::time::timeout(std::time::Duration::from_secs(2), self.available_global_update(pin)).await {
                Ok(Ok(Some(name))) => updates.push(name),
                Ok(Ok(None)) => {},
                Ok(Err(error)) => log::warn!("Global update check skipped: {error}"),
                Err(_) => log::warn!("Global update check timed out"),
            }
        }
        Ok(updates)
    }
    async fn available_global_update(&self, pin: &crate::profile_branch::GlobalRevisionPin) -> Result<Option<String>, String> {
        let config = self.config.lock().get().distribution.clone();
        let client = crate::distribution::DistributionClient::new(&config).map_err(|e| e.to_string())?;
        let profiles = client.list_profiles().await.map_err(|e| e.to_string())?;
        let Some(profile) = profiles.iter().find(|profile| profile.profile_id == pin.profile_id) else {
            // Catalog withdrawal does not invalidate an already installed revision.
            return Ok(None);
        };
        let target = crate::distribution::selected_revision(profile);
        Ok((pin.revision_id != target.revision_id || pin.manifest_sha256 != target.manifest_sha256)
            .then(|| profile.name.clone()))
    }
    pub async fn update_inherited_chain(&self, id: InstanceID, modal_action: &ModalAction) -> Result<(), String> {
        let chain = self.inherited_local_chain(id)?;
        let overall = modal_action.push_tracker("Updating inherited files".into());
        overall.set_total(chain.len().max(1));
        // Update ancestors first, then carry each resulting snapshot down to its children.
        for (instance, snapshot) in chain.into_iter().rev() {
            if modal_action.has_requested_cancel() {
                overall.set_finished(ProgressTrackerFinishType::Error);
                return Err("Inherited update cancelled".into());
            }
            let step = modal_action.push_sub_tracker("Applying parent changes".into());
            let result: Result<(), String> = match snapshot.branch.lineage.parent {
                Some(crate::profile_branch::ProfileParentRef::GlobalRevision { pin }) => {
                    // Only update a published ancestor with a newer revision. Missing
                    // catalog entries must not block surviving local descendants.
                    match tokio::time::timeout(std::time::Duration::from_secs(2), self.available_global_update(&pin)).await {
                        Ok(Ok(Some(_))) => {
                            let assets = modal_action.push_sub_tracker("Downloading modpack assets".into());
                            let result = self.update_global_profile_instance(instance, Some((modal_action, &assets))).await.map(|_| ());
                            assets.set_finished(if result.is_ok() { ProgressTrackerFinishType::Normal } else { ProgressTrackerFinishType::Error });
                            result
                        },
                        Ok(Ok(None)) => Ok(()),
                        Ok(Err(error)) => Err(error),
                        Err(_) => Err("Global ancestor update check timed out".into()),
                    }
                },
                Some(crate::profile_branch::ProfileParentRef::LocalProfile { profile_uuid }) => {
                    if self.find_instance_id_for_profile_uuid(profile_uuid).is_some() {
                        self.update_local_profile_branch(instance).await.map(|_| ())
                    } else {
                        Ok(())
                    }
                },
                None => Ok(()),
            };
            if let Err(error) = result {
                step.set_finished(ProgressTrackerFinishType::Error);
                overall.set_finished(ProgressTrackerFinishType::Error);
                return Err(error);
            }
            step.set_finished(ProgressTrackerFinishType::Normal);
            overall.add_count(1);
        }
        overall.set_finished(ProgressTrackerFinishType::Normal);
        Ok(())
    }
}

/// Metadata-only tree stamp: catches edits outside the launcher without reading mod contents.
/// Volatile game output and private worlds never affect inherited profile updates.
fn parent_tree_fingerprint(root: &Path) -> Result<String, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|e| format!("Unable to inspect parent files: {e}"))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
            if directory == root && matches!(entry.file_name().to_string_lossy().to_ascii_lowercase().as_str(),
                "saves" | "logs" | "crash-reports" | "screenshots" | "session.lock" | "usercache.json") { continue; }
            #[cfg(windows)]
            if junction::exists(&path).unwrap_or(false) { continue; }
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() { continue; }
            if kind.is_dir() { pending.push(path); }
            else if kind.is_file() {
                let metadata = entry.metadata().map_err(|e| e.to_string())?;
                let modified = metadata.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |duration| duration.as_nanos());
                files.push((relative.to_string_lossy().replace('\\', "/"), metadata.len(), modified));
            }
        }
    }
    files.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (path, size, modified) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(size.to_le_bytes());
        hasher.update(modified.to_le_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

impl BackendState {
    pub async fn update_local_profile_branch(&self, id: InstanceID) -> Result<bool, String> {
        let child_snapshot = self.persistent_profile_branch_status(id, None).map_err(|error| error.to_string())?;
        let Some(crate::profile_branch::ProfileParentRef::LocalProfile {
            profile_uuid: parent_uuid,
        }) = child_snapshot.branch.lineage.parent.as_ref()
        else {
            return Err("This instance is not a local branch".into());
        };
        let parent_id = self
            .find_instance_id_for_profile_uuid(*parent_uuid)
            .ok_or_else(|| "The local parent instance is no longer available".to_owned())?;
        let parent_snapshot = self
            .persistent_profile_branch_status(parent_id, None)
            .map_err(|error| error.to_string())?;
        if parent_snapshot.profile_uuid != *parent_uuid {
            return Err("The selected local parent identity changed".into());
        }
        let parent_root = {
            let state = self.instance_state.read();
            state
                .instances
                .get(parent_id)
                .ok_or_else(|| "The local parent instance is no longer available".to_owned())?
                .dot_minecraft_path
                .to_path_buf()
        };

        let previous = child_snapshot.branch.entries.clone();
        let parent_branch_for_scan = parent_snapshot.branch.clone();
        let parent_uuid_value = *parent_uuid;
        let entries = tokio::task::spawn_blocking(move || {
            snapshot_parent_tree(&parent_root, parent_uuid_value, &parent_branch_for_scan, Some(&previous))
        })
        .await
        .map_err(|error| format!("Parent snapshot task failed: {error}"))??;
        let delta = local_parent_delta(
            &child_snapshot.branch,
            *parent_uuid,
            &parent_snapshot.branch.lineage,
            parent_snapshot.branch.applied_revision.clone(),
            &parent_snapshot.branch.config_settings,
            entries,
        )?;
        let changed = !delta.changes.is_empty()
            || child_snapshot.branch.lineage != delta.lineage
            || child_snapshot.branch.applied_revision != delta.target_revision
            || child_snapshot.branch.config_settings != delta.config_settings;
        let outcome = self.apply_persistent_profile_delta(id, &delta).map_err(|error| error.to_string())?;
        match outcome {
            crate::profile_layout_flow::ReconcileOutcome::Ready { .. } => { self.record_parent_version(id, &parent_snapshot)?; Ok(changed) },
            crate::profile_layout_flow::ReconcileOutcome::NeedsReconcile { conflicts, .. } => {
                Err(format!("Local parent update needs attention: {}", conflicts.join(", ")))
            },
        }
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| format!("Unable to open {path:?}: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 128 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| format!("Unable to read {path:?}: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(std::path::PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("pandora-local-branch-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn local_parent_snapshot_tracks_the_full_dot_minecraft_tree_and_removals() {
        let root = TestRoot::new();
        let minecraft = root.0.join(".minecraft");
        fs::create_dir_all(minecraft.join("config")).unwrap();
        fs::create_dir_all(minecraft.join("resourcepacks/pack")).unwrap();
        fs::write(minecraft.join("config/client.toml"), b"quality = 1\n").unwrap();
        fs::write(minecraft.join("resourcepacks/pack/pack.mcmeta"), b"{\"pack\":{}}\n").unwrap();

        let parent_uuid = Uuid::new_v4();
        let parent_branch = ProfileBranchManifest::default();
        let first = snapshot_parent_tree(&minecraft, parent_uuid, &parent_branch, None).unwrap();
        assert_eq!(first.len(), 2);
        let child_branch = ProfileBranchManifest::default();
        let initial_delta =
            local_parent_delta(&child_branch, parent_uuid, &parent_branch.lineage, None, &[], first.clone()).unwrap();
        assert_eq!(initial_delta.changes.len(), 2);
        assert!(initial_delta.changes.iter().all(|change| matches!(change, ProfileDeltaChange::Upsert(_))));

        fs::remove_file(minecraft.join("config/client.toml")).unwrap();
        fs::write(minecraft.join("resourcepacks/pack/pack.mcmeta"), b"{\"pack\":{\"v\":2}}\n").unwrap();
        fs::write(minecraft.join("new-file.txt"), b"new\n").unwrap();
        let previous = first.into_iter().map(|entry| (entry.path, entry.metadata)).collect::<BTreeMap<_, _>>();
        let updated = snapshot_parent_tree(&minecraft, parent_uuid, &parent_branch, Some(&previous)).unwrap();
        let delta = local_parent_delta(
            &ProfileBranchManifest {
                entries: previous,
                ..ProfileBranchManifest::default()
            },
            parent_uuid,
            &parent_branch.lineage,
            None,
            &[],
            updated,
        )
        .unwrap();
        assert!(
            delta.changes.iter().any(
                |change| matches!(change, ProfileDeltaChange::Remove { path, .. } if path == "config/client.toml")
            )
        );
        assert!(
            delta
                .changes
                .iter()
                .any(|change| matches!(change, ProfileDeltaChange::Upsert(entry) if entry.path == "new-file.txt"))
        );
        assert!(delta.changes.iter().any(|change| matches!(change, ProfileDeltaChange::Upsert(entry) if entry.path == "resourcepacks/pack/pack.mcmeta")));
    }

    #[test]
    fn disabled_parent_mod_is_snapshotted_as_the_enabled_mod_identity() {
        let root = TestRoot::new();
        let minecraft = root.0.join(".minecraft");
        let mods = minecraft.join("mods");
        fs::create_dir_all(&mods).unwrap();
        let disabled_file = mods.join("example.jar.disabled");
        fs::write(&disabled_file, b"mod bytes").unwrap();

        let parent_uuid = Uuid::new_v4();
        let entries = snapshot_parent_tree(&minecraft, parent_uuid, &ProfileBranchManifest::default(), None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "mods/example.jar");
        assert_eq!(entries[0].source, disabled_file);
    }
}
