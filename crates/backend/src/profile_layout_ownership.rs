//! Side-effect-light ownership reconciliation candidate for the persistent profile layout.
//!
//! This module deliberately does not perform filesystem I/O, journal/staging publication,
//! legacy restore, or Start-path selection. Callers must provide observations for only the
//! managed delta/collision paths they are already reconciling; this is not a full-tree scan.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedEntry {
    pub logical_identity: String,
    pub applied_hash: String,
}

impl ManagedEntry {
    pub fn new(logical_identity: impl Into<String>, applied_hash: impl Into<String>) -> Self {
        Self {
            logical_identity: logical_identity.into(),
            applied_hash: applied_hash.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveEntry {
    Missing,
    File { hash: String },
    Directory,
    ReparsePoint,
    OtherType,
}

impl LiveEntry {
    pub fn file(hash: impl Into<String>) -> Self {
        Self::File { hash: hash.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestinationState {
    Vacant,
    ManagedProven,
    Local,
    LocalOverride,
    Tombstone,
    UnexpectedType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileAction {
    NoOp,
    InstallManaged,
    ReplaceManaged,
    RemoveManaged,
    PreserveLocal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictReason {
    LocalCollision,
    LocalOverride,
    Tombstone,
    UnexpectedType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileDecision {
    pub state: DestinationState,
    pub action: ReconcileAction,
    pub conflict: Option<ConflictReason>,
    /// Unsafe path kinds cannot be made persistent safely without a separately proven model.
    /// They require the caller to keep/use the stock path rather than destructively guessing.
    pub stock_fallback: bool,
}

impl ReconcileDecision {
    fn clean(state: DestinationState, action: ReconcileAction) -> Self {
        Self {
            state,
            action,
            conflict: None,
            stock_fallback: false,
        }
    }

    fn conflict(state: DestinationState, reason: ConflictReason, stock_fallback: bool) -> Self {
        Self {
            state,
            action: ReconcileAction::PreserveLocal,
            conflict: Some(reason),
            stock_fallback,
        }
    }
}

/// Classify one destination without touching it.
///
/// Destructive actions are returned only when the live regular file proves the previously
/// committed `applied_hash`. A missing previously managed file is a tombstone. Local bytes are
/// always preserved when ownership cannot be proved.
pub fn classify_destination(
    previous: Option<&ManagedEntry>,
    live: &LiveEntry,
    desired: Option<&ManagedEntry>,
) -> ReconcileDecision {
    match previous {
        None => classify_without_previous(live, desired),
        Some(previous) => classify_with_previous(previous, live, desired),
    }
}

fn classify_without_previous(live: &LiveEntry, desired: Option<&ManagedEntry>) -> ReconcileDecision {
    match desired {
        None => match live {
            LiveEntry::Missing => ReconcileDecision::clean(DestinationState::Vacant, ReconcileAction::NoOp),
            _ => ReconcileDecision::clean(DestinationState::Local, ReconcileAction::PreserveLocal),
        },
        Some(_) => match live {
            LiveEntry::Missing => ReconcileDecision::clean(DestinationState::Vacant, ReconcileAction::InstallManaged),
            LiveEntry::File { .. } => {
                ReconcileDecision::conflict(DestinationState::Local, ConflictReason::LocalCollision, false)
            },
            LiveEntry::Directory | LiveEntry::ReparsePoint | LiveEntry::OtherType => {
                ReconcileDecision::conflict(DestinationState::UnexpectedType, ConflictReason::UnexpectedType, true)
            },
        },
    }
}

fn classify_with_previous(
    previous: &ManagedEntry,
    live: &LiveEntry,
    desired: Option<&ManagedEntry>,
) -> ReconcileDecision {
    match live {
        LiveEntry::Missing => match desired {
            Some(_) => ReconcileDecision::conflict(DestinationState::Tombstone, ConflictReason::Tombstone, false),
            None => ReconcileDecision::clean(DestinationState::Tombstone, ReconcileAction::PreserveLocal),
        },
        LiveEntry::File { hash } if hash == &previous.applied_hash => match desired {
            None => ReconcileDecision::clean(DestinationState::ManagedProven, ReconcileAction::RemoveManaged),
            Some(desired) if desired == previous => {
                ReconcileDecision::clean(DestinationState::ManagedProven, ReconcileAction::NoOp)
            },
            Some(_) => ReconcileDecision::clean(DestinationState::ManagedProven, ReconcileAction::ReplaceManaged),
        },
        LiveEntry::File { .. } => match desired {
            Some(_) => {
                ReconcileDecision::conflict(DestinationState::LocalOverride, ConflictReason::LocalOverride, false)
            },
            None => ReconcileDecision::clean(DestinationState::LocalOverride, ReconcileAction::PreserveLocal),
        },
        LiveEntry::Directory | LiveEntry::ReparsePoint | LiveEntry::OtherType => match desired {
            Some(_) => {
                ReconcileDecision::conflict(DestinationState::UnexpectedType, ConflictReason::UnexpectedType, true)
            },
            None => ReconcileDecision::clean(DestinationState::LocalOverride, ReconcileAction::PreserveLocal),
        },
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathReconcileInput {
    pub path: String,
    pub previous: Option<ManagedEntry>,
    pub live: LiveEntry,
    pub desired: Option<ManagedEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedPath {
    pub path: String,
    pub decision: ReconcileDecision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileOwnershipPlan {
    pub profile_id: String,
    pub paths: Vec<PlannedPath>,
    pub requires_stock_fallback: bool,
}

/// Plan only the explicitly supplied delta/collision paths for one profile.
///
/// The profile id is carried through to make accidental cross-profile application visible to
/// callers/tests. This function has no global state and cannot mutate another profile.
pub fn plan_profile(
    profile_id: impl Into<String>,
    inputs: impl IntoIterator<Item = PathReconcileInput>,
) -> ProfileOwnershipPlan {
    let mut requires_stock_fallback = false;
    let mut paths = Vec::new();

    for input in inputs {
        let decision = classify_destination(input.previous.as_ref(), &input.live, input.desired.as_ref());
        requires_stock_fallback |= decision.stock_fallback;
        paths.push(PlannedPath {
            path: input.path,
            decision,
        });
    }

    ProfileOwnershipPlan {
        profile_id: profile_id.into(),
        paths,
        requires_stock_fallback,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyRestoreStatus {
    NotNeeded,
    Restored,
    Pending,
    Failed,
    Ambiguous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationGate {
    Eligible,
    StockFallback,
}

/// Gate migration before any new persistent-layout state is claimed.
///
/// If `original_mods` exists, the only eligible state is proof that the existing stopped-instance
/// restore completed. Failed/ambiguous/pending legacy recovery stays on the stock path.
pub fn legacy_migration_gate(original_mods_exists: bool, restore_status: LegacyRestoreStatus) -> MigrationGate {
    match (original_mods_exists, restore_status) {
        (true, LegacyRestoreStatus::Restored) => MigrationGate::Eligible,
        (true, _) => MigrationGate::StockFallback,
        (false, LegacyRestoreStatus::NotNeeded | LegacyRestoreStatus::Restored) => MigrationGate::Eligible,
        (false, _) => MigrationGate::StockFallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managed(id: &str, hash: &str) -> ManagedEntry {
        ManagedEntry::new(id, hash)
    }

    fn input(
        path: &str,
        previous: Option<ManagedEntry>,
        live: LiveEntry,
        desired: Option<ManagedEntry>,
    ) -> PathReconcileInput {
        PathReconcileInput {
            path: path.to_owned(),
            previous,
            live,
            desired,
        }
    }

    #[test]
    fn update_a_vs_b_keeps_profile_boundary_explicit() {
        let a = plan_profile(
            "profile-a",
            [input(
                "mods/example.jar",
                Some(managed("example-v1", "hash-a1")),
                LiveEntry::file("hash-a1"),
                Some(managed("example-v2", "hash-a2")),
            )],
        );
        let b = plan_profile(
            "profile-b",
            [input(
                "mods/example.jar",
                Some(managed("example-v1", "hash-b1")),
                LiveEntry::file("hash-b1"),
                Some(managed("example-v1", "hash-b1")),
            )],
        );

        assert_eq!(a.profile_id, "profile-a");
        assert_eq!(a.paths[0].decision.action, ReconcileAction::ReplaceManaged);
        assert_eq!(b.profile_id, "profile-b");
        assert_eq!(b.paths[0].decision.action, ReconcileAction::NoOp);
        assert!(!b.requires_stock_fallback);
    }

    #[test]
    fn local_mod_is_preserved_and_never_claimed() {
        let decision = classify_destination(None, &LiveEntry::file("user-mod-hash"), None);

        assert_eq!(decision.state, DestinationState::Local);
        assert_eq!(decision.action, ReconcileAction::PreserveLocal);
        assert_eq!(decision.conflict, None);
    }

    #[test]
    fn local_deletion_of_managed_file_is_a_tombstone_conflict() {
        let old = managed("example-v1", "hash-v1");
        let desired = managed("example-v2", "hash-v2");
        let decision = classify_destination(Some(&old), &LiveEntry::Missing, Some(&desired));

        assert_eq!(decision.state, DestinationState::Tombstone);
        assert_eq!(decision.action, ReconcileAction::PreserveLocal);
        assert_eq!(decision.conflict, Some(ConflictReason::Tombstone));
        assert!(!decision.stock_fallback);
    }

    #[test]
    fn local_modification_is_not_overwritten_during_update() {
        let old = managed("example-v1", "hash-v1");
        let desired = managed("example-v2", "hash-v2");
        let decision = classify_destination(Some(&old), &LiveEntry::file("locally-modified"), Some(&desired));

        assert_eq!(decision.state, DestinationState::LocalOverride);
        assert_eq!(decision.action, ReconcileAction::PreserveLocal);
        assert_eq!(decision.conflict, Some(ConflictReason::LocalOverride));
    }

    #[test]
    fn new_managed_destination_collision_preserves_local_bytes() {
        let desired = managed("new-pack-mod", "managed-hash");
        let decision = classify_destination(None, &LiveEntry::file("preexisting-local"), Some(&desired));

        assert_eq!(decision.state, DestinationState::Local);
        assert_eq!(decision.action, ReconcileAction::PreserveLocal);
        assert_eq!(decision.conflict, Some(ConflictReason::LocalCollision));
    }

    #[test]
    fn managed_removal_after_local_modification_reclassifies_local() {
        let old = managed("removed-mod", "managed-hash");
        let decision = classify_destination(Some(&old), &LiveEntry::file("locally-modified"), None);

        assert_eq!(decision.state, DestinationState::LocalOverride);
        assert_eq!(decision.action, ReconcileAction::PreserveLocal);
        assert_eq!(decision.conflict, None);
    }

    #[test]
    fn proved_managed_file_can_be_removed() {
        let old = managed("removed-mod", "managed-hash");
        let decision = classify_destination(Some(&old), &LiveEntry::file("managed-hash"), None);

        assert_eq!(decision.state, DestinationState::ManagedProven);
        assert_eq!(decision.action, ReconcileAction::RemoveManaged);
        assert_eq!(decision.conflict, None);
    }

    #[test]
    fn reparse_or_unexpected_type_is_conflict_and_stock_fallback() {
        let old = managed("example-v1", "hash-v1");
        let desired = managed("example-v2", "hash-v2");

        for live in [LiveEntry::Directory, LiveEntry::ReparsePoint, LiveEntry::OtherType] {
            let decision = classify_destination(Some(&old), &live, Some(&desired));
            assert_eq!(decision.state, DestinationState::UnexpectedType);
            assert_eq!(decision.action, ReconcileAction::PreserveLocal);
            assert_eq!(decision.conflict, Some(ConflictReason::UnexpectedType));
            assert!(decision.stock_fallback);
        }
    }

    #[test]
    fn ambiguous_legacy_restore_blocks_migration() {
        assert_eq!(legacy_migration_gate(true, LegacyRestoreStatus::Ambiguous), MigrationGate::StockFallback);
        assert_eq!(legacy_migration_gate(true, LegacyRestoreStatus::Failed), MigrationGate::StockFallback);
        assert_eq!(legacy_migration_gate(true, LegacyRestoreStatus::Pending), MigrationGate::StockFallback);
        assert_eq!(legacy_migration_gate(true, LegacyRestoreStatus::Restored), MigrationGate::Eligible);
    }
}
