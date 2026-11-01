//! Read-only sharing status. Filesystem changes go through shared_data ownership receipts.
use crate::{
    BackendState,
    shared_data::{self, Context, Scope},
    shared_storage,
};
use bridge::message::{SyncState, SyncTargetState};
use schema::shared_data::contains_path;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn get_sync_state(backend: &BackendState) -> SyncState {
    let config = backend.config.lock().get().clone();
    let participants = {
        let mut state = backend.instance_state.write();
        state
            .instances
            .iter_mut()
            .filter_map(|i| {
                (!i.configuration.get().disable_file_syncing)
                    .then(|| (i.id, i.name.to_string(), i.root_path.clone(), i.dot_minecraft_path.clone()))
            })
            .collect::<Vec<_>>()
    };
    let mut targets: BTreeMap<Arc<str>, SyncTargetState> = BTreeMap::new();
    for (paths, file) in [
        (&config.sync_targets.folders, false),
        (&config.sync_targets.files, true),
    ] {
        for path in paths {
            targets.insert(
                path.clone(),
                SyncTargetState {
                    enabled: true,
                    is_file: file,
                    sync_count: 0,
                    cannot_sync_count: 0,
                    cannot_sync_instances: Vec::new(),
                },
            );
        }
    }
    for path in schema::shared_data::GLOBAL_FOLDERS {
        targets.entry((*path).into()).or_insert(SyncTargetState {
            enabled: false,
            is_file: false,
            sync_count: 0,
            cannot_sync_count: 0,
            cannot_sync_instances: Vec::new(),
        });
    }
    for (id, name, root, minecraft) in &participants {
        let group = backend.shared_group_for_instance(*id);
        let plan = group.and_then(|group| {
            shared_data::routes(&Context {
                root,
                minecraft,
                directories: &backend.directories,
                global: &config.sync_targets,
                previous: None,
                group: group.as_ref(),
                ignored: &config.ignored_profile_paths,
            })
            .map_err(|e| e.to_string())
        });
        if let Err(error) = &plan {
            log::warn!("Unable to inspect instance sharing: {error}");
        }
        for (path, status) in &mut targets {
            let local = shared_storage::checked_path(minecraft, path);
            let problem = local.as_ref().is_err()
                || plan.is_err()
                || local.as_ref().is_ok_and(|p| {
                    if status.is_file {
                        p.is_dir() || shared_storage::is_link(p)
                    } else {
                        p.is_file()
                    }
                });
            let excluded = plan.as_ref().is_ok_and(|routes| {
                routes.iter().any(|r| {
                    (contains_path(&r.path, r.is_file, path) || contains_path(path, status.is_file, &r.path))
                        && (r.conflict.is_some() || matches!(r.scope, Scope::Group { .. }))
                })
            });
            if problem || excluded {
                status.cannot_sync_instances.push(name.clone());
                continue;
            }
            if !status.enabled {
                continue;
            }
            let local = local.unwrap();
            if status.is_file {
                // Files use the committed receipt/seed and are refreshed at launch and game exit.
                if shared_data::active_shared_path(root, path).unwrap_or(false) {
                    status.sync_count += 1;
                }
            } else {
                let shared = backend.directories.synced_dir.join(path.as_ref());
                if std::fs::canonicalize(&local)
                    .ok()
                    .zip(std::fs::canonicalize(shared).ok())
                    .is_some_and(|(a, b)| a == b)
                {
                    status.sync_count += 1;
                }
            }
        }
    }
    for status in targets.values_mut() {
        status.cannot_sync_instances.sort_by_key(|name| name.to_ascii_lowercase());
        status.cannot_sync_count = status.cannot_sync_instances.len();
    }
    SyncState {
        sync_folder: backend.directories.synced_dir.clone(),
        targets,
        total_count: participants.len(),
    }
}
