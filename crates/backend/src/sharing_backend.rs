//! Backend entry points serialize data movement with instance/profile activity.
use crate::{
    BackendState,
    shared_data::{self, Context},
    shared_storage,
};
use bridge::instance::{InstanceID, InstanceStatus};
use schema::backend_config::SyncTargets;

pub(crate) struct SharedChangeGuard<'a> {
    flag: &'a std::sync::atomic::AtomicBool,
    tasks: &'a std::sync::atomic::AtomicUsize,
}
impl Drop for SharedChangeGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, std::sync::atomic::Ordering::Release);
        self.tasks.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

impl BackendState {
    pub(crate) fn begin_shared_change(&self) -> Result<SharedChangeGuard<'_>, String> {
        self.shared_data_changing
            .compare_exchange(false, true, std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Acquire)
            .map_err(|_| "No se ha podido cambiar la sincronización.".to_owned())?;
        self.launcher_tasks.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let guard = SharedChangeGuard {
            flag: &self.shared_data_changing,
            tasks: &self.launcher_tasks,
        };
        if self.launcher_update_restarting.load(std::sync::atomic::Ordering::Acquire) {
            return Err("No se ha podido cambiar la sincronización.".into());
        }
        Ok(guard)
    }

    pub(crate) fn prepare_sharing_for_launch(&self, id: InstanceID, action: &bridge::modal_action::ModalAction) {
        use bridge::modal_action::ProgressTrackerFinishType;
        let tracker = action.push_tracker("Sincronización".into());
        match self.prepare_shared_data(id) {
            Ok(()) => tracker.set_finished(ProgressTrackerFinishType::Normal),
            Err(error) => {
                log::warn!("Unable to prepare shared data before launch: {error}");
                tracker.set_finished(ProgressTrackerFinishType::Error);
                action.set_finished_with_error(
                    if error == "Detén las instancias afectadas antes de cambiar lo que comparten." {
                        error.into()
                    } else {
                        "No se ha podido cambiar la sincronización.".into()
                    },
                );
            },
        }
    }
    pub(crate) fn prepare_shared_data(&self, id: InstanceID) -> Result<(), String> {
        let _shared_lock = shared_storage::lock(&self.directories.root_launcher_dir).map_err(|e| e.to_string())?;
        let config = self.config.lock().get().clone();
        let group = self.shared_group_for_instance(id)?;
        let mut instances = self.instance_state.write();
        let mut active_roots = Vec::new();
        for other in instances
            .instances
            .iter()
            .filter(|i| i.id != id && i.status() != InstanceStatus::NotRunning)
        {
            let roots = shared_data::active_folder_roots(&other.root_path).map_err(|e| e.to_string())?;
            if roots.is_empty() {
                active_roots.push(self.directories.synced_dir.to_path_buf());
                active_roots.push(self.directories.save_groups_dir.to_path_buf());
            } else {
                active_roots.extend(roots);
            }
        }
        let instance = instances.instances.get_mut(id).ok_or("Unknown instance")?;
        if !instance.processes.is_empty() || !instance.closing_processes.is_empty() || instance.aot_create.is_some() {
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
        let settings = instance.configuration.get();
        let global = schema::shared_data::effective_global(
            &config.sync_targets,
            &settings.global_sharing,
            settings.disable_file_syncing,
        );
        let context = Context {
            root: &root,
            minecraft: &instance.dot_minecraft_path,
            directories: &self.directories,
            global: &global,
            previous: config.previous_sync_targets.as_ref(),
            group: group.as_ref(),
            ignored: &config.ignored_profile_paths,
        };
        if !active_roots.is_empty()
            && shared_data::storage_overlaps(
                &shared_data::mutation_roots(&context).map_err(|e| e.to_string())?,
                &active_roots,
            )
        {
            return Err("Detén las instancias afectadas antes de cambiar lo que comparten.".into());
        }
        let result = shared_data::apply(&context);
        if result.as_ref().is_err() || result.as_ref().is_ok_and(|changed| *changed) {
            instance.rewatch_directories(&mut self.file_watching.write());
        }
        result.map(|_| ()).map_err(|e| e.to_string())
    }

    pub(crate) fn ensure_shared_profile_isolation_safe(&self, id: InstanceID, paths: &[String]) -> Result<(), String> {
        let state = self.instance_state.read();
        let instance = state.instances.get(id).ok_or("Unknown instance")?;
        let changing = shared_data::profile_isolation_roots(&instance.root_path, paths).map_err(|e| e.to_string())?;
        if changing.is_empty() {
            return Ok(());
        }
        for other in state.instances.iter().filter(|i| i.id != id && i.status() != InstanceStatus::NotRunning) {
            let mut roots = shared_data::active_folder_roots(&other.root_path).map_err(|e| e.to_string())?;
            if roots.is_empty() {
                roots.push(self.directories.synced_dir.to_path_buf());
                roots.push(self.directories.save_groups_dir.to_path_buf());
            }
            if shared_data::storage_overlaps(&changing, &roots) {
                return Err("Detén las instancias afectadas antes de cambiar lo que comparten.".into());
            }
        }
        Ok(())
    }

    pub(crate) fn shared_routes_for_instance(&self, id: InstanceID) -> Result<Vec<shared_data::Route>, String> {
        let config = self.config.lock().get().clone();
        let group = self.shared_group_for_instance(id)?;
        let mut instances = self.instance_state.write();
        let instance = instances.instances.get_mut(id).ok_or("Unknown instance")?;
        let settings = instance.configuration.get();
        let global = schema::shared_data::effective_global(
            &config.sync_targets,
            &settings.global_sharing,
            settings.disable_file_syncing,
        );
        shared_data::routes(&Context {
            root: &instance.root_path,
            minecraft: &instance.dot_minecraft_path,
            directories: &self.directories,
            global: &global,
            previous: None,
            group: group.as_ref(),
            ignored: &config.ignored_profile_paths,
        })
        .map_err(|e| e.to_string())
    }

    pub(crate) fn instance_global_sharing(
        &self,
        id: InstanceID,
    ) -> Result<bridge::message::InstanceGlobalSharingSummary, String> {
        let global = self.config.lock().get().sync_targets.clone();
        let mut state = self.instance_state.write();
        let instance = state.instances.get_mut(id).ok_or("Unknown instance")?;
        let settings = instance.configuration.get();
        let mut configuration = settings.global_sharing.clone();
        let catalog = schema::shared_data::global_catalog(&global);
        configuration.choices.retain(|p, _| catalog.contains_key(p));
        // Represent the legacy blanket opt-out honestly in the new controls.
        if settings.disable_file_syncing {
            configuration.enabled = true;
            configuration.choices =
                schema::shared_data::global_catalog(&global).into_keys().map(|p| (p, false)).collect();
        }
        Ok(bridge::message::InstanceGlobalSharingSummary { global, configuration })
    }
    pub(crate) fn set_instance_global_sharing(
        &self,
        id: InstanceID,
        configuration: schema::shared_data::InstanceGlobalSharing,
    ) -> Result<(), String> {
        let _change = self.begin_shared_change()?;
        let global = self.config.lock().get().sync_targets.clone();
        let catalog = schema::shared_data::global_catalog(&global);
        if configuration.choices.keys().any(|p| !catalog.contains_key(p)) {
            return Err("No se ha podido cambiar la sincronización.".into());
        }
        {
            let mut state = self.instance_state.write();
            let instance = state.instances.get_mut(id).ok_or("Unknown instance")?;
            if instance.status() != InstanceStatus::NotRunning
                || instance.launch_keepalive.as_ref().is_some_and(|k| k.is_alive())
            {
                return Err("Detén las instancias afectadas antes de cambiar lo que comparten.".into());
            }
            instance.configuration.modify(|c| {
                c.global_sharing = configuration;
                c.disable_file_syncing = false;
            });
        }
        self.prepare_shared_data(id)
    }

    pub(crate) fn set_shared_global(&self, target: String, is_file: bool, enabled: bool) -> Result<(), String> {
        let _change = self.begin_shared_change()?;
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
    active_roots: &[std::path::PathBuf],
) -> Result<(), String> {
    let _shared_lock = shared_storage::lock(&directories.root_launcher_dir).map_err(|e| e.to_string())?;
    if shared_data::storage_overlaps(
        &shared_data::file_storage_roots(&instance.root_path).map_err(|e| e.to_string())?,
        active_roots,
    ) {
        return Err("Detén las instancias afectadas antes de cambiar lo que comparten.".into());
    }
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
