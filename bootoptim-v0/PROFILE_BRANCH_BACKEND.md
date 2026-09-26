# Local profile branch backend

This document describes the backend seam implemented on top of Pandora's current
persistent instance layout. It does **not** change Start. PR #66 remains the
ground truth: an instance's existing `.minecraft` stays live and writable and
Start does not rebuild or rotate `mods/`.

## Settings-facing backend API

The backend exposes these stopped-instance methods on `BackendState`:

- `persistent_profile_branch_status(id, target_revision)` — reads the durable
  UUID/branch state and reports applied-vs-target revision metadata without
  scanning managed destinations.
- `configure_persistent_profile_lineage(id, lineage)` — stores either an exact
  global revision parent or a local parent UUID. Local overlays never leave the
  client.
- `apply_persistent_profile_delta(id, delta)` — applies an already verified and
  resolved revision-history delta. Only changed destinations are observed; only
  changed `enforced` regular files are hashed.
- `repair_persistent_modpack(id, effective)` — explicit full managed-tree
  parity. It is rejected for a purely local profile and remains separate from
  the existing Repair game files backend.

`resolve_effective_entries` is a pure helper for combining a verified parent
effective tree with a private local overlay while retaining origin, ownership,
and policy metadata.

## Persistent format

All local state remains under the instance root's `.pandora-layout-v1/`
namespace:

- `identity.json` — schema version and durable local profile UUID.
- `manifest.json` — transactionally committed generation, ownership proof for
  enforced managed files, and a `branch` block containing lineage,
  `applied_revision`, and effective-entry metadata.
- `journal.json` — in-progress publication transaction.
- `staging/<tx>/` and `backup/<tx>/` — private transaction state.
- `conflicts/` — blocking/unsafe reconciliation records.
- `conflict-copies/<tx>/<relative path>` — recoverable old local bytes that an
  `enforced` update had to overwrite.

The manifest schema remains version 1 and the new `branch` field is
serde-defaulted so an existing v1 candidate manifest can be opened without
inventing remote lineage.

A global revision pin is exactly
`(profile_id, revision_id, manifest_sha256)`; no parent is interpreted as
"latest". A local parent reference is a durable local profile UUID. A derived
local profile may cache its verified global ancestor pin for Repair modpack
eligibility.

## Policy and fast-path semantics

- `enforced`: a changed target value is applied. If the live regular file no
  longer matches the previous applied hash, the old bytes are moved into
  `conflict-copies` only after the transaction commits. Rollback restores the
  old bytes instead.
- `default_once`: seed only when the path has not been initialized and is
  absent. After that first decision the entry becomes user-owned and later
  ancestor changes do not hash or overwrite it.
- `user_owned`: same non-destructive destination behavior as initialized
  `default_once`.
- Local ownership shields a path from later inherited ancestor deltas.
- A stable target revision with an empty delta performs no destination I/O.
- A non-empty update verifies only destinations named by that delta.
- Repair modpack checks the whole resolved managed tree (not unrelated local
  additions) and is unavailable without a global ancestor.

## Interruption and isolation

The existing durable UUID-specific OS lock is acquired before layout open,
recovery, planning, staging, publication, manifest commit, and cleanup. The
instance-state write guard also keeps Start/state mutation out of the
maintenance transaction.

Publication stages immutable sources, writes and fsyncs the journal, verifies
old/new hashes immediately before live mutation, keeps reversible backups,
commits the manifest atomically, verifies the target, then cleans up. On the
next maintenance open, an interrupted transaction either finishes committed
cleanup or rolls the live files and manifest back. Reparse/symlink or unexpected
destination types are blocking conflicts rather than destructive guesses.

The implementation does not add Distribution networking, signature
verification, UI, Modrinth/CurseForge expansion, or a Start hook. Those remain
outside this backend seam.
