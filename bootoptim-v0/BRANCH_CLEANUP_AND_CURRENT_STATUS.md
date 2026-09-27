# Launcher branch cleanup and current status

Checked on 2026-09-28 against `agent/integration-current` at
`bb8273aabd8c743dc0b4081e28a02ce58752450c`.

## Integrated launcher work

The following work is already part of the integration branch and should not be
reintroduced from old topic branches:

- PR #64: launch-fast library policy and default asset USN cache.
- PR #66: CLI Normal Start and a persistent, writable `.minecraft` per
  instance; no per-launch Mods rebuild or copy.
- PRs #67/#68: persistent profile identity and lineage, ownership/recovery
  transaction, delta reconciliation, and the Repair modpack backend.
- PR #69: verified global profile catalog/revision client and initial global
  instance creation.
- PR #70: incremental updates for direct global instances.
- PR #71: Wachiland Launcher branding/data location and removal of unused
  sandbox/file-sync controls.
- PR #72: HTTPS connection probe independent of local signing-key setup.

The launcher profile flow still needs runtime validation against the configured
Distribution server. Local-child creation, editing local overlays, lineage UI,
update history/progress, and Repair modpack UI remain future work; see
`ROADMAP.md`.

## Superseded profile-layout PR chain

PRs #32–#39 form an old stacked candidate chain. They target obsolete draft
branches rather than `agent/integration-current`; PRs #67/#68 recomposed and
integrated the production implementation. PRs #32–#39 are now closed and their
exact remote head refs have been deleted to prevent accidental merges. Their
source and CI history remain available in GitHub PR records.

| PR | Useful evidence retained | Why its branch is obsolete |
| --- | --- | --- |
| #32 | Architecture for per-profile identity, managed/local ownership, delta-only reconciliation, conflict retention, journaled publication, recovery, and fail-open handling. The current architecture/client-flow documents retain the applicable design. | Design-only draft stacked on old prelaunch work; #67/#68 are the integrated implementation. |
| #33 | Focused ownership planner passed 9 tests covering profile isolation, overrides, tombstones, collisions, removals, reparse objects, and ambiguous legacy restore. | Standalone classifier candidate; ownership logic was recomposed into the integrated backend. |
| #34 | Persistent state/journal backend passed 7 focused tests for identity, staging, publication, and recovery. | Backend-only candidate on the obsolete #32 base; superseded by #67/#68. |
| #35 | Signed private Distribution protocol/design and publication boundary. | Architecture-only child of the old stack; current Distribution repository docs and launcher roadmap are authoritative. |
| #36 | Composed stopped-profile transaction flow passed 19 focused tests, including crash recovery and staged-payload validation. | Old client-flow candidate; superseded by integrated reconciliation in #67/#68. |
| #37 | Clone identity reminting and OS-backed per-profile lock candidate. | Old implementation stack, not the source of the integrated identity/locking behavior. |
| #38 | Native validation found Windows `sync_parent` failing with access denied; this is an important portability failure to retain when reviewing directory durability. The candidate gate was not fully green. | Its Windows gate failed on its recorded final run, and it applies to the superseded #37 implementation. |
| #39 | Follow-up Windows/macOS native lock probes completed successfully for that candidate implementation. | A narrow verification follow-up to #37/#38, not independently useful after #67/#68 replaced the implementation. |

No old PR branch is a source of pending production work. The linked PRs retain
their detailed bodies, commits, review discussion, and CI results; cleanup does
not erase that GitHub history.

## Kept separate: profile branch delta and config policy WIPs

Two valuable launcher work branches are intentionally kept separate:

- `codex/profile-branch-delta-wip` at `15d6be621` is a local follow-up to
  integrated PR #68. It adds a Settings-facing lineage/status API, effective
  parent-plus-overlay resolution, per-file ownership/policy handling, delta
  application and a no-op fast path, recoverable enforced-conflict copies,
  Repair modpack eligibility, and focused backend coverage. It predates recent
  integration documentation commits and has not been reviewed/rebased as a
  promotion.
  Its candidate contract is preserved in
  `PROFILE_BRANCH_BACKEND_CANDIDATE.md`. Keep the branch until it receives a
  fresh integration review; do not mislabel it as the already merged PR #68.
- `codex/toml-config-rules` at `7617d322f` adds TOML, `.properties`, and `.txt`
  merge helpers plus per-setting inherited policy resolution.
  `cargo check -p pandora_launcher --locked` passed and the launcher compiled
  and launched locally. However, `merge_config_file` is not yet wired into the
  persistent install/update transaction, conflict-copy handling, or recovery
  journal. It is not ready for `agent/integration-current`; preserve this
  branch/worktree for direct continuation.

The `codex/profile-branch-delta-wip` worktree also contains untracked
`artifacts-agent198/` and `artifacts-current-agent198/` Windows build outputs.
They are outside the branch diff and were left untouched during this cleanup.

## Cleanup policy

- `agent/integration-current` is the shared production/documentation branch;
  do not modify `main`.
- PRs #32–#39 are closed and their exact obsolete remote heads are deleted;
  their GitHub records remain available for detailed history.
- Clean superseded local branches and worktrees were removed after verifying
  their status and confirming their work was integrated or documented.
- Keep historical AppCDS/USN research branches and PR records: they contain
  evidence and are outside this launcher-profile cleanup.
- Keep `codex/toml-config-rules` isolated until its transactional integration
  is complete and reviewed.
- Keep `codex/profile-branch-delta-wip` until its delta/repair follow-up has
  been compared with integrated PR #68 and explicitly promoted or rejected.
