use std::{
    fs,
    path::{Path, PathBuf},
};

use bridge::{
    instance::{InstanceID, InstanceStatus},
    message::SaveGroupSummary,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{BackendState, instance::Instance};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SaveGroupRecord {
    id: Uuid,
    name: String,
}

fn record_path(root: &Path) -> PathBuf {
    root.join("group.json")
}
fn group_saves(root: &Path) -> PathBuf {
    root.join("saves")
}

fn is_link(path: &Path) -> bool {
    #[cfg(windows)]
    {
        junction::exists(path).unwrap_or(false) || fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    }
    #[cfg(not(windows))]
    {
        fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    }
}

fn make_link(target: &Path, link: &Path) -> Result<(), String> {
    // Resolve the target from the launcher's filesystem namespace before
    // storing it in a Windows junction. Packaged/dev hosts can redirect
    // AppData paths; a junction to the logical path may then escape that
    // namespace and become unreadable to the launcher itself.
    let target = fs::canonicalize(target).map_err(|e| format!("Unable to resolve save-group storage: {e}"))?;
    #[cfg(windows)]
    {
        junction::create(&target, link).map_err(|e| format!("Unable to link saves to the group: {e}"))
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, link).map_err(|e| format!("Unable to link saves to the group: {e}"))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (target, link);
        Err("Save groups are not supported on this platform".into())
    }
}

fn remove_link(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        if junction::exists(path).unwrap_or(false) {
            junction::delete(path).map_err(|e| e.to_string())
        } else {
            fs::remove_file(path).map_err(|e| e.to_string())
        }
    }
    #[cfg(unix)]
    {
        fs::remove_file(path).map_err(|e| e.to_string())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        Err("Save groups are not supported on this platform".into())
    }
}

fn ensure_stopped(instance: &Instance) -> Result<(), String> {
    if instance.status() != InstanceStatus::NotRunning {
        Err("Stop this instance before changing its save group".into())
    } else {
        Ok(())
    }
}

fn validate_plain_saves(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || is_link(path) => {
            Err("The instance saves path is already a link not managed by a save group".into())
        },
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err("The instance saves path is not a directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Unable to inspect saves folder: {error}")),
    }
}

fn load_records(base: &Path) -> Result<Vec<(SaveGroupRecord, PathBuf)>, String> {
    let mut result = Vec::new();
    let entries = match fs::read_dir(base) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(format!("Unable to read save groups: {error}")),
    };
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let root = entry.path();
        if !root.is_dir() || is_link(&root) {
            continue;
        }
        let Ok(record) = fs::read(record_path(&root))
            .and_then(|bytes| serde_json::from_slice::<SaveGroupRecord>(&bytes).map_err(std::io::Error::other))
        else {
            continue;
        };
        if record.id.to_string() == entry.file_name().to_string_lossy() {
            result.push((record, root));
        }
    }
    Ok(result)
}

fn group_for_link(saves: &Path, groups: &[(SaveGroupRecord, PathBuf)]) -> Result<Option<Uuid>, String> {
    if !is_link(saves) {
        return Ok(None);
    }
    let target = fs::canonicalize(saves);
    // If an older junction points at the logical AppData path, its target may
    // be inaccessible inside the process namespace. Still recognize it as a
    // managed group so the user can safely leave it and restore the worlds.
    let lexical_target = fs::read_link(saves).ok();
    for (record, root) in groups {
        let expected_path = group_saves(root);
        let expected = fs::canonicalize(&expected_path).map_err(|e| format!("Unable to resolve group saves: {e}"))?;
        let canonical_match = target.as_ref().is_ok_and(|target| target == &expected);
        let lexical_match = lexical_target
            .as_ref()
            .is_some_and(|target| same_path(target, &expected_path) || same_path(target, &expected));
        if canonical_match || lexical_match {
            return Ok(Some(record.id));
        }
    }
    match target {
        Ok(_) => Err("The saves folder is linked to a location outside managed save groups".into()),
        Err(error) => Err(format!("Unable to resolve saves link: {error}")),
    }
}

pub fn group_id_for_saves_path(saves: &Path, groups_dir: &Path) -> Result<Option<Uuid>, String> {
    let groups = load_records(groups_dir)?;
    group_for_link(saves, &groups)
}

fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    fn normalized(path: &Path) -> String {
        let value = path.to_string_lossy().replace('/', "\\");
        let value = value.strip_prefix("\\\\?\\").unwrap_or(&value);
        value.trim_end_matches(['\\', '/']).to_lowercase()
    }

    #[cfg(windows)]
    {
        normalized(left) == normalized(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn repair_managed_link(saves: &Path, target: &Path) -> Result<(), String> {
    if !is_link(saves) {
        return Err("The saves link changed before it could be repaired".into());
    }

    let target = fs::canonicalize(target).map_err(|e| format!("Unable to resolve group saves: {e}"))?;
    let temporary_link = saves.with_file_name(format!("saves-repair-{}.tmp", Uuid::new_v4()));
    make_link(&target, &temporary_link)?;

    if let Err(error) = remove_link(saves) {
        let _ = remove_link(&temporary_link);
        return Err(format!("Unable to replace the old saves link: {error}"));
    }

    if let Err(error) = fs::rename(&temporary_link, saves) {
        let restore = make_link(&target, saves);
        let _ = remove_link(&temporary_link);
        return match restore {
            Ok(()) => Ok(()),
            Err(restore_error) => Err(format!(
                "Unable to install the repaired saves link: {error}; the group data remains safe at {target:?}, but link restoration also failed: {restore_error}"
            )),
        };
    }

    Ok(())
}

fn instance_paths(state: &BackendState, id: InstanceID) -> Result<(PathBuf, String), String> {
    let guard = state.instance_state.read();
    let instance = guard.instances.get(id).ok_or("Unknown instance")?;
    ensure_stopped(instance)?;
    Ok((instance.saves_path.to_path_buf(), instance.name.to_string()))
}

fn current_members(
    state: &BackendState,
    group_root: &Path,
    except: Option<InstanceID>,
) -> Result<Vec<InstanceID>, String> {
    let expected = fs::canonicalize(group_saves(group_root)).map_err(|e| e.to_string())?;
    let guard = state.instance_state.read();
    let mut members = Vec::new();
    for instance in guard.instances.iter() {
        let id = instance.id;
        if Some(id) == except {
            continue;
        }
        if is_link(&instance.saves_path) && fs::canonicalize(&instance.saves_path).is_ok_and(|p| p == expected) {
            ensure_stopped(instance)?;
            members.push(id);
        }
    }
    Ok(members)
}

fn member_count(state: &BackendState, group_root: &Path) -> usize {
    let Ok(expected) = fs::canonicalize(group_saves(group_root)) else {
        return 0;
    };
    state
        .instance_state
        .read()
        .instances
        .iter()
        .filter(|instance| {
            is_link(&instance.saves_path) && fs::canonicalize(&instance.saves_path).is_ok_and(|path| path == expected)
        })
        .count()
}

fn rollback_moves(moved: &[(PathBuf, PathBuf)]) -> Vec<String> {
    moved
        .iter()
        .rev()
        .filter_map(|(source, target)| {
            fs::rename(target, source)
                .err()
                .map(|error| format!("Could not roll {target:?} back to {source:?}: {error}"))
        })
        .collect()
}

fn candidate_name(name: &str, source: &str, occupied: &mut std::collections::HashSet<String>) -> String {
    let base = format!("{name} (from {})", sanitize_filename::sanitize(source));
    let mut candidate = base.clone();
    let mut suffix = 2;
    while !occupied.insert(candidate.to_lowercase()) {
        candidate = format!("{base} ({suffix})");
        suffix += 1;
    }
    candidate
}

fn join_contents(source: &Path, target: &Path, instance_name: &str) -> Result<(), String> {
    let entries = fs::read_dir(source)
        .map_err(|e| format!("Unable to read instance saves: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut occupied = fs::read_dir(target)
        .map_err(|e| format!("Unable to read group saves: {e}"))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for entry in entries {
        let from = entry.path();
        let meta = match fs::symlink_metadata(&from) {
            Ok(meta) => meta,
            Err(error) => {
                let rollback = rollback_moves(&moved);
                return Err(format!(
                    "Unable to inspect save item {from:?}: {error}. {}",
                    if rollback.is_empty() {
                        "Earlier moves were rolled back.".to_owned()
                    } else {
                        rollback.join("; ")
                    }
                ));
            },
        };
        if meta.file_type().is_symlink() || is_link(&from) || !(meta.is_dir() || meta.is_file()) {
            let rollback = rollback_moves(&moved);
            return Err(format!(
                "Unsupported or linked item in saves folder: {from:?}. {}",
                if rollback.is_empty() {
                    "Earlier moves were rolled back.".to_owned()
                } else {
                    rollback.join("; ")
                }
            ));
        }
        let raw = entry.file_name().to_string_lossy().into_owned();
        let target_name = if occupied.insert(raw.to_lowercase()) {
            raw.clone()
        } else {
            candidate_name(&raw, instance_name, &mut occupied)
        };
        let to = target.join(target_name);
        if let Err(error) = fs::rename(&from, &to) {
            let rollback = rollback_moves(&moved);
            return Err(format!(
                "Unable to move {from:?} into save group (no files were intentionally overwritten): {error}. {}",
                if rollback.is_empty() {
                    "Earlier moves were rolled back.".to_owned()
                } else {
                    rollback.join("; ")
                }
            ));
        }
        moved.push((from, to));
    }
    Ok(())
}

impl BackendState {
    pub fn list_save_groups(&self, id: InstanceID) -> Result<Vec<SaveGroupSummary>, String> {
        let saves = self
            .instance_state
            .read()
            .instances
            .get(id)
            .ok_or("Unknown instance")?
            .saves_path
            .to_path_buf();
        let groups = load_records(&self.directories.save_groups_dir)?;
        let mut selected = group_for_link(&saves, &groups)?;
        if let Some(group_id) = selected
            && fs::canonicalize(&saves).is_err()
        {
            let status = self.instance_state.read().instances.get(id).ok_or("Unknown instance")?.status();
            if status != InstanceStatus::NotRunning {
                return Err("Stop this instance before repairing its save-group link".into());
            }
            let (_, root) = groups.iter().find(|(record, _)| record.id == group_id).ok_or("Save group not found")?;
            repair_managed_link(&saves, &group_saves(root))?;
            selected = group_for_link(&saves, &groups)?;
        }
        Ok(groups
            .into_iter()
            .map(|(record, root)| SaveGroupSummary {
                id: record.id,
                name: record.name,
                member_count: member_count(self, &root),
                selected: selected == Some(record.id),
            })
            .collect())
    }

    pub fn create_save_group(&self, id: InstanceID, name: String) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.len() > 80 {
            return Err("Group name must contain 1 to 80 characters".into());
        }
        let (saves, _) = instance_paths(self, id)?;
        validate_plain_saves(&saves)?;
        let groups = load_records(&self.directories.save_groups_dir)?;
        if group_for_link(&saves, &groups)?.is_some() {
            return Err("This instance already belongs to a save group".into());
        }
        fs::create_dir_all(&self.directories.save_groups_dir).map_err(|e| e.to_string())?;
        let record = SaveGroupRecord {
            id: Uuid::new_v4(),
            name: name.to_owned(),
        };
        let root = self.directories.save_groups_dir.join(record.id.to_string());
        fs::create_dir(&root).map_err(|e| e.to_string())?;
        let record_bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
        if let Err(error) = fs::write(record_path(&root), record_bytes) {
            let _ = fs::remove_dir_all(&root);
            return Err(format!("Unable to save group metadata: {error}"));
        }
        let group_worlds = group_saves(&root);
        let source_existed = saves.exists();
        if source_existed {
            if let Err(error) = fs::rename(&saves, &group_worlds) {
                let _ = fs::remove_dir_all(&root);
                return Err(format!("Unable to move worlds into group storage: {error}"));
            }
        } else if let Err(error) = fs::create_dir(&group_worlds) {
            let _ = fs::remove_dir_all(&root);
            return Err(error.to_string());
        }
        if let Err(error) = make_link(&group_worlds, &saves) {
            let rollback = if source_existed {
                fs::rename(&group_worlds, &saves).map_err(|e| e.to_string())
            } else {
                fs::remove_dir(&group_worlds).map_err(|e| e.to_string())
            };
            if rollback.is_ok() {
                let _ = fs::remove_dir_all(&root);
            }
            return Err(format!(
                "{error}; {}",
                rollback
                    .err()
                    .map_or("original saves state restored".to_owned(), |e| format!("rollback needs attention: {e}"))
            ));
        }
        Ok(())
    }

    pub fn join_save_group(&self, id: InstanceID, group_id: Uuid) -> Result<(), String> {
        let (saves, instance_name) = instance_paths(self, id)?;
        validate_plain_saves(&saves)?;
        let groups = load_records(&self.directories.save_groups_dir)?;
        if group_for_link(&saves, &groups)?.is_some() {
            return Err("This instance already belongs to a save group".into());
        }
        let (record, root) = groups
            .into_iter()
            .find(|(record, _)| record.id == group_id)
            .ok_or("Save group not found")?;
        let _ = record;
        current_members(self, &root, None)?; // membership changes must not move worlds while another member is playing
        let target = group_saves(&root);
        fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        let existed = saves.exists();
        if !existed {
            fs::create_dir_all(&saves).map_err(|e| e.to_string())?;
        }
        join_contents(&saves, &target, &instance_name)?;
        if let Err(error) = remove_dir_if_empty(&saves) {
            return Err(error);
        }
        if let Err(error) = make_link(&target, &saves) {
            fs::create_dir_all(&saves).ok();
            return Err(format!(
                "{error}. The worlds already moved into the group and remain safe there; rejoin after fixing the link."
            ));
        }
        Ok(())
    }

    pub fn leave_save_group(&self, id: InstanceID) -> Result<(), String> {
        let (saves, _) = instance_paths(self, id)?;
        let groups = load_records(&self.directories.save_groups_dir)?;
        let group_id = group_for_link(&saves, &groups)?.ok_or("This instance is not in a save group")?;
        let (_, root) = groups
            .into_iter()
            .find(|(record, _)| record.id == group_id)
            .ok_or("Save group not found")?;
        let others = current_members(self, &root, Some(id))?;
        remove_link(&saves).map_err(|e| format!("Unable to detach this instance from its save group: {e}"))?;
        if others.is_empty() {
            if let Err(error) = fs::rename(group_saves(&root), &saves) {
                let _ = make_link(&group_saves(&root), &saves);
                return Err(format!("Unable to return group worlds to the last instance: {error}"));
            }
            fs::remove_dir_all(root)
                .map_err(|e| format!("Worlds were returned, but empty group metadata remains: {e}"))?;
        } else {
            fs::create_dir(&saves)
                .map_err(|e| format!("Detached from group, but unable to create empty local saves folder: {e}"))?;
        }
        Ok(())
    }
}

fn remove_dir_if_empty(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error) => Err(format!("Unable to replace local saves folder: {error}")),
    }
}
