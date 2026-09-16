# Persistent per-profile `.minecraft` layout

Status: architecture decision / implementation plan. This document does not
claim a performance improvement. Physical validation, when an implementation
exists, must use the existing Start-to-Java/prelaunch boundary rather than TTMM.

## Decision

Treat the existing `Instance` root as the current profile boundary and keep its
`.minecraft` tree as the persistent, user-visible launch layout. Managed pack
changes are reconciled transactionally when the profile is installed, updated,
or its launch-affecting managed selection changes. A clean unchanged profile is
already launchable: Start must not rotate `mods`, scan/copy user extras,
materialize the managed mod set, or re-apply sync targets.

The global content library remains an immutable/deduplicated source. Ownership,
publication state, conflicts, recovery and generation are per profile. Sharing a
content-library blob never means sharing a mutable destination inode.

This intentionally replaces the steady-state `mods -> original_mods -> mods`
launch transformation. `original_mods` remains only a legacy/stock recovery
shape during migration and fail-open fallback; it is not a prepared-layout
cache.

## Current source contract that the design must preserve

The decision is based on the current PR #30 tree:

- `Instance` owns `root_path`, `dot_minecraft_path`, content paths/watch state,
  and a `frozen_mods_folder` flag. `load_content` returns the last Mods view
  while that flag is set.
- `BackendState::prelaunch` applies syncing and then calls
  `prelaunch_setup_mods`. The stock setup loads Mods, determines known Pandora
  files, preserves unknown top-level entries as extras, resolves modpack files
  and disabled children, freezes the Mods view, renames `mods` to
  `original_mods`, recreates `mods`, materializes managed mods and copies extras.
- `restore_mods_folder_if_stopped` refuses restoration while a process,
  closing process or launch keepalive is live. Once stopped it merges
  `mods/.connector` into `original_mods/.connector` when sandboxing is off,
  removes the launch Mods directory, renames `original_mods` back, and unfreezes.
  Instance loading and process housekeeping both invoke this recovery path.
- Modpack child selection already uses each summary's `disabled_children`.
  Override state is recorded in auxiliary metadata. Existing override logic
  replaces a prior override only when the on-disk file still matches the old
  applied hash; `config/yosbr/` currently has a special always-try-to-override
  rule.
- `syncing::apply_to_instance` can modify file targets and creates shared folder
  links/junctions. It currently runs in prelaunch, so persistent-layout mode has
  to move that application to profile/settings reconciliation rather than simply
  skip it.
- Content updates are already instance-addressed: update actions produce a
  `ContentInstall` with `InstallTarget::Instance(id)`. The content library may be
  global, while the destination is per instance.
- `fs::fastcopy` supports reflink, optional hardlink, then ordinary copy. The
  current prelaunch managed materialization passes `hard_link=false`; that is
  the required alias-safety boundary for the persistent layout too.

## Profile identity and on-disk state

`InstanceID` is a runtime slab identifier, so it cannot be the durable profile
identity. On first migration create a UUID stored with the profile layout state.
A renamed instance carries the UUID with its root. Duplicating an instance must
mint a new UUID and must not copy an active transaction/recovery identity.

Proposed layout, all control state on the same volume as the instance root:

```text
<instance>/
  .minecraft/                         # persistent live/user-visible layout
  .pandora-layout-v1/
    manifest.json                     # last committed generation
    journal.json                      # present only while transaction/recovery is active
    staging/<transaction-id>/         # new managed payload before publication
    backup/<transaction-id>/          # old proven-managed entries moved out during publication
    conflicts/<transaction-id>/       # pending managed candidate/evidence; never silent data loss
```

`manifest.json` is written with `write_safe`-style temp+flush+sync+rename. It
contains at least:

```text
schema
profile_uuid
generation
state = ready | conflict | needs_reconcile | stock_only
managed_input_fingerprint
sync_identity
sandbox_policy
managed_entries[path] = {
    logical_identity,
    source_kind,
    source_hash,
    applied_hash,
    entry_policy
}
pending_conflicts[]
```

The manifest is not an inventory of all user files. Unknown/local files are
preserved precisely by not owning them. Only paths in the old/new managed set,
plus explicit sync/runtime exclusions, participate in reconciliation.

### Managed identity

A managed entry identity must be deterministic from the semantic inputs that
produce it, not from mtimes. Depending on entry kind it includes:

- safe destination path and file kind;
- content-library hash/source for an installed mod;
- loader/Minecraft selection where it changes effective content;
- enabled/disabled state and disabled modpack-child selection;
- modpack parent identity plus child path/source/hash;
- generated/builtin byte identity;
- applied override identity/policy for config and extras;
- any profile-local setting that changes the desired managed destination set.

The profile-level fingerprint is a canonical digest of the sorted desired
managed entries and relevant policy versions. Shared/global input is included
explicitly only when it changes this profile's desired layout.

## Ownership model

Each destination is one of these logical classes:

1. **Managed**: Pandora can prove the live bytes are the bytes it last
   published. Replacement/deletion is permitted only after that proof.
2. **Local**: user/runtime-owned, including unknown files and directories.
   Reconciliation does not delete or overwrite it.
3. **Local override of managed**: the path was managed previously but no longer
   matches the last applied identity (including an intentional deletion).
   It is data, not corruption, and cannot be overwritten implicitly.
4. **External/synced**: owned by the existing sync target. It is excluded from
   managed publication and validated as a link/junction according to existing
   sync semantics.
5. **Runtime special**: state such as Connector that has explicit lifecycle
   semantics and cannot be treated as an immutable managed blob.

Before any destructive managed update, compare the live path with the previous
manifest's `applied_hash` (and file kind). Metadata/file-id can avoid work when
safe, but uncertainty requires hashing that path during the update operation.
There is no full-tree hash pass and no Start-time validation pass.

A missing formerly-managed file is a local tombstone unless the launcher can
prove it performed the deletion in the current transaction. It is not silently
reinstalled.

## Conflict policy

The safe default is **preserve the live local value and leave the managed update
pending**. The new managed candidate may be retained in the profile conflict
area for explicit resolution; the current live path is not displaced merely to
make the update succeed.

| Old committed ownership | Live state | New desired state | Action |
| --- | --- | --- | --- |
| absent | absent | managed file | stage and publish managed file |
| managed, still matches | same managed identity | unchanged | no I/O |
| managed, still matches | old managed bytes | changed managed identity | backup old, atomically replace with staged candidate |
| managed | locally modified bytes | changed/unchanged managed | preserve live; record local override/conflict; do not overwrite |
| managed | missing | still desired | preserve tombstone; record conflict |
| managed, still matches | old managed bytes | removed from pack | move/remove only the proven-managed entry |
| managed | locally modified bytes | removed from pack | keep it and reclassify local |
| absent/local | local file at new managed destination | managed file | preserve local; stage managed candidate as conflict |
| any | unexpected directory/symlink/reparse/type | managed file | no destructive action; conflict or stock-only |

`config/yosbr/` keeps its managed-default intent only while the destination is
still proven to be the previously published managed entry. A user modification
turns it into a conflict instead of silently destroying data. This tightens the
existing special-case overwrite behavior to satisfy the explicit local-data
preservation requirement.

UI conflict resolution is a later surface, but the storage/API state must
support at least: keep local (managed candidate stays unapplied), accept managed
(local is first moved to conflict/backup storage), or cancel update. No default
resolution loses bytes.

## Transaction and publication state machine

```text
Legacy/Unknown
   | migrate/reconcile while stopped
   v
Ready(generation N) ------------------------------+
   | managed/profile input changes                |
   v                                              | unchanged Start
NeedsReconcile                                    | = no layout writes
   | acquire per-profile lock; recover old txn    |
   v                                              |
Planning -> Conflict -----------------------------+ (live generation N remains)
   |
   v
Staging -> Prepared -> Publishing -> ManifestCommit -> Ready(N+1)
                         |     ^
                         |crash/failure
                         v     |
                      Recovering
                         |
                  rollback or finish
```

A game process/closing process/launch keepalive makes layout reconciliation
ineligible. Updates that would modify managed layout remain pending until the
profile is stopped.

### Publication protocol

1. Recover any prior `journal.json` before planning a new transaction.
2. Build the desired managed manifest from current content/modpack/profile
   inputs. This work belongs to install/update/profile-change flow, not Start.
3. Compute only the old-vs-new managed delta. Validate destructive destinations
   against the old committed ownership. Any mismatch becomes a conflict.
4. Stage each new managed file under the profile root. For content-library
   files, use reflink with **hardlink disabled** and ordinary copy fallback.
   Generated bytes are written to staging and synced. Never rename a source out
   of the global content library.
5. Persist a journal describing exact operations and expected old/new
   identities before touching live paths.
6. For each changed path, move a proven-managed old entry to the same-profile
   backup area, then rename the staged replacement into the live destination.
   Individual same-volume renames are atomic. Because local files are interleaved
   with managed files, a whole-layout directory swap is deliberately not used.
7. Write/sync/rename the new manifest only after every live operation succeeds.
8. Mark the transaction committed, then remove staging/backup lazily outside
   Start. Cleanup is idempotent.

The multi-file transaction is **crash-recoverable, not globally atomic**. The
journal plus per-path atomic renames are the correctness boundary. While a
transaction is `Publishing`/`Recovering`, the profile is not launchable.

### Crash recovery

On launcher startup/profile load, before exposing `Ready`:

- journal absent + valid manifest: normal;
- staging exists but no publication journal: delete only launcher-owned staging;
- journal before manifest commit: replay backwards from recorded backups and
  staged/live identities; never infer ownership from filename alone;
- manifest committed but cleanup incomplete: finish cleanup idempotently;
- corrupt manifest/journal or ambiguous live/backup identity: preserve all live
  and recovery data, mark `stock_only`/manual recovery, and do not destructively
  guess.

A game crash no longer requires restoring the whole Mods directory in persistent
mode because no launch-time rotation occurred. Legacy `original_mods` recovery
continues to run for profiles not yet migrated or explicitly on stock fallback.

## Start contract

For a persistent-layout profile, the in-memory state loaded before the click is
one of `Ready`, `Conflict`, `NeedsReconcile`, `Recovering`, or `StockOnly`.

`Ready` Start performs no layout reconciliation. Specifically it does not call
`apply_syncing_to_instance`, `prelaunch_setup_mods`, `apply_copies_to_mods_dir`,
or a Mods-directory preservation scan. It launches the already-prepared
`.minecraft` tree.

A state other than `Ready` must never trigger a hidden synchronous rebuild and
then claim the fast path. The first implementation may fail open to the existing
stock prelaunch only when the legacy state is known safe; transaction ambiguity
or unresolved data conflicts must be resolved/recovered first. Reconciliation
is an explicit install/update/profile-change operation.

## Sync, modpack, extras, Connector and sandbox

### Sync

Sync folders/files remain governed by `SyncTargets`; they are not claimed as
managed pack entries. Applying/changing sync targets becomes a profile/settings
transition that updates affected profiles and their `sync_identity`. Existing
Unix symlink/Windows junction semantics stay authoritative. If expected sync
link state cannot be guaranteed, persistent mode is `StockOnly` until repaired.

### Modpacks and disabled children

The desired managed set is produced by the same modpack resolution and
`disabled_children` filtering used today. A child toggle is a managed-input
change and reconciles that profile only. Modpack download/source identity is
retained per entry so a same-name unrelated local file is never claimed by
filename alone.

### Extra/local files

The current prelaunch scans unknown top-level Mods entries only because it is
about to replace the whole directory. Persistent mode stops replacing that
whole directory; therefore unknown files/directories stay in place and need no
copy. A new managed destination colliding with one becomes a conflict.

### Config/overrides

Existing auxiliary applied-override hashes are migration evidence and should be
folded into the new manifest. Update-time replacement remains conditional on
proving previous ownership. No normal managed update performs a whole config
scan.

### Connector

In non-sandbox persistent mode, `mods/.connector` is runtime/local state and can
remain in place, eliminating the copy-out/merge-back caused by Mods rotation.
It is excluded from immutable managed publication.

Sandbox currently depends on discarding launch-time Connector changes by
skipping the restore merge. A persistent live Mods directory would otherwise
change that behavior. Therefore the first implementation must keep sandbox
profiles on the stock path unless/until a separately proven sandbox-safe
Connector overlay exists. Do not introduce a new fragile junction/hardlink to
fake this semantic.

## Filesystem/Windows safety boundary

The prepared-layout mode is conditional on ordinary filesystem guarantees; it
must fail open rather than weaken ownership:

- **Hardlinks are forbidden for live managed destinations.** They allow a local
  write to mutate a shared content-library inode.
- Reflink/COW is allowed only as an optimization. Unsupported reflink,
  cross-volume source, or filesystem without clone support falls back to normal
  copy.
- Rename is used only for same-profile staging/backup/live publication, never to
  consume the shared source blob. Control directories are deliberately under
  the instance root to make same-volume rename the normal case.
- Unexpected symlinks, junctions/reparse points, path-type changes, unsafe paths,
  or traversal through an unowned link are conflicts unless they are an
  explicitly modeled sync target.
- ACL denial, read-only destinations, antivirus/share violations, disk-full,
  failed fsync/rename or other I/O errors abort publication. Before manifest
  commit the transaction rolls back from recorded backups. If rollback itself
  is ambiguous, block persistent launch and preserve evidence rather than
  deleting data.
- No correctness claim depends on filesystem watcher delivery. Watchers may mark
  state dirty promptly, but update-time ownership proof and startup journal
  recovery are authoritative.

## Migration from current instances

Migration is per profile and happens while the instance is stopped:

1. If legacy `original_mods` exists, run the existing stopped-instance restore
   first. An ambiguous/failed restore remains stock-only; do not build new state
   on top of it.
2. Mint the durable profile UUID and create an empty transaction namespace.
3. Build the desired managed set using the current content/modpack/child logic.
4. Use existing auxiliary metadata and content hashes as evidence, but claim a
   current path as managed only if its bytes/type match the desired or known
   previously-applied identity. Unknown/mismatching destinations stay local and
   become conflicts if the managed set needs the same path.
5. Reconcile/stage/publish through the normal transaction protocol.
6. Commit generation 1. From that point a clean launch no longer creates
   `original_mods`.

A corrupt/missing new manifest never causes deletion or a Start-time full
rebuild. The live `.minecraft` remains authoritative user data; the profile is
marked for explicit reconcile/migration or safe stock fallback.

Instance duplication must mint a new profile UUID and reset transaction/cache
identity. Shared immutable library blobs may still be reused.

## Implementation cuts

The change is intentionally split so each slice has a falsifiable boundary:

1. **State model only:** per-profile manifest/journal types, canonical managed
   fingerprint, recovery parser, no launch behavior change.
2. **Transactional publisher:** staging, copy/reflink-without-hardlink,
   backup/rollback/recovery and adversarial filesystem tests, still not selected
   by Start.
3. **Desired-layout resolver:** extract current modpack/mod/child/config logic
   into a side-effect-light plan consumed by both stock prelaunch and publisher.
   Do not optimize `Instance::load_content` parsing in this work.
4. **Mutation hooks:** instance-target content install/update, child enablement,
   relevant profile configuration and sync-setting changes reconcile only the
   affected profile(s).
5. **Persistent Start fast path:** only after committed `Ready`, bypass sync
   application and Mods reconstruction. Keep stock path as explicit fallback.
6. **Legacy migration:** one-time stopped-profile conversion and
   `original_mods` recovery compatibility.
7. **Sandbox follow-up:** only if Connector/runtime isolation can be preserved
   without aliasing or fragile links; otherwise retain stock fallback.

## Required tests before a runtime candidate

| Scenario | Required assertion |
| --- | --- |
| Profiles A/B seeded | distinct profile UUID, manifest, journal/staging namespace and live layout |
| Update A | A generation/layout changes; B manifest bytes, generation and live managed files do not change |
| Local mod added to A | survives update A and switch A->B->A; never appears in B |
| Local modification at managed A path | update records conflict; local bytes unchanged; managed candidate not silently installed |
| New managed file collides with local A file | preserve local; conflict is durable across restart |
| Managed removal after local modification | local file retained/reclassified |
| Disabled modpack child toggle | only selected profile desired set/generation changes |
| Switch Ready A->B->A | no materialization/sync writes merely because of switching |
| Crash during staging | live generation unchanged; orphan launcher staging cleaned safely |
| Crash after backup/before replacement | recovery restores old live managed path |
| Crash after replacement/before manifest commit | rollback reconstructs generation N or enters explicit recovery; never guesses/deletes local |
| Crash after manifest commit | generation N+1 remains authoritative; cleanup is idempotent |
| Corrupt manifest | live `.minecraft` preserved; no destructive auto-rebuild at Start; explicit reconcile/stock-only |
| Corrupt journal | profile not treated Ready; all evidence preserved |
| Filesystem has no reflink | ordinary copy succeeds; no hardlink attempted |
| ACL/antivirus rename failure | no committed partial generation; rollback or explicit recovery |
| Cross-volume global content library | copy fallback; same-profile publication remains rename-based |
| Unexpected symlink/junction at managed path | conflict/stock-only, not traversal/destructive replace |
| Sync target enabled/disabled | existing shared target semantics preserved and fingerprint updated outside Start |
| Connector non-sandbox | runtime cache persists without copy-out/merge caused by Mods rotation |
| Sandbox | stock path until equivalent discard semantics are proven |
| Legacy `original_mods` after crash | existing restore happens before migration; no new manifest committed on failed restore |
| Ready Start | test hook proves no `apply_syncing_to_instance`, Mods top-level preservation scan, `apply_copies_to_mods_dir`, or `original_mods` rotation |

Focused Rust fmt/check/tests are required on each implementation slice. Hosted CI
is correctness evidence only. Any performance claim requires a physical A/B of
prelaunch and Start-to-Java with the existing measurement root; do not report
TTMM for this launcher-layout change.
