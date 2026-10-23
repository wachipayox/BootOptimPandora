//! Command-line instance management: list, info, create, update and delete.
//!
//! Every command goes through the backend paths the GUI uses (`create_global_profile_instance`,
//! `update_inherited_chain`, the instance folder removal of `DeleteInstance`). Nothing here writes instance
//! files by hand, so the signed pin, the lineage, the identity lock and the save groups are all handled by the
//! same code as the carousel. Results are lines with the prefix `[WBOMBO-INSTANCE]` and an exit code, the same
//! way the `[WBOMBO-PROFILE]` lines of a command-line Start are reported (see [`crate::cli_update_gate`]).
//!
//! Deleting is deliberately narrow: only instances that `--create-instance` made (`created_by_cli` in
//! `info_v1.json`) and instances of `profile_test-…` profiles. An agent cannot delete the player's own instance.

use std::{path::Path, sync::Arc};

use bridge::{
    instance::InstanceID,
    message::{CliInstanceCommand, CliRevision, CliStartOutcome},
    modal_action::{ModalAction, ProgressTrackerFinishType},
    profile_family::is_test_profile,
};
use serde::Serialize;

use crate::{
    BackendState,
    cli_update_gate::{EXIT_INSTANCE_NOT_FOUND, EXIT_NAME_EXISTS, EXIT_OPERATION_FAILED, EXIT_REFUSED},
    distribution::{DistributionClient, GlobalProfileSummary, RevisionRef, selected_revision},
    profile_branch::ProfileParentRef,
};

pub const PREFIX: &str = "[WBOMBO-INSTANCE]";

/// What one instance looks like to the command line. `sequence` is only filled by `--instance-info`, which
/// asks the server for it: the instance stores the revision id and digest, not the sequence number.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InstanceReport {
    pub name: String,
    pub profile: Option<String>,
    pub revision: Option<String>,
    pub manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<i64>,
    pub loader: String,
    pub minecraft: String,
    pub minecraft_dir: String,
    pub created_by_cli: bool,
}

pub async fn run(backend: &Arc<BackendState>, command: CliInstanceCommand) -> CliStartOutcome {
    let outcome = match command {
        CliInstanceCommand::List { json } => list(backend, json),
        CliInstanceCommand::Info { name, json } => info(backend, &name, json).await,
        CliInstanceCommand::Create {
            name,
            profile_id,
            revision,
        } => create(backend, &name, &profile_id, &revision).await,
        CliInstanceCommand::Update { name, revision } => update(backend, &name, &revision).await,
        CliInstanceCommand::Delete { name, confirm } => delete(backend, &name, &confirm),
    };
    for line in &outcome.lines {
        log::info!("{line}");
    }
    outcome
}

fn done(lines: Vec<String>) -> CliStartOutcome {
    CliStartOutcome { lines, exit_code: 0 }
}

fn stop(exit_code: u8, line: String) -> CliStartOutcome {
    CliStartOutcome {
        lines: vec![line],
        exit_code,
    }
}

fn text(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn not_found(name: &str) -> CliStartOutcome {
    stop(EXIT_INSTANCE_NOT_FOUND, format!("{PREFIX} INSTANCE-NOT-FOUND name={}", text(name)))
}

fn failed(verb: &str, name: &str, reason: &str) -> CliStartOutcome {
    stop(
        EXIT_OPERATION_FAILED,
        format!("{PREFIX} {verb} instance={} reason={}", text(name), text(reason)),
    )
}

fn refused(name: &str, reason: &str) -> CliStartOutcome {
    stop(EXIT_REFUSED, format!("{PREFIX} REFUSED instance={} reason={}", text(name), reason))
}

// ---- rules ------------------------------------------------------------------------------------------

/// The delete rule. `profile_id` is the global profile the instance descends from, if any.
/// `confirm` must repeat the exact name, so a stray command cannot delete by accident.
pub(crate) fn delete_verdict(
    name: &str,
    confirm: &str,
    created_by_cli: bool,
    profile_id: Option<&str>,
) -> Result<(), &'static str> {
    if confirm != name {
        return Err("confirm-does-not-match");
    }
    if created_by_cli || profile_id.is_some_and(is_test_profile) {
        Ok(())
    } else {
        Err("not-created-by-cli-nor-test-profile")
    }
}

/// The name must be one folder name that the launcher would not rewrite (it silently renames to `name (1)`
/// when the sanitised name is taken), so the instance created is the one that was asked for.
pub(crate) fn name_is_usable(name: &str) -> bool {
    crate::fs::is_single_component_path_str(name)
        && sanitize_filename::is_sanitized_with_options(
            name,
            sanitize_filename::OptionsForCheck {
                windows: true,
                ..Default::default()
            },
        )
}

/// The revision of `profile` that a selector names, when the catalog already says which it is.
pub(crate) fn known_revision(profile: &GlobalProfileSummary, selector: &CliRevision) -> Option<RevisionRef> {
    match selector {
        CliRevision::Selected => Some(selected_revision(profile).clone()),
        CliRevision::Latest => Some(profile.latest_revision.clone()),
        CliRevision::Id(id) => std::iter::once(&profile.latest_revision)
            .chain(profile.channels.iter().map(|channel| &channel.revision))
            .find(|revision| &revision.revision_id == id)
            .cloned(),
    }
}

async fn resolve_revision(
    client: &DistributionClient,
    profile: &GlobalProfileSummary,
    selector: &CliRevision,
) -> Result<RevisionRef, String> {
    if let Some(known) = known_revision(profile, selector) {
        return Ok(known);
    }
    let CliRevision::Id(id) = selector else {
        return Err("revision could not be resolved".into());
    };
    client
        .lookup_revision_ref(&profile.profile_id, id)
        .await
        .map_err(|error| error.to_string())
}

// ---- finding instances ------------------------------------------------------------------------------

struct Found {
    id: InstanceID,
    name: String,
    root: Arc<Path>,
    dot_minecraft: Arc<Path>,
    configuration: schema::instance::InstanceConfiguration,
    busy: bool,
}

/// Same lookup as the command-line Start: the exact name, then a case-insensitive match.
fn find(backend: &BackendState, name: &str) -> Option<Found> {
    // `Persistent::get` needs `&mut`, so the lookup holds the write lock for a moment.
    let mut state = backend.instance_state.write();
    let mut found = None;
    for instance in state.instances.iter_mut() {
        if instance.name == name {
            found = Some(instance);
            break;
        } else if instance.name.eq_ignore_ascii_case(name) {
            found = Some(instance);
        }
    }
    found.map(|instance| Found {
        id: instance.id,
        name: instance.name.to_string(),
        root: instance.root_path.clone(),
        dot_minecraft: instance.dot_minecraft_path.clone(),
        configuration: instance.configuration.get().clone(),
        busy: !instance.processes.is_empty()
            || instance.launch_keepalive.as_ref().is_some_and(|keepalive| keepalive.is_alive())
            || bridge::inherited_operations::is_active(instance.id),
    })
}

fn all_names(backend: &BackendState) -> Vec<String> {
    let mut names: Vec<String> = backend.instance_state.read().instances.iter().map(|i| i.name.to_string()).collect();
    names.sort_by_key(|name| name.to_lowercase());
    names
}

/// The profile the instance descends from and the revision applied to it, from the local branch state only.
/// `None`s for a local-only or unreadable instance; nothing contacts the server.
fn profile_facts(backend: &BackendState, id: InstanceID) -> (Option<String>, Option<(String, String)>) {
    match backend.persistent_profile_branch_status(id, None) {
        Ok(snapshot) => (
            snapshot.branch.lineage.global_ancestor.as_ref().map(|pin| pin.profile_id.clone()),
            snapshot.branch.applied_revision.map(|pin| (pin.revision_id, pin.manifest_sha256)),
        ),
        Err(_) => (None, None),
    }
}

fn report(backend: &BackendState, found: &Found) -> InstanceReport {
    let (profile, revision) = profile_facts(backend, found.id);
    InstanceReport {
        name: found.name.clone(),
        profile,
        manifest_sha256: revision.as_ref().map(|(_, digest)| digest.clone()),
        revision: revision.map(|(id, _)| id),
        sequence: None,
        loader: found.configuration.loader.pretty_name().to_owned(),
        minecraft: found.configuration.minecraft_version.to_string(),
        minecraft_dir: found.dot_minecraft.to_string_lossy().into_owned(),
        created_by_cli: found.configuration.created_by_cli,
    }
}

pub(crate) fn report_line(report: &InstanceReport) -> String {
    format!(
        "{PREFIX} INSTANCE name={} profile={} revision={} sequence={} loader={} minecraft={} created_by_cli={} minecraft_dir={}",
        text(&report.name),
        report.profile.as_deref().unwrap_or("none"),
        report.revision.as_deref().unwrap_or("none"),
        report.sequence.map_or_else(|| "unknown".to_owned(), |sequence| sequence.to_string()),
        report.loader,
        report.minecraft,
        report.created_by_cli,
        text(&report.minecraft_dir),
    )
}

// ---- commands ---------------------------------------------------------------------------------------

fn list(backend: &Arc<BackendState>, json: bool) -> CliStartOutcome {
    let reports: Vec<InstanceReport> = all_names(backend)
        .iter()
        .filter_map(|name| find(backend, name))
        .map(|found| report(backend, &found))
        .collect();
    if json {
        return done(vec![serde_json::to_string(&reports).unwrap_or_else(|_| "[]".into())]);
    }
    let mut lines: Vec<String> = reports.iter().map(report_line).collect();
    lines.push(format!("{PREFIX} LISTED count={}", reports.len()));
    done(lines)
}

async fn info(backend: &Arc<BackendState>, name: &str, json: bool) -> CliStartOutcome {
    let Some(found) = find(backend, name) else {
        return not_found(name);
    };
    let mut report = report(backend, &found);
    // The sequence number is not stored in the instance; ask the server when it is reachable.
    if let (Some(profile), Some(revision)) = (&report.profile, &report.revision) {
        let config = backend.config.lock().get().distribution.clone();
        if let Ok(client) = DistributionClient::new(&config)
            && let Ok(Ok(found)) =
                tokio::time::timeout(std::time::Duration::from_secs(15), client.lookup_revision_ref(profile, revision))
                    .await
        {
            report.sequence = Some(found.sequence);
        }
    }
    if json {
        return done(vec![serde_json::to_string(&report).unwrap_or_else(|_| "{}".into())]);
    }
    done(vec![report_line(&report)])
}

async fn create(backend: &Arc<BackendState>, name: &str, profile_id: &str, revision: &CliRevision) -> CliStartOutcome {
    if !name_is_usable(name) {
        return refused(name, "invalid-instance-name");
    }
    if find(backend, name).is_some() {
        return stop(EXIT_NAME_EXISTS, format!("{PREFIX} EXISTS instance={}", text(name)));
    }
    let config = backend.config.lock().get().distribution.clone();
    let client = match DistributionClient::new(&config) {
        Ok(client) => client,
        Err(error) => return failed("FAILED", name, &error.to_string()),
    };
    // Test profiles are hidden from the normal catalog, whatever the launcher channel is.
    let catalog = match client.list_profiles_with_tests(true).await {
        Ok(catalog) => catalog,
        Err(error) => return failed("FAILED", name, &format!("catalog: {error}")),
    };
    let Some(profile) = catalog.iter().find(|profile| profile.profile_id == profile_id) else {
        return failed("FAILED", name, "profile-not-published");
    };
    let target = match resolve_revision(&client, profile, revision).await {
        Ok(target) => target,
        Err(error) => return failed("FAILED", name, &format!("revision: {error}")),
    };

    let started = std::time::Instant::now();
    let modal_action = ModalAction::default();
    let progress = modal_action.push_tracker("Preparing modpack assets".into());
    progress.set_total(1);
    match backend
        .create_global_profile_instance(name, profile_id, &target, &modal_action, &progress, None, true)
        .await
    {
        Ok(()) => {
            progress.set_finished(ProgressTrackerFinishType::Normal);
            modal_action.set_finished();
            done(vec![
                format!(
                    "{PREFIX} CREATING instance={} profile={profile_id} rev {} sequence={}",
                    text(name),
                    target.revision_id,
                    target.sequence
                ),
                format!(
                    "{PREFIX} CREATED instance={} profile={profile_id} rev {} seconds={}",
                    text(name),
                    target.revision_id,
                    started.elapsed().as_secs()
                ),
            ])
        },
        Err(error) => {
            progress.set_finished(ProgressTrackerFinishType::Error);
            modal_action.set_finished_with_error(error.clone().into());
            failed("CREATE-FAILED", name, &error)
        },
    }
}

async fn update(backend: &Arc<BackendState>, name: &str, revision: &CliRevision) -> CliStartOutcome {
    let Some(found) = find(backend, name) else {
        return not_found(name);
    };
    let display = found.name.clone();
    if found.busy {
        return failed("UPDATE-FAILED", &display, "busy");
    }
    if !bridge::inherited_operations::try_begin(found.id) {
        return failed("UPDATE-FAILED", &display, "busy");
    }
    let result = update_locked(backend, &found, revision).await;
    bridge::inherited_operations::finish(found.id);
    result
}

async fn update_locked(backend: &Arc<BackendState>, found: &Found, revision: &CliRevision) -> CliStartOutcome {
    let display = &found.name;
    let (profile, before) = profile_facts(backend, found.id);
    let before = before.map(|(revision, _)| revision);
    let modal_action = ModalAction::default();
    let changed = if *revision == CliRevision::Selected {
        // The same rules as "Update and start": every stale inherited ancestor, then this instance.
        let report = match backend.check_inherited_update_report(found.id).await {
            Ok(report) => report,
            Err(error) => return failed("UPDATE-FAILED", display, &error),
        };
        if report.pending.is_empty() {
            false
        } else {
            match backend.update_inherited_chain(found.id, &modal_action).await {
                Ok(()) => true,
                Err(error) => {
                    modal_action.set_finished_with_error(error.clone().into());
                    return failed("UPDATE-FAILED", display, &error);
                },
            }
        }
    } else {
        // A chosen revision applies to the direct global parent, as a pinned update does.
        let is_global_child = matches!(
            backend
                .persistent_profile_branch_status(found.id, None)
                .map(|snapshot| snapshot.branch.lineage.parent),
            Ok(Some(ProfileParentRef::GlobalRevision { .. }))
        );
        let Some(profile) = profile.filter(|_| is_global_child) else {
            return failed("UPDATE-FAILED", display, "revision-needs-a-direct-global-profile-child");
        };
        let config = backend.config.lock().get().distribution.clone();
        let client = match DistributionClient::new(&config) {
            Ok(client) => client,
            Err(error) => return failed("UPDATE-FAILED", display, &error.to_string()),
        };
        let catalog = match client.list_profiles_with_tests(true).await {
            Ok(catalog) => catalog,
            Err(error) => return failed("UPDATE-FAILED", display, &format!("catalog: {error}")),
        };
        let Some(summary) = catalog.iter().find(|candidate| candidate.profile_id == profile) else {
            return failed("UPDATE-FAILED", display, "profile-not-published");
        };
        let target = match resolve_revision(&client, summary, revision).await {
            Ok(target) => target,
            Err(error) => return failed("UPDATE-FAILED", display, &format!("revision: {error}")),
        };
        let progress = modal_action.push_tracker("Updating inherited files".into());
        match backend
            .update_global_profile_instance_to(found.id, Some((&modal_action, &progress)), Some(target))
            .await
        {
            Ok(changed) => changed,
            Err(error) => {
                modal_action.set_finished_with_error(error.clone().into());
                return failed("UPDATE-FAILED", display, &error);
            },
        }
    };
    modal_action.set_finished();
    let (profile_now, after) = profile_facts(backend, found.id);
    let after = after.map(|(revision, _)| revision);
    let profile_now = profile_now.unwrap_or_else(|| "none".into());
    let state = if changed { "UPDATED" } else { "UP-TO-DATE" };
    done(vec![format!(
        "{PREFIX} {state} instance={} profile={profile_now} from rev {} to rev {}",
        text(display),
        before.as_deref().unwrap_or("none"),
        after.as_deref().unwrap_or("none"),
    )])
}

fn delete(backend: &Arc<BackendState>, name: &str, confirm: &str) -> CliStartOutcome {
    let Some(found) = find(backend, name) else {
        return not_found(name);
    };
    let display = found.name.clone();
    // The profile is read only when the instance was not made by the command line: the instance of a player
    // is never touched by this check beyond reading its local state.
    let profile = if found.configuration.created_by_cli {
        None
    } else {
        profile_facts(backend, found.id).0
    };
    if let Err(reason) = delete_verdict(&display, confirm, found.configuration.created_by_cli, profile.as_deref()) {
        return refused(&display, reason);
    }
    if found.busy {
        return failed("DELETE-FAILED", &display, "busy");
    }
    if found.root.parent() != Some(&*backend.directories.instances_dir) {
        return refused(&display, "not-in-the-instances-folder");
    }
    if let Err(error) = std::fs::remove_dir_all(&found.root) {
        return failed("DELETE-FAILED", &display, &error.to_string());
    }
    backend.remove_instance(found.id);
    done(vec![format!("{PREFIX} DELETED instance={}", text(&display))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distribution::ChannelRef;

    #[test]
    fn delete_is_allowed_for_cli_instances_and_test_profiles_only() {
        assert_eq!(delete_verdict("a", "a", true, None), Ok(()));
        assert_eq!(delete_verdict("a", "a", false, Some("profile_test-269-cli-smoke")), Ok(()));
        // The player's own instance of a real profile, or a purely local one, is refused.
        assert_eq!(
            delete_verdict("a", "a", false, Some("profile_wachiland-elite-beta")),
            Err("not-created-by-cli-nor-test-profile")
        );
        assert_eq!(delete_verdict("a", "a", false, None), Err("not-created-by-cli-nor-test-profile"));
    }

    #[test]
    fn delete_needs_the_exact_name_repeated() {
        assert_eq!(delete_verdict("Pack", "pack", true, None), Err("confirm-does-not-match"));
        assert_eq!(delete_verdict("Pack", "", true, None), Err("confirm-does-not-match"));
        assert_eq!(delete_verdict("Pack", "Pack", true, None), Ok(()));
    }

    #[test]
    fn instance_names_must_be_one_clean_folder_name() {
        assert!(name_is_usable("agente-269-cli-smoke"));
        assert!(name_is_usable("Wachiland Elite BETA"));
        for bad in ["", "a/b", "a\\b", "..", "con", "a:b", "trailing."] {
            assert!(!name_is_usable(bad), "{bad:?}");
        }
    }

    fn summary() -> GlobalProfileSummary {
        let revision = |id: &str, sequence: i64| RevisionRef {
            revision_id: id.into(),
            sequence,
            manifest_sha256: "a".repeat(64),
        };
        GlobalProfileSummary {
            description: String::new(),
            icon: None,
            profile_id: "profile_test-1".into(),
            name: "Test".into(),
            latest_revision: revision("rev_latest", 9),
            channels: vec![ChannelRef {
                name: "stable".into(),
                revision: revision("rev_stable", 7),
            }],
        }
    }

    #[test]
    fn revision_selectors_resolve_against_the_catalog() {
        let profile = summary();
        let id = |selector: &CliRevision| known_revision(&profile, selector).map(|revision| revision.revision_id);
        assert_eq!(id(&CliRevision::Selected).as_deref(), Some("rev_stable"));
        assert_eq!(id(&CliRevision::Latest).as_deref(), Some("rev_latest"));
        assert_eq!(id(&CliRevision::Id("rev_stable".into())).as_deref(), Some("rev_stable"));
        assert_eq!(id(&CliRevision::Id("rev_latest".into())).as_deref(), Some("rev_latest"));
        // An older revision is not in the catalog: it has to be looked up and verified.
        assert_eq!(id(&CliRevision::Id("rev_old".into())), None);
    }

    #[test]
    fn a_profile_without_a_stable_channel_selects_its_latest_revision() {
        let mut profile = summary();
        profile.channels.clear();
        assert_eq!(
            known_revision(&profile, &CliRevision::Selected)
                .map(|revision| revision.revision_id)
                .as_deref(),
            Some("rev_latest")
        );
    }

    fn sample() -> InstanceReport {
        InstanceReport {
            name: "agente-269-cli-smoke".into(),
            profile: Some("profile_wachiland-elite-beta".into()),
            revision: Some("rev_1".into()),
            manifest_sha256: Some("b".repeat(64)),
            sequence: None,
            loader: "NeoForge".into(),
            minecraft: "1.21.1".into(),
            minecraft_dir: "C:/x/.minecraft".into(),
            created_by_cli: true,
        }
    }

    #[test]
    fn json_report_has_stable_field_names_and_leaves_out_an_unknown_sequence() {
        let value: serde_json::Value = serde_json::to_value(sample()).unwrap();
        assert_eq!(value["name"], "agente-269-cli-smoke");
        assert_eq!(value["profile"], "profile_wachiland-elite-beta");
        assert_eq!(value["revision"], "rev_1");
        assert_eq!(value["created_by_cli"], true);
        assert_eq!(value["minecraft"], "1.21.1");
        assert!(value.get("sequence").is_none());
        let mut with_sequence = sample();
        with_sequence.sequence = Some(4);
        assert_eq!(serde_json::to_value(with_sequence).unwrap()["sequence"], 4);
    }

    #[test]
    fn report_line_is_one_line_with_the_prefix() {
        let mut report = sample();
        report.name = "multi\nline".into();
        let line = report_line(&report);
        assert!(line.starts_with("[WBOMBO-INSTANCE] INSTANCE name=multi line profile=profile_wachiland-elite-beta "));
        assert!(!line.contains('\n'));
        assert!(line.contains("sequence=unknown"));
        assert!(line.contains("created_by_cli=true"));
    }
}
