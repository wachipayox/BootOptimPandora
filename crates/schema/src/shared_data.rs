//! Portable shared-data presets and segment-aware scope selection.
use crate::backend_config::SyncTargets;

pub const GLOBAL_FOLDERS: &[&str] = &[
    "screenshots",
    "Distant_Horizons_server_data",
    ".voxy",
    "journeymap",
    "flashback",
    "xaero",
    ".bobby",
    ".analogaudio",
    "schematics",
    ".sable",
];
pub const GROUP_FILES: &[&str] = &["command_history.txt", "hotbar.nbt"];
pub const GROUP_FOLDERS: &[&str] = &["saves", "config/litematica", "Sable-Schematics", "music_sheets"];
pub const RETIRED_GLOBAL_FILES: &[&str] = &["options.txt", "servers.dat", "command_history.txt", "hotbar.nbt"];
pub const RETIRED_GLOBAL_FOLDERS: &[&str] = &["saves", "config", "resourcepacks", "downloads", "shaderpacks"];

pub fn global_defaults() -> SyncTargets {
    SyncTargets {
        files: Default::default(),
        folders: GLOBAL_FOLDERS.iter().map(|s| (*s).into()).collect(),
    }
}
pub fn group_defaults() -> SyncTargets {
    SyncTargets {
        files: GROUP_FILES.iter().map(|s| (*s).into()).collect(),
        folders: GROUP_FOLDERS.iter().map(|s| (*s).into()).collect(),
    }
}

/// Per-instance choices are dormant while the master switch is off. An absent
/// path follows the current global value, rather than a copied snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct InstanceGlobalSharing {
    pub enabled: bool,
    pub choices: std::collections::BTreeMap<String, bool>,
}
impl InstanceGlobalSharing {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}
pub fn global_catalog(global: &SyncTargets) -> std::collections::BTreeMap<String, bool> {
    let mut result: std::collections::BTreeMap<String, bool> =
        GLOBAL_FOLDERS.iter().map(|p| ((*p).into(), false)).collect();
    result.extend(global.folders.iter().map(|p| (p.to_string(), false)));
    result.extend(global.files.iter().map(|p| (p.to_string(), true)));
    result
}
pub fn effective_global(global: &SyncTargets, local: &InstanceGlobalSharing, disabled: bool) -> SyncTargets {
    if disabled {
        return SyncTargets::default();
    }
    let mut result = global.clone();
    if local.enabled {
        for (path, is_file) in global_catalog(global) {
            let Some(enabled) = local.choices.get(&path) else {
                continue;
            };
            result.files.remove(path.as_str());
            result.folders.remove(path.as_str());
            if *enabled {
                if is_file {
                    result.files.insert(path.into());
                } else {
                    result.folders.insert(path.into());
                }
            }
        }
    }
    result
}

/// Apply once without resetting later user choices or dropping custom routes.
pub fn migrate_global_defaults(config: &mut crate::backend_config::BackendConfig) -> bool {
    if config.shared_data_defaults_version >= 1 {
        return false;
    }
    config.previous_sync_targets = Some(config.sync_targets.clone());
    config
        .sync_targets
        .files
        .retain(|path| !RETIRED_GLOBAL_FILES.iter().any(|old| old.eq_ignore_ascii_case(path)));
    config
        .sync_targets
        .folders
        .retain(|path| !RETIRED_GLOBAL_FOLDERS.iter().any(|old| old.eq_ignore_ascii_case(path)));
    config.sync_targets.folders.extend(global_defaults().folders);
    config.shared_data_defaults_version = 1;
    true
}

/// Matching is conservative on every platform so settings remain portable to Windows.
/// Callers validate paths before storage; this function never resolves filesystem links.
pub fn contains_path(target: &str, is_file: bool, path: &str) -> bool {
    target.eq_ignore_ascii_case(path)
        || (!is_file
            && path.get(..target.len()).is_some_and(|p| p.eq_ignore_ascii_case(target))
            && path.as_bytes().get(target.len()) == Some(&b'/'))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedScope {
    Global,
    Group,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedTarget<'a> {
    pub path: &'a str,
    pub is_file: bool,
    pub scope: SharedScope,
}

/// A group's route has priority over any global route covering the same file.
pub fn resolve<'a>(path: &str, global: &'a SyncTargets, group: Option<&'a SyncTargets>) -> Option<SharedTarget<'a>> {
    let find = |targets: &'a SyncTargets, scope| {
        targets
            .files
            .iter()
            .map(|p| SharedTarget {
                path: p.as_ref(),
                is_file: true,
                scope,
            })
            .chain(targets.folders.iter().map(|p| SharedTarget {
                path: p.as_ref(),
                is_file: false,
                scope,
            }))
            .filter(|target| contains_path(target.path, target.is_file, path))
            .max_by_key(|target| target.path.len())
    };
    group
        .and_then(|g| find(g, SharedScope::Group))
        .or_else(|| find(global, SharedScope::Global))
}

/// A folder is excluded as a whole when any of its descendants is enforced.
pub fn inherited_conflict<'a>(
    target: SharedTarget<'_>,
    enforced: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    enforced.into_iter().find(|path| contains_path(target.path, target.is_file, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn instance_global_choices_follow_live_defaults_and_do_not_invent_routes() {
        let mut global = global_defaults();
        let mut local = InstanceGlobalSharing::default();
        local.choices.insert("screenshots".into(), false);
        assert!(effective_global(&global, &local, false).folders.contains("screenshots"));
        local.enabled = true;
        assert!(!effective_global(&global, &local, false).folders.contains("screenshots"));
        local.choices.remove("screenshots");
        global.folders.remove("screenshots");
        assert!(!effective_global(&global, &local, false).folders.contains("screenshots"));
        local.choices.insert("screenshots".into(), true);
        local.choices.insert("not-globally-configured".into(), true);
        let chosen = effective_global(&global, &local, false);
        assert!(chosen.folders.contains("screenshots"));
        assert!(!chosen.folders.contains("not-globally-configured"));
        global.files.insert("custom.txt".into());
        local.choices.insert("custom.txt".into(), false);
        assert!(!effective_global(&global, &local, false).files.contains("custom.txt"));
        let saved = serde_json::to_vec(&local).unwrap();
        let reopened: InstanceGlobalSharing = serde_json::from_slice(&saved).unwrap();
        assert_eq!(reopened, local);
        assert!(effective_global(&global, &local, true).folders.is_empty());
    }
    #[test]
    fn migration_retains_custom_paths_and_is_not_reapplied_after_user_disables() {
        let mut config = crate::backend_config::BackendConfig::default();
        config.sync_targets.files.insert("custom.txt".into());
        config.sync_targets.folders.insert("config".into());
        config.sync_targets.files.insert("servers.dat".into());
        let original = config.sync_targets.clone();
        assert!(migrate_global_defaults(&mut config));
        assert_eq!(config.previous_sync_targets, Some(original));
        assert!(config.sync_targets.files.contains("custom.txt"));
        assert!(!config.sync_targets.files.contains("servers.dat"));
        assert!(!config.sync_targets.folders.contains("config"));
        config.sync_targets.folders.remove("screenshots");
        let saved = serde_json::to_vec(&config).unwrap();
        let mut reopened: crate::backend_config::BackendConfig = serde_json::from_slice(&saved).unwrap();
        assert!(!migrate_global_defaults(&mut reopened));
        assert!(!reopened.sync_targets.folders.contains("screenshots"));
    }
    #[test]
    fn folders_match_segments_and_portable_case_not_similar_names() {
        assert!(contains_path("config/litematica", false, "Config/Litematica/a.json"));
        assert!(!contains_path("config/litematica", false, "config/litematica-other/a.json"));
        assert!(!contains_path("hotbar.nbt", true, "hotbar.nbt/child"));
        assert!(!contains_path("config", false, "config2/x"));
    }
    #[test]
    fn group_scope_wins_even_when_global_target_is_more_specific() {
        let global = SyncTargets {
            files: ["config/litematica/a.json".into()].into(),
            folders: Default::default(),
        };
        let group = group_defaults();
        assert_eq!(
            resolve("config/litematica/a.json", &global, Some(&group)).unwrap().scope,
            SharedScope::Group
        );
        assert_eq!(resolve("config/litematica/a.json", &global, None).unwrap().scope, SharedScope::Global);
    }
    #[test]
    fn enforced_descendant_excludes_whole_folder_but_not_neighbor() {
        let group = group_defaults();
        let global = global_defaults();
        let target = resolve("config/litematica/a.json", &global, Some(&group)).unwrap();
        assert_eq!(inherited_conflict(target, ["config/litematica/a.json"]), Some("config/litematica/a.json"));
        assert_eq!(inherited_conflict(target, ["config/litematica-other/a.json"]), None);
    }
}
