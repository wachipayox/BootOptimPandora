//! Command-line Start gate for inherited profile updates.
//!
//! Interactive Start asks "Update and start" vs "Start without updating". A headless
//! `--run-instance*` Start cannot ask, so it must not start silently on stale files:
//! without `--update`/`--no-update` it reports the pending ancestors and exits with
//! [`EXIT_UPDATE_AVAILABLE`]. A failed check fails open, like the interactive dialog.

use bridge::message::CliUpdatePolicy;

/// Pending inherited update: the launcher printed `UPDATE-AVAILABLE` lines and did not start.
pub const EXIT_UPDATE_AVAILABLE: u8 = 3;
/// `--update` was requested and applying the update failed; the instance was not started.
pub const EXIT_UPDATE_FAILED: u8 = 4;
/// No instance has the requested name.
pub const EXIT_INSTANCE_NOT_FOUND: u8 = 5;
/// `--create-instance`: an instance with that name already exists.
pub const EXIT_NAME_EXISTS: u8 = 6;
/// `--delete-instance` (or an invalid `--create-instance` name): refused by the safety rules; nothing changed.
pub const EXIT_REFUSED: u8 = 7;
/// An instance command (create, update, delete) failed after it was accepted. Same code as a failed update.
pub const EXIT_OPERATION_FAILED: u8 = EXIT_UPDATE_FAILED;

pub const PREFIX: &str = "[WBOMBO-PROFILE]";
pub const RELAUNCH_HINT: &str = "Relaunch with --update (update then start) or --no-update (start unchanged).";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalUpdate {
    pub name: String,
    pub revision_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingUpdateKind {
    /// The pinned global Distribution revision differs from the selected/stable one.
    GlobalRevision {
        profile: String,
        current: String,
        available: String,
    },
    /// The local parent's recorded generation changed since the child last synchronized.
    LocalGeneration {
        parent: String,
        current: Option<u64>,
        available: Option<u64>,
    },
    /// Same generation, but the parent's files were edited outside the launcher.
    LocalFiles {
        parent: String,
        generation: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingInheritedUpdate {
    /// Name shown by the interactive dialog (unchanged UI text).
    pub label: String,
    /// Instance whose parent link is stale (the launched one or one of its local ancestors).
    pub via: String,
    pub kind: PendingUpdateKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InheritedUpdateReport {
    pub pending: Vec<PendingInheritedUpdate>,
    /// Pinned `(profile_id, revision_id)` of the global root ancestor, if any.
    pub global_current: Option<(String, String)>,
    /// The global check could not complete (offline, timeout); treated as "no global update".
    pub check_failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateAction {
    Start,
    UpdateThenStart,
    Exit(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDecision {
    pub lines: Vec<String>,
    pub action: GateAction,
}

fn generation(value: Option<u64>) -> String {
    value.map_or_else(|| "none".into(), |value| value.to_string())
}

fn sanitize(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

pub fn pending_line(instance: &str, update: &PendingInheritedUpdate) -> String {
    let via = if update.via.is_empty() || update.via == instance {
        String::new()
    } else {
        format!(" via={}", update.via)
    };
    let detail = match &update.kind {
        PendingUpdateKind::GlobalRevision {
            profile,
            current,
            available,
        } => {
            format!("current={profile} rev {current} available={profile} rev {available}")
        },
        PendingUpdateKind::LocalGeneration {
            parent,
            current,
            available,
        } => format!(
            "current={parent} gen {} available={parent} gen {} kind=local-parent",
            generation(*current),
            generation(*available)
        ),
        PendingUpdateKind::LocalFiles {
            parent,
            generation: value,
        } => format!(
            "current={parent} gen {g} available={parent} gen {g} kind=local-parent-files",
            g = generation(*value)
        ),
    };
    sanitize(&format!("{PREFIX} UPDATE-AVAILABLE instance={instance}{via} {detail}"))
}

fn revision_suffix(report: &InheritedUpdateReport) -> String {
    match &report.global_current {
        Some((profile, revision)) => format!("profile={profile} rev {revision}"),
        None => "rev local".into(),
    }
}

/// Decide what a command-line Start does with the result of the inherited update check.
pub fn decide(instance: &str, report: Result<&InheritedUpdateReport, &str>, policy: CliUpdatePolicy) -> GateDecision {
    let report = match report {
        Ok(report) => report,
        Err(reason) => {
            // Same as the interactive dialog: a failed check never blocks an installed instance.
            return GateDecision {
                lines: vec![sanitize(&format!(
                    "{PREFIX} UPDATE-CHECK-FAILED instance={instance} reason={reason}"
                ))],
                action: GateAction::Start,
            };
        },
    };
    let mut lines = Vec::new();
    if let Some(reason) = &report.check_failure {
        lines.push(sanitize(&format!("{PREFIX} UPDATE-CHECK-FAILED instance={instance} reason={reason}")));
    }
    if report.pending.is_empty() {
        if report.check_failure.is_none() {
            lines.push(sanitize(&format!("{PREFIX} UP-TO-DATE instance={instance} {}", revision_suffix(report))));
        }
        return GateDecision {
            lines,
            action: GateAction::Start,
        };
    }
    match policy {
        CliUpdatePolicy::Ask => {
            lines.extend(report.pending.iter().map(|update| pending_line(instance, update)));
            lines.push(RELAUNCH_HINT.into());
            GateDecision {
                lines,
                action: GateAction::Exit(EXIT_UPDATE_AVAILABLE),
            }
        },
        CliUpdatePolicy::Update => {
            lines.extend(report.pending.iter().map(|update| pending_line(instance, update)));
            lines.push(sanitize(&format!("{PREFIX} UPDATING instance={instance}")));
            GateDecision {
                lines,
                action: GateAction::UpdateThenStart,
            }
        },
        CliUpdatePolicy::NoUpdate => {
            lines.extend(
                report
                    .pending
                    .iter()
                    .map(|update| pending_line(instance, update).replacen(" UPDATE-AVAILABLE ", " UPDATE-SKIPPED ", 1)),
            );
            GateDecision {
                lines,
                action: GateAction::Start,
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global() -> PendingInheritedUpdate {
        PendingInheritedUpdate {
            label: "Wachiland Elite".into(),
            via: "Pack".into(),
            kind: PendingUpdateKind::GlobalRevision {
                profile: "wachiland-elite".into(),
                current: "r1".into(),
                available: "r2".into(),
            },
        }
    }

    fn local() -> PendingInheritedUpdate {
        PendingInheritedUpdate {
            label: "Local parent revision".into(),
            via: "Pack".into(),
            kind: PendingUpdateKind::LocalGeneration {
                parent: "Base".into(),
                current: Some(4),
                available: Some(5),
            },
        }
    }

    fn report(pending: Vec<PendingInheritedUpdate>) -> InheritedUpdateReport {
        InheritedUpdateReport {
            pending,
            global_current: Some(("wachiland-elite".into(), "r1".into())),
            check_failure: None,
        }
    }

    #[test]
    fn up_to_date_starts_and_reports_revision() {
        let decision = decide("Pack", Ok(&report(vec![])), CliUpdatePolicy::Ask);
        assert_eq!(decision.action, GateAction::Start);
        assert_eq!(
            decision.lines,
            vec!["[WBOMBO-PROFILE] UP-TO-DATE instance=Pack profile=wachiland-elite rev r1"]
        );
    }

    #[test]
    fn pending_without_flag_exits_with_distinct_code() {
        let decision = decide("Pack", Ok(&report(vec![local(), global()])), CliUpdatePolicy::Ask);
        assert_eq!(decision.action, GateAction::Exit(EXIT_UPDATE_AVAILABLE));
        assert_eq!(
            decision.lines,
            vec![
                "[WBOMBO-PROFILE] UPDATE-AVAILABLE instance=Pack current=Base gen 4 available=Base gen 5 kind=local-parent",
                "[WBOMBO-PROFILE] UPDATE-AVAILABLE instance=Pack current=wachiland-elite rev r1 available=wachiland-elite rev r2",
                RELAUNCH_HINT,
            ]
        );
    }

    #[test]
    fn ancestor_link_names_the_stale_instance() {
        let mut update = global();
        update.via = "Base".into();
        assert!(pending_line("Pack", &update).starts_with("[WBOMBO-PROFILE] UPDATE-AVAILABLE instance=Pack via=Base "));
    }

    #[test]
    fn update_flag_updates_then_starts() {
        let decision = decide("Pack", Ok(&report(vec![global()])), CliUpdatePolicy::Update);
        assert_eq!(decision.action, GateAction::UpdateThenStart);
    }

    #[test]
    fn no_update_flag_starts_and_reports_skip() {
        let decision = decide("Pack", Ok(&report(vec![global()])), CliUpdatePolicy::NoUpdate);
        assert_eq!(decision.action, GateAction::Start);
        assert!(decision.lines[0].starts_with("[WBOMBO-PROFILE] UPDATE-SKIPPED instance=Pack current="));
    }

    #[test]
    fn failed_check_fails_open() {
        let decision = decide("Pack", Err("chain unreadable"), CliUpdatePolicy::Ask);
        assert_eq!(decision.action, GateAction::Start);
        assert_eq!(
            decision.lines,
            vec!["[WBOMBO-PROFILE] UPDATE-CHECK-FAILED instance=Pack reason=chain unreadable"]
        );

        let mut offline = report(vec![]);
        offline.check_failure = Some("connection refused".into());
        let decision = decide("Pack", Ok(&offline), CliUpdatePolicy::Ask);
        assert_eq!(decision.action, GateAction::Start);
        assert_eq!(
            decision.lines,
            vec!["[WBOMBO-PROFILE] UPDATE-CHECK-FAILED instance=Pack reason=connection refused"]
        );
    }

    #[test]
    fn offline_global_still_gates_known_local_changes() {
        let mut partial = report(vec![local()]);
        partial.check_failure = Some("timed out".into());
        let decision = decide("Pack", Ok(&partial), CliUpdatePolicy::Ask);
        assert_eq!(decision.action, GateAction::Exit(EXIT_UPDATE_AVAILABLE));
        assert!(decision.lines[0].contains("UPDATE-CHECK-FAILED"));
    }
}
