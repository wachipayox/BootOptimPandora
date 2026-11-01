//! Backend entry points serialize data movement with instance/profile activity.
use crate::{
    BackendState,
    shared_data::{self, Context},
    shared_storage,
};
use bridge::instance::{InstanceID, InstanceStatus};
use schema::backend_config::SyncTargets;

impl BackendState {
    pub(crate) fn prepare_shared_data(&self, id: InstanceID) -> Result<(), String> {
        let _shared_lock = shared_storage::lock(&self.directories.root_launcher_dir).map_err(|e| e.to_string())?;
        let config = self.config.lock().get().clone();
        let group = self.shared_group_for_instance(id)?;
        let mut instances = self.instance_state.write();
        let instance = instances.instances.get_mut(id).ok_or("Unknown instance")?;
        if !instance.processes.is_empty() || !instance.closing_processes.is_empty() {
            return Err("Detén las instancias afectadas antes de cambiar lo que comparten.".into());
        }
        let root = instance.root_path.clone();
        let _profile_lock = if root.join(".pandora-layout-v1").exists() {
            let guard =
                crate::profile_layout_identity::acquire_or_initialize_profile_lock(&root).map_err(|e| e.to_string())?;
            let _ = crate::profile_layout_flow::PersistentProfileLayout::open(&root).map_err(|e| e.to_string())?;
            Some(guard)
        } else {
            None
        };
        let disabled = instance.configuration.get().disable_file_syncing;
        let empty = SyncTargets::default();
        shared_data::apply(&Context {
            root: &root,
            minecraft: &instance.dot_minecraft_path,
            directories: &self.directories,
            global: if disabled { &empty } else { &config.sync_targets },
            previous: config.previous_sync_targets.as_ref(),
            group: group.as_ref(),
            ignored: &config.ignored_profile_paths,
        })
        .map_err(|e| e.to_string())
    }

    pub(crate) fn shared_routes_for_instance(&self, id: InstanceID) -> Result<Vec<shared_data::Route>, String> {
        let config = self.config.lock().get().clone();
        let group = self.shared_group_for_instance(id)?;
        let mut instances = self.instance_state.write();
        let instance = instances.instances.get_mut(id).ok_or("Unknown instance")?;
        let empty = SyncTargets::default();
        let disabled = instance.configuration.get().disable_file_syncing;
        shared_data::routes(&Context {
            root: &instance.root_path,
            minecraft: &instance.dot_minecraft_path,
            directories: &self.directories,
            global: if disabled { &empty } else { &config.sync_targets },
            previous: None,
            group: group.as_ref(),
            ignored: &config.ignored_profile_paths,
        })
        .map_err(|e| e.to_string())
    }

    pub(crate) fn set_shared_global(&self, target: String, is_file: bool, enabled: bool) -> Result<(), String> {
        shared_data::validate_route(&target).map_err(|e| e.to_string())?;
        let ids = {
            let state = self.instance_state.read();
            if state.instances.iter().any(|i| {
                i.status() != InstanceStatus::NotRunning
                    || i.launch_keepalive.as_ref().is_some_and(|keepalive| keepalive.is_alive())
            }) {
                return Err("Detén las instancias afectadas antes de cambiar lo que comparten.".into());
            }
            state.instances.iter().map(|i| i.id).collect::<Vec<_>>()
        };
        self.config.lock().modify(|config| {
            let (selected, other) = if is_file {
                (&mut config.sync_targets.files, &mut config.sync_targets.folders)
            } else {
                (&mut config.sync_targets.folders, &mut config.sync_targets.files)
            };
            other.remove(target.as_str());
            if enabled {
                selected.insert(target.clone().into());
            } else {
                selected.remove(target.as_str());
            }
        });
        for id in ids {
            self.prepare_shared_data(id)?;
        }
        Ok(())
    }
}

/// Called after game exit while the instance-state guard still prevents another launch.
pub(crate) fn publish_finished(
    instance: &crate::instance::Instance,
    directories: &crate::directories::LauncherDirectories,
) -> Result<(), String> {
    let _shared_lock = shared_storage::lock(&directories.root_launcher_dir).map_err(|e| e.to_string())?;
    let empty = SyncTargets::default();
    let ignored = schema::ignored_profile_paths::IgnoredProfilePaths::default();
    shared_data::publish(&Context {
        root: &instance.root_path,
        minecraft: &instance.dot_minecraft_path,
        directories,
        global: &empty,
        previous: None,
        group: None,
        ignored: &ignored,
    })
    .map_err(|e| e.to_string())
}
