# Persistent profile client-flow candidate

This document describes the Agent 188 backend candidate stacked on PR #32. It does **not** select a Start fast path and makes no Minecraft startup-time or TTMM claim.

## Boundary

The callable backend seam is `BackendState::reconcile_persistent_profile_layout(InstanceID, &[DesiredManagedFile])`. It is intended for a later install/update/profile-change caller, not for the Start button.

Before the new profile namespace is opened, the service requires the instance to be stopped, keeps sandbox profiles on the stock path, invokes Pandora's existing `restore_mods_folder_if_stopped`, and proves any pre-existing `original_mods` recovery completed. Only then may `.pandora-layout-v1` be created.

The caller supplies a fully resolved local desired set. Each entry contains a safe instance-relative destination, semantic logical identity, expected SHA-256, and a local immutable source path. This slice performs no network I/O, signature verification, update discovery, account lookup, channel selection, or Linux service call.

## Ownership and publication

The side-effect-light classifier from PR #33 is reused as the ownership authority. A destructive replace/remove is emitted only when the live regular-file SHA-256 equals the previously committed `applied_hash`. Modified formerly-managed bytes become `LocalOverride`; a missing formerly-managed destination is a tombstone; a new destination colliding with local bytes stays local. Reparse/unexpected types require stock fallback. Unknown local paths outside the old/new managed delta are not scanned or claimed.

A successful update uses one UUID-owned transaction under `.pandora-layout-v1`:

1. Build the old/new managed delta and reject conflicts before live mutation.
2. Copy changed managed sources into private `staging/<tx>/live/...`; hardlinks are never used.
3. Persist `journal.json` with exact per-path operation kind and expected old/new hashes before touching `.minecraft`.
4. For replacement/removal, re-prove the old live hash and rename it into same-profile `backup/<tx>/live/...`. For installation/replacement, rename the verified staged file into the live destination.
5. Verify all target live identities.
6. Move the previous manifest into the transaction backup, then rename the staged target manifest into place.
7. Verify the target live identities again. Only a committed `state=ready` manifest with the complete target live publication can become Ready; staging/backup/journal cleanup follows and is idempotent.

The transaction is crash-recoverable rather than globally atomic. Before manifest commit, recovery walks journal operations backwards and restores only recorded, hash-proven backup bytes. After manifest commit, recovery verifies the target live state before cleanup. Any corrupt journal/manifest, ownership mismatch, unexpected type, reparse point, or ambiguous live/backup identity preserves evidence and returns an error so the caller stays stock/legacy; recovery does not guess ownership.

Conflicts are durable launcher-owned records under `.pandora-layout-v1/conflicts/`. Their presence makes the loaded status `NeedsReconcile`; a later successful publication clears them.

## Future Linux service / Beta connection

A later Linux distribution service or Beta channel integration must terminate **before** this API at a verified local desired-set boundary. The future adapter must authenticate/authorize the user/channel, verify immutable revision signatures and object hashes, resolve inheritance/overlays, materialize verified content-library objects locally, then pass only those already-verified source paths plus semantic identities/hashes to `reconcile_persistent_profile_layout`.

The client flow must not treat server filenames as ownership proof, must not bypass local override/tombstone/conflict rules, and must not make a profile Ready until the local transaction commits and verifies. Beta visibility/channel policy remains server/UI work and is not represented in this slice.

## Validation and metrics

Focused Rust validation covers profile A/B UUID isolation, proven managed replacement, local override/tombstone/local collision preservation, legacy restoration gating, crashes before and after manifest commit, corrupt journal/control state, reparse rejection, and idempotent recovery. The release Windows/Linux/macOS matrix is intentionally deferred until review accepts this backend candidate.

Physical HDD evidence from PR #30 remains motivation only: `prelaunch Start→Java = 23.731 s`, `load_content = 14.702 s`, `apply_copies_to_mods_dir = 3.077 s`, and extra copies `= 4.649 s` are separate/non-additive scopes. This candidate is validated for correctness/recovery only. If a Ready Start path is wired later, measure click/request→Java spawn separately from Java spawn→usable menu and from AppCDS training/exit work.
