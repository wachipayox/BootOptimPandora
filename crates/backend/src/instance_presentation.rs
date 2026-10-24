//! Lightweight, local instance presentation. No tree walk or file hashing.
use crate::{
    BackendState,
    profile_branch::{ProfileBranchManifest, ProfileEntryOrigin, ProfileEntryOwnership, ProfileParentRef},
};
use bridge::instance::{InstanceID, InstanceProfileInfo};

fn snapshot(branch: Option<ProfileBranchManifest>, local_parent_name: Option<String>) -> InstanceProfileInfo {
    let Some(branch) = branch else {
        return InstanceProfileInfo::default();
    };
    let pin = branch.applied_revision.as_ref().or(branch.lineage.global_ancestor.as_ref());
    let mut info = InstanceProfileInfo {
        global_profile_id: pin.map(|p| p.profile_id.clone()),
        global_revision_id: pin.map(|p| p.revision_id.clone()),
        local_parent_name,
        ..Default::default()
    };
    for (path, entry) in branch.entries {
        if entry.ownership == ProfileEntryOwnership::Inherited {
            info.inherited_paths.insert(path.clone());
            if matches!(entry.origin, ProfileEntryOrigin::GlobalRevision { .. }) {
                info.global_paths.insert(path);
            }
        }
    }
    info
}
impl BackendState {
    pub(crate) fn instance_profile_info(&self, id: InstanceID) -> Result<InstanceProfileInfo, String> {
        let root = self
            .instance_state
            .read()
            .instances
            .get(id)
            .ok_or("Instance unavailable")?
            .root_path
            .clone();
        let branch = crate::profile_layout_flow::read_committed_branch(&root).map_err(|e| e.to_string())?;
        let local_parent_name = branch
            .as_ref()
            .and_then(|b| match b.lineage.parent.as_ref() {
                Some(ProfileParentRef::LocalProfile { profile_uuid }) => {
                    self.find_instance_id_for_profile_uuid(*profile_uuid)
                },
                _ => None,
            })
            .and_then(|id| self.instance_state.read().instances.get(id).map(|i| i.name.to_string()));
        Ok(snapshot(branch, local_parent_name))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_branch::{GlobalRevisionPin, ProfileEntryMetadata, ProfileFilePolicy, ProfileLineage};
    #[test]
    fn pure_local_has_no_global_history() {
        let info = snapshot(None, None);
        assert!(info.global_profile_id.is_none());
        let info = snapshot(Some(ProfileBranchManifest::default()), Some("Local parent".into()));
        assert!(info.global_profile_id.is_none());
        assert_eq!(info.local_parent_name.as_deref(), Some("Local parent"));
    }
    #[test]
    fn metadata_distinguishes_inherited_files_from_local_overrides() {
        let pin = GlobalRevisionPin::new("profile_example", "rev_example", "0".repeat(64)).unwrap();
        let inherited = ProfileEntryMetadata {
            logical_identity: "example".into(),
            source_sha256: "0".repeat(64),
            source_size_bytes: 0,
            source_modified_unix_nanos: 0,
            origin: ProfileEntryOrigin::GlobalRevision { pin: pin.clone() },
            ownership: ProfileEntryOwnership::Inherited,
            policy: ProfileFilePolicy::Enforced,
        };
        let mut branch = ProfileBranchManifest {
            lineage: ProfileLineage::from_global(pin.clone()).unwrap(),
            applied_revision: Some(pin),
            ..Default::default()
        };
        branch.entries.insert("mods/inherited.jar".into(), inherited.clone());
        branch.entries.insert(
            "mods/local.jar".into(),
            ProfileEntryMetadata {
                ownership: ProfileEntryOwnership::Local,
                ..inherited
            },
        );
        let info = snapshot(Some(branch), None);
        assert_eq!(info.global_profile_id.as_deref(), Some("profile_example"));
        assert_eq!(info.global_revision_id.as_deref(), Some("rev_example"));
        assert_eq!(info.inherited_paths, ["mods/inherited.jar".to_owned()].into());
        assert_eq!(info.global_paths, info.inherited_paths);
    }
}
