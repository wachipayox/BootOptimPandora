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
