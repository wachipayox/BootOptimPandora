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
        || path.split('/').any(|part| part.starts_with(".pandora-"))
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

pub(crate) fn apply(context: &Context<'_>) -> io::Result<()> {
    fs::create_dir_all(context.minecraft)?;
    let desired = plan(context)?;
    let mut receipt = read(context.root)?;
    let original = receipt.clone();
    let protected_paths = protected(context.root, context.ignored)?;
    // Adopt only known old global links; never claim an arbitrary external link.
    if let Some(previous) = context.previous {
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
            if storage::targeting(&local, &destination) {
                receipt.folders.entry(path.to_string()).or_insert(Folder {
                    route,
                    target: destination,
                });
            }
        }
    }
    if let Some(group) = context.group {
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
        if storage::targeting(&local, &destination) {
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
            // Persist intent before linking so a crash between the link and receipt write is recoverable.
            receipt.folders.insert(
                route.path.clone(),
                Folder {
                    route: route.clone(),
                    target: destination.clone(),
                },
            );
            write(context.root, &receipt)?;
            storage::attach_folder(&local, &destination, &backup(context.root, route)?)?;
        }
    }
    Ok(())
}
pub(crate) fn publish(context: &Context<'_>) -> io::Result<()> {
    let mut receipt = read(context.root)?;
    for file in receipt.files.values_mut() {
        validate_owned_target(context, &file.route, &file.target)?;
        let local = storage::checked_path(context.minecraft, &file.route.path)?;
        file.seed =
            storage::publish_file(&local, &file.target, file.seed.as_deref(), &backup(context.root, &file.route)?)?;
    }
    write(context.root, &receipt)
}
/// Called under the inherited-layout lock before applying an enforced profile delta.
/// This never writes to the shared source: it materializes a local independent copy.
pub(crate) fn prepare_profile_paths(root: &Path, paths: &[String]) -> io::Result<()> {
    let mut receipt = read(root)?;
    if receipt.folders.is_empty() && receipt.files.is_empty() {
        return Ok(());
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
    write(root, &receipt)
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
