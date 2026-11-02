//! Shared-data plans and per-instance ownership receipts, outside .minecraft.
use crate::{directories::LauncherDirectories, shared_storage as storage};
use schema::{
    backend_config::SyncTargets,
    ignored_profile_paths::IgnoredProfilePaths,
    shared_data::{SharedScope, SharedTarget, contains_path},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const CONTROL: &str = ".pandora-shared-v1";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Scope {
    Global,
    Group {
        id: Uuid,
        name: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Route {
    pub path: String,
    pub is_file: bool,
    pub scope: Scope,
    pub conflict: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Folder {
    route: Route,
    target: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct File {
    route: Route,
    target: PathBuf,
    seed: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Receipt {
    #[serde(default)]
    legacy_global_migrated: bool,
    #[serde(default)]
    legacy_group_seen: Option<Uuid>,
    #[serde(default)]
    routes: Vec<Route>,
    #[serde(default)]
    folders: BTreeMap<String, Folder>,
    #[serde(default)]
    files: BTreeMap<String, File>,
}

pub(crate) struct Group {
    pub id: Uuid,
    pub name: String,
    pub targets: SyncTargets,
}
pub(crate) struct Context<'a> {
    pub root: &'a Path,
    pub minecraft: &'a Path,
    pub directories: &'a LauncherDirectories,
    pub global: &'a SyncTargets,
    pub previous: Option<&'a SyncTargets>,
    pub group: Option<&'a Group>,
    pub ignored: &'a IgnoredProfilePaths,
}
fn error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn control(root: &Path) -> io::Result<PathBuf> {
    let path = root.join(CONTROL);
    if storage::is_link(&path) {
        return Err(error("Shared ownership control is a link"));
    }
    fs::create_dir_all(&path)?;
    Ok(path)
}
fn read(root: &Path) -> io::Result<Receipt> {
    let path = root.join(CONTROL).join("state.json");
    if !path.exists() {
        return Ok(Receipt::default());
    }
    if storage::is_link(&root.join(CONTROL)) || storage::is_link(&path) {
        return Err(error("Shared ownership receipt is linked"));
    }
    if fs::metadata(&path)?.len() > 4 * 1024 * 1024 {
        return Err(error("Shared ownership receipt is oversized"));
    }
    let receipt: Receipt = serde_json::from_slice(&fs::read(path)?).map_err(|e| error(e.to_string()))?;
    for route in receipt
        .routes
        .iter()
        .chain(receipt.folders.values().map(|f| &f.route))
        .chain(receipt.files.values().map(|f| &f.route))
    {
        validate_route(&route.path)?;
    }
    Ok(receipt)
}
fn write(root: &Path, receipt: &Receipt) -> io::Result<()> {
    crate::fs::write_safe(
        &control(root)?.join("state.json"),
        &serde_json::to_vec_pretty(receipt).map_err(|e| error(e.to_string()))?,
    )
}

pub(crate) fn validate_route(path: &str) -> io::Result<()> {
    let safe = bridge::safe_path::SafePath::new(path).ok_or_else(|| error("Unsafe shared path"))?;
    if Path::new(path).is_absolute()
        || path.starts_with('/')
        || safe.as_str() != path
        || path == "."
        || path.split('/').any(|part| part.to_ascii_lowercase().starts_with(".pandora-"))
    {
        return Err(error("Shared paths must be portable relative paths outside launcher control state"));
    }
    if matches!(
        path.split('/').next().unwrap_or("").to_ascii_lowercase().as_str(),
        "assets" | "libraries" | "versions" | "natives" | "runtime"
    ) {
        return Err(error("Launcher runtime paths cannot be shared"));
    }
    Ok(())
}
pub(crate) fn validate_targets(targets: &SyncTargets) -> io::Result<()> {
    for path in targets.files.iter().chain(&targets.folders) {
        validate_route(path)?;
    }
    if targets.files.iter().any(|p| targets.folders.iter().any(|d| p.eq_ignore_ascii_case(d))) {
        return Err(error("A shared path cannot be both file and folder"));
    }
    Ok(())
}
fn overlaps(a: &str, a_file: bool, b: &str, b_file: bool) -> bool {
    contains_path(a, a_file, b) || contains_path(b, b_file, a)
}
fn protected(root: &Path, ignored: &IgnoredProfilePaths) -> io::Result<BTreeSet<String>> {
    let Some(manifest) = crate::profile_layout_flow::read_committed_manifest(root).map_err(|e| error(e.to_string()))?
    else {
        return Ok(BTreeSet::new());
    };
    if manifest.state != crate::profile_layout_flow::ProfileLayoutState::Ready {
        return Err(error("Inherited profile recovery must finish before sharing changes"));
    }
    let mut paths: BTreeSet<_> = manifest
        .branch
        .entries
        .iter()
        .filter(|(p, m)| m.policy == crate::profile_branch::ProfileFilePolicy::Enforced && !ignored.contains(p))
        .map(|(p, _)| p.clone())
        .collect();
    paths.extend(manifest.branch.tombstones.keys().filter(|p| !ignored.contains(p)).cloned());
    paths.extend(
        manifest
            .branch
            .config_settings
            .iter()
            .filter(|r| r.policy == "enforced" && !ignored.contains(&r.path))
            .map(|r| r.path.clone()),
    );
    Ok(paths)
}
fn add_routes(targets: &SyncTargets, scope: Scope, out: &mut Vec<Route>) {
    for (paths, is_file) in [(&targets.folders, false), (&targets.files, true)] {
        for path in paths {
            out.push(Route {
                path: path.to_string(),
                is_file,
                scope: scope.clone(),
                conflict: None,
            });
        }
    }
}
fn plan(context: &Context<'_>) -> io::Result<Vec<Route>> {
    validate_targets(context.global)?;
    if let Some(g) = context.group {
        validate_targets(&g.targets)?;
    }
    let mut group = Vec::new();
    if let Some(g) = context.group {
        add_routes(
            &g.targets,
            Scope::Group {
                id: g.id,
                name: g.name.clone(),
            },
            &mut group,
        );
    }
    let mut routes = Vec::new();
    add_routes(context.global, Scope::Global, &mut routes);
    // A directory link cannot contain a second owner. A group overlap excludes that global route.
    routes.retain(|route| !group.iter().any(|g| overlaps(&route.path, route.is_file, &g.path, g.is_file)));
    routes.extend(group);
    routes.sort_by_key(|r| r.path.len());
    let mut effective: Vec<Route> = Vec::new();
    for route in routes {
        if !effective
            .iter()
            .any(|p| !p.is_file && p.scope == route.scope && contains_path(&p.path, false, &route.path))
        {
            effective.push(route);
        }
    }
    let enforced = protected(context.root, context.ignored)?;
    for route in &mut effective {
        let target = SharedTarget {
            path: &route.path,
            is_file: route.is_file,
            scope: match route.scope {
                Scope::Global => SharedScope::Global,
                _ => SharedScope::Group,
            },
        };
        route.conflict =
            schema::shared_data::inherited_conflict(target, enforced.iter().map(String::as_str)).map(str::to_owned);
    }
    Ok(effective)
}
fn target(context: &Context<'_>, route: &Route) -> io::Result<PathBuf> {
    let (base, relative) = match route.scope {
        Scope::Global => (context.directories.synced_dir.to_path_buf(), route.path.clone()),
        Scope::Group { id, .. } => {
            let base = context.directories.save_groups_dir.join(id.to_string());
            if !base.is_dir() || storage::is_link(&base) {
                return Err(error("Shared group storage is unavailable"));
            }
            (
                base,
                if route.path == "saves" {
                    "saves".into()
                } else {
                    format!("shared/{}", route.path)
                },
            )
        },
    };
    if storage::is_link(&base) {
        return Err(error("Shared storage root is linked"));
    }
    fs::create_dir_all(&base)?;
    storage::checked_path(&base, &relative)
}
fn backup(root: &Path, route: &Route) -> io::Result<PathBuf> {
    Ok(control(root)?.join("recovery").join(&route.path))
}
fn validate_owned_target(context: &Context<'_>, route: &Route, recorded: &Path) -> io::Result<()> {
    if target(context, route)? != recorded {
        return Err(error("Shared ownership receipt storage mismatch"));
    }
    Ok(())
}
/// Does not traverse .minecraft: only selected route metadata and the committed manifest.
pub(crate) fn routes(context: &Context<'_>) -> io::Result<Vec<Route>> {
    plan(context)
}

pub(crate) fn apply(context: &Context<'_>) -> io::Result<bool> {
    fs::create_dir_all(context.minecraft)?;
    let desired = plan(context)?;
    let mut receipt = read(context.root)?;
    let original = receipt.clone();
    let mut folder_changed = false;
    let protected_paths = protected(context.root, context.ignored)?;
    // Adopt only known old global links; never claim an arbitrary external link.
    if let Some(previous) = context.previous.filter(|_| !receipt.legacy_global_migrated) {
        for path in &previous.folders {
            validate_route(path)?;
            if receipt
                .folders
                .values()
                .any(|f| f.route.scope == Scope::Global && contains_path(&f.route.path, false, path))
            {
                continue;
            }
            let local = storage::checked_path(context.minecraft, path)?;
            let route = Route {
                path: path.to_string(),
                is_file: false,
                scope: Scope::Global,
                conflict: None,
            };
            let destination = target(context, &route)?;
            if storage::targeting_or_broken(&local, &destination) {
                receipt.folders.entry(path.to_string()).or_insert(Folder {
                    route,
                    target: destination,
                });
            }
        }
    }
    if let Some(group) = context.group.filter(|g| receipt.legacy_group_seen != Some(g.id)) {
        let route = Route {
            path: "saves".into(),
            is_file: false,
            scope: Scope::Group {
                id: group.id,
                name: group.name.clone(),
            },
            conflict: None,
        };
        let destination = target(context, &route)?;
        let local = storage::checked_path(context.minecraft, "saves")?;
        if storage::targeting_or_broken(&local, &destination) {
            receipt.folders.entry("saves".into()).or_insert(Folder {
                route,
                target: destination,
            });
        }
    }
    receipt.routes = desired.clone();
    if receipt != original {
        write(context.root, &receipt)?;
    }
    for path in receipt.folders.keys().cloned().collect::<Vec<_>>() {
        let old = receipt.folders.get(&path).unwrap().clone();
        validate_owned_target(context, &old.route, &old.target)?;
        let retain = desired.iter().any(|r| {
            r.path == path && !r.is_file && r.conflict.is_none() && target(context, r).is_ok_and(|p| p == old.target)
        });
        if !retain {
            let local = storage::checked_path(context.minecraft, &path)?;
            folder_changed |= storage::targeting_or_broken(&local, &old.target);
            storage::detach_folder(&local, &old.target, &backup(context.root, &old.route)?)?;
            receipt.folders.remove(&path);
            write(context.root, &receipt)?;
        }
    }
    for path in receipt.files.keys().cloned().collect::<Vec<_>>() {
        let old = receipt.files.get(&path).unwrap().clone();
        validate_owned_target(context, &old.route, &old.target)?;
        let retain = desired.iter().any(|r| {
            r.path == path && r.is_file && r.conflict.is_none() && target(context, r).is_ok_and(|p| p == old.target)
        });
        if !retain {
            let local = storage::checked_path(context.minecraft, &path)?;
            if !protected_paths.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
                let _ = storage::publish_file(
                    &local,
                    &old.target,
                    old.seed.as_deref(),
                    &backup(context.root, &old.route)?,
                )?;
            }
            receipt.files.remove(&path);
            write(context.root, &receipt)?;
        }
    }
    for route in desired.iter().filter(|r| r.conflict.is_none()) {
        let destination = target(context, route)?;
        let local = storage::checked_path(context.minecraft, &route.path)?;
        if route.is_file {
            let seed = if let Some(old) = receipt.files.get(&route.path) {
                storage::publish_file(&local, &destination, old.seed.as_deref(), &backup(context.root, route)?)?
            } else {
                storage::attach_file(&local, &destination, &backup(context.root, route)?)?
            };
            let entry = File {
                route: route.clone(),
                target: destination,
                seed,
            };
            if receipt.files.get(&route.path) != Some(&entry) {
                receipt.files.insert(route.path.clone(), entry);
                write(context.root, &receipt)?;
            }
        } else {
            if receipt
                .folders
                .get(&route.path)
                .is_some_and(|old| old.route == *route && old.target == destination)
                && storage::targeting(&local, &destination)
            {
                continue;
            }
            folder_changed |= !storage::targeting(&local, &destination);
            // Persist intent before linking so a crash between the link and receipt write is recoverable.
            receipt.folders.insert(
                route.path.clone(),
                Folder {
                    route: route.clone(),
                    target: destination.clone(),
                },
            );
            write(context.root, &receipt)?;
            if route.path.eq_ignore_ascii_case("saves")
                && local.is_dir()
                && !storage::is_link(&local)
                && destination.is_dir()
            {
                let name = context.root.file_name().and_then(|s| s.to_str()).unwrap_or("instance");
                crate::save_groups::merge_worlds_for_sharing(&local, &destination, name).map_err(error)?;
            }
            storage::attach_folder(&local, &destination, &backup(context.root, route)?)?;
        }
    }
    let group_seen = context.group.map(|g| g.id);
    if !receipt.legacy_global_migrated || receipt.legacy_group_seen != group_seen {
        receipt.legacy_global_migrated = true;
        receipt.legacy_group_seen = group_seen;
        write(context.root, &receipt)?;
    }
    Ok(folder_changed)
}
pub(crate) fn publish(context: &Context<'_>) -> io::Result<()> {
    let mut receipt = read(context.root)?;
    if receipt.files.is_empty() {
        return Ok(());
    }
    let original = receipt.clone();
    for file in receipt.files.values_mut() {
        validate_owned_target(context, &file.route, &file.target)?;
        let local = storage::checked_path(context.minecraft, &file.route.path)?;
        file.seed =
            storage::publish_file(&local, &file.target, file.seed.as_deref(), &backup(context.root, &file.route)?)?;
    }
    if receipt != original {
        write(context.root, &receipt)?;
    }
    Ok(())
}
/// Called under the inherited-layout lock before applying an enforced profile delta.
/// This never writes to the shared source: it materializes a local independent copy.
pub(crate) fn prepare_profile_paths(root: &Path, paths: &[String]) -> io::Result<Vec<String>> {
    let mut receipt = read(root)?;
    let conflicts: Vec<String> = paths
        .iter()
        .filter(|path| receipt.routes.iter().any(|route| contains_path(&route.path, route.is_file, path)))
        .cloned()
        .collect();
    if receipt.folders.is_empty() && receipt.files.is_empty() {
        return Ok(conflicts);
    }
    for path in paths {
        crate::profile_branch::validate_profile_relative_path(path).map_err(|e| error(e.to_string()))?;
    }
    for key in receipt.folders.keys().cloned().collect::<Vec<_>>() {
        let old = receipt.folders.get(&key).unwrap().clone();
        if let Some(conflict) = paths.iter().find(|p| contains_path(&key, false, p)) {
            let local = storage::checked_path(&root.join(".minecraft"), &key)?;
            storage::detach_folder(&local, &old.target, &backup(root, &old.route)?)?;
            receipt.folders.remove(&key);
            for route in &mut receipt.routes {
                if route.path == key {
                    route.conflict = Some(conflict.clone());
                }
            }
            write(root, &receipt)?;
        }
    }
    let removed: Vec<_> = receipt
        .files
        .keys()
        .filter(|key| paths.iter().any(|p| p.eq_ignore_ascii_case(key)))
        .cloned()
        .collect();
    for key in removed {
        receipt.files.remove(&key);
        for route in &mut receipt.routes {
            if route.path == key {
                route.conflict = Some(key.clone());
            }
        }
    }
    write(root, &receipt)?;
    Ok(conflicts)
}
pub(crate) fn active_routes(root: &Path) -> io::Result<Vec<Route>> {
    let receipt = read(root)?;
    let mut routes: Vec<_> = receipt.files.into_values().map(|file| file.route).collect();
    routes.extend(
        receipt
            .folders
            .into_values()
            .filter(|f| storage::targeting(&root.join(".minecraft").join(&f.route.path), &f.target))
            .map(|f| f.route),
    );
    Ok(routes)
}
pub(crate) fn active_shared_path(root: &Path, path: &str) -> io::Result<bool> {
    Ok(active_routes(root)?.iter().any(|route| contains_path(&route.path, route.is_file, path)))
}

/// Storage folders currently exposed to a game through owned directory links.
pub(crate) fn active_folder_roots(root: &Path) -> io::Result<Vec<PathBuf>> {
    Ok(read(root)?
        .folders
        .into_values()
        .filter(|f| storage::targeting(&root.join(".minecraft").join(&f.route.path), &f.target))
        .map(|f| f.target)
        .collect())
}
pub(crate) fn profile_isolation_roots(root: &Path, paths: &[String]) -> io::Result<Vec<PathBuf>> {
    Ok(read(root)?
        .folders
        .into_values()
        .filter(|f| {
            paths.iter().any(|p| contains_path(&f.route.path, false, p))
                && storage::targeting(&root.join(".minecraft").join(&f.route.path), &f.target)
        })
        .map(|f| f.target)
        .collect())
}
pub(crate) fn storage_overlaps(first: &[PathBuf], second: &[PathBuf]) -> bool {
    fn key(path: &Path) -> String {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let value = path.to_string_lossy().replace('\\', "/");
        #[cfg(windows)]
        {
            value.to_lowercase()
        }
        #[cfg(not(windows))]
        {
            value
        }
    }
    first.iter().any(|a| {
        second.iter().any(|b| {
            let a = key(a);
            let b = key(b);
            contains_path(&a, false, &b) || contains_path(&b, false, &a)
        })
    })
}
/// Only selected route metadata is examined; no recursive instance walk.
pub(crate) fn mutation_roots(context: &Context<'_>) -> io::Result<Vec<PathBuf>> {
    let desired = plan(context)?;
    let receipt = read(context.root)?;
    let mut roots = Vec::new();
    let mut detached = Vec::new();
    for old in receipt.folders.values() {
        let retained = desired.iter().any(|r| {
            !r.is_file
                && r.path == old.route.path
                && r.conflict.is_none()
                && target(context, r).is_ok_and(|p| p == old.target)
        });
        if !retained && storage::targeting_or_broken(&context.minecraft.join(&old.route.path), &old.target) {
            roots.push(old.target.clone());
            detached.push((old.route.path.clone(), old.target.clone()));
        }
    }
    if let Some(previous) = context.previous.filter(|_| !receipt.legacy_global_migrated) {
        for path in &previous.folders {
            validate_route(path)?;
            let route = Route {
                path: path.to_string(),
                is_file: false,
                scope: Scope::Global,
                conflict: None,
            };
            let shared = target(context, &route)?;
            let retained = desired
                .iter()
                .any(|r| !r.is_file && r.path == route.path && r.scope == Scope::Global && r.conflict.is_none());
            if !retained && storage::targeting_or_broken(&context.minecraft.join(path.as_ref()), &shared) {
                detached.push((route.path.clone(), shared.clone()));
                roots.push(shared);
            }
        }
    }
    if let Some(group) = context.group.filter(|g| receipt.legacy_group_seen != Some(g.id)) {
        let route = Route {
            path: "saves".into(),
            is_file: false,
            scope: Scope::Group {
                id: group.id,
                name: group.name.clone(),
            },
            conflict: None,
        };
        let shared = target(context, &route)?;
        let retained = desired.iter().any(|r| r.path == "saves" && !r.is_file && r.conflict.is_none());
        if !retained && storage::targeting_or_broken(&context.minecraft.join("saves"), &shared) {
            detached.push((route.path.clone(), shared.clone()));
            roots.push(shared);
        }
    }
    for route in desired.iter().filter(|r| r.conflict.is_none()) {
        let destination = target(context, route)?;
        if route.is_file {
            roots.push(destination);
            continue;
        }
        // Inspect the bytes that a known parent detachment will materialize, without
        // following an arbitrary link or mutating the instance during preflight.
        let local = if let Some((parent, shared)) = detached
            .iter()
            .find(|(parent, _)| parent != &route.path && contains_path(parent, false, &route.path))
        {
            storage::checked_path(shared, &route.path[parent.len() + 1..])?
        } else {
            storage::checked_path(context.minecraft, &route.path)?
        };
        if !storage::targeting(&local, &destination) && local.is_dir() && fs::read_dir(&local)?.next().is_some() {
            roots.push(destination);
        }
    }
    Ok(roots)
}

pub(crate) fn file_storage_roots(root: &Path) -> io::Result<Vec<PathBuf>> {
    Ok(read(root)?.files.into_values().map(|f| f.target).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        directories: LauncherDirectories,
        root: PathBuf,
        minecraft: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let directories =
                LauncherDirectories::new(std::env::temp_dir().join(format!("pandora-sharing-plan-{}", Uuid::new_v4())));
            let root = directories.instances_dir.join("fixture");
            let minecraft = root.join(".minecraft");
            fs::create_dir_all(&minecraft).unwrap();
            Self {
                directories,
                root,
                minecraft,
            }
        }
        fn context<'a>(
            &'a self,
            global: &'a SyncTargets,
            group: Option<&'a Group>,
            ignored: &'a IgnoredProfilePaths,
        ) -> Context<'a> {
            if let Some(group) = group {
                fs::create_dir_all(self.directories.save_groups_dir.join(group.id.to_string())).unwrap();
            }
            Context {
                root: &self.root,
                minecraft: &self.minecraft,
                directories: &self.directories,
                global,
                previous: None,
                group,
                ignored,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert!(self.directories.root_launcher_dir.starts_with(std::env::temp_dir()));
            let _ = fs::remove_dir_all(&self.directories.root_launcher_dir);
        }
    }
    #[test]
    fn per_instance_global_override_detaches_only_that_instance_and_can_follow_global_again() {
        let f = Fixture::new();
        let ignored = IgnoredProfilePaths::default();
        let mut global = SyncTargets::default();
        global.folders.insert("screenshots".into());
        fs::create_dir_all(f.minecraft.join("screenshots")).unwrap();
        fs::write(f.minecraft.join("screenshots/shared.png"), b"shared").unwrap();
        apply(&f.context(&global, None, &ignored)).unwrap();
        let second = f.directories.instances_dir.join("second");
        let minecraft = second.join(".minecraft");
        fs::create_dir_all(minecraft.join("screenshots")).unwrap();
        fs::write(minecraft.join("screenshots/personal.png"), b"personal").unwrap();
        let mut choices = schema::shared_data::InstanceGlobalSharing::default();
        choices.enabled = true;
        choices.choices.insert("screenshots".into(), false);
        let selected = schema::shared_data::effective_global(&global, &choices, false);
        let context = Context {
            root: &second,
            minecraft: &minecraft,
            directories: &f.directories,
            global: &selected,
            previous: None,
            group: None,
            ignored: &ignored,
        };
        apply(&context).unwrap();
        assert!(!storage::is_link(&minecraft.join("screenshots")));
        assert!(storage::is_link(&f.minecraft.join("screenshots")));
        assert_eq!(fs::read(minecraft.join("screenshots/personal.png")).unwrap(), b"personal");
        choices.choices.clear(); // G: use the live global selection.
        let selected = schema::shared_data::effective_global(&global, &choices, false);
        apply(&Context {
            global: &selected,
            ..context
        })
        .unwrap();
        assert!(storage::is_link(&minecraft.join("screenshots")));
        assert_eq!(fs::read(f.minecraft.join("screenshots/personal.png")).unwrap(), b"personal");
        choices.choices.insert("screenshots".into(), false);
        let selected = schema::shared_data::effective_global(&global, &choices, false);
        apply(&Context {
            global: &selected,
            ..context
        })
        .unwrap();
        fs::write(minecraft.join("screenshots/shared.png"), b"independent").unwrap();
        assert_eq!(fs::read(f.minecraft.join("screenshots/shared.png")).unwrap(), b"shared");
        assert!(global.folders.contains("screenshots"));
    }
    #[test]
    fn launcher_control_routes_are_rejected_in_portable_case() {
        for path in [
            ".pandora-layout-v1",
            ".PANDORA-SHARED-V1/state.json",
            "config/.Pandora-layout-v1",
        ] {
            assert!(validate_route(path).is_err());
        }
    }
    #[test]
    fn retired_global_worlds_materialize_before_group_attachment() {
        let f = Fixture::new();
        let ignored = IgnoredProfilePaths::default();
        let mut previous = SyncTargets::default();
        previous.folders.insert("saves".into());
        let shared = f.directories.synced_dir.join("saves");
        fs::create_dir_all(shared.join("World")).unwrap();
        fs::write(shared.join("World/level.dat"), b"old-global").unwrap();
        storage::attach_folder(&f.minecraft.join("saves"), &shared, &f.root.join("fixture-recovery")).unwrap();
        let global = SyncTargets::default();
        let mut context = f.context(&global, None, &ignored);
        context.previous = Some(&previous);
        apply(&context).unwrap();
        assert!(!storage::is_link(&f.minecraft.join("saves")));
        assert_eq!(fs::read(f.minecraft.join("saves/World/level.dat")).unwrap(), b"old-global");
        assert_eq!(fs::read(shared.join("World/level.dat")).unwrap(), b"old-global");
    }
    #[test]
    fn group_child_replaces_global_parent_without_following_it_in_preflight() {
        let f = Fixture::new();
        let ignored = IgnoredProfilePaths::default();
        let mut global = SyncTargets::default();
        global.folders.insert("config".into());
        fs::create_dir_all(f.minecraft.join("config/litematica")).unwrap();
        fs::write(f.minecraft.join("config/litematica/settings.json"), b"personal").unwrap();
        apply(&f.context(&global, None, &ignored)).unwrap();
        let mut group = Group {
            id: Uuid::new_v4(),
            name: "fixture".into(),
            targets: SyncTargets::default(),
        };
        group.targets.folders.insert("config/litematica".into());
        let context = f.context(&global, Some(&group), &ignored);
        let roots = mutation_roots(&context).unwrap();
        assert!(roots.contains(&f.directories.synced_dir.join("config")));
        apply(&context).unwrap();
        assert!(!storage::is_link(&f.minecraft.join("config")));
        assert!(storage::is_link(&f.minecraft.join("config/litematica")));
        assert_eq!(
            fs::read(f.directories.synced_dir.join("config/litematica/settings.json")).unwrap(),
            b"personal"
        );
    }
    #[test]
    fn reenabling_worlds_renames_collisions_instead_of_merging_world_contents() {
        let f = Fixture::new();
        let ignored = IgnoredProfilePaths::default();
        let global = SyncTargets::default();
        let mut group = Group {
            id: Uuid::new_v4(),
            name: "fixture".into(),
            targets: SyncTargets::default(),
        };
        group.targets.folders.insert("saves".into());
        fs::create_dir_all(f.minecraft.join("saves/World")).unwrap();
        fs::write(f.minecraft.join("saves/World/level.dat"), b"group").unwrap();
        apply(&f.context(&global, Some(&group), &ignored)).unwrap();
        group.targets.folders.clear();
        apply(&f.context(&global, Some(&group), &ignored)).unwrap();
        fs::write(f.minecraft.join("saves/World/level.dat"), b"local").unwrap();
        group.targets.folders.insert("saves".into());
        apply(&f.context(&global, Some(&group), &ignored)).unwrap();
        assert_eq!(fs::read(f.minecraft.join("saves/World/level.dat")).unwrap(), b"group");
        let versions: Vec<_> = fs::read_dir(f.minecraft.join("saves"))
            .unwrap()
            .map(|e| fs::read(e.unwrap().path().join("level.dat")).unwrap())
            .collect();
        assert_eq!(versions.len(), 2);
        assert!(versions.contains(&b"local".to_vec()));
    }
}
