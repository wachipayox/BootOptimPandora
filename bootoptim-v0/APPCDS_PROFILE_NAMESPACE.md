# AppCDS per-profile namespace contract

Status: Agent 201 correctness/composition slice on `agent/integration-current@3410b4c47eec0f47012d3c33bbd5e1d58b669e14`.

## Decision

AppCDS remains physically instance-local at the existing
`<instance>/.bootoptim/appcds` path, so Pandora's fixed TRAIN/READY flag protocol
does not change. A legacy instance keeps that behavior unchanged.

When a persistent profile exists, the cache is explicitly bound to its durable
`profile_uuid` with `profile.namespace`, and the same UUID is serialized into
the launch plan. Identical pack bytes in two profiles therefore do not authorize
archive sharing. Shared immutable content-library objects remain source objects
only; they are never inferred to mean shared AppCDS ownership.

## Fail-closed profile scope

A persistent namespace is eligible only when:

- `.pandora-layout-v1` and its identity/manifest are plain non-reparse objects;
- schema is v1, the UUID is canonical and identity/manifest UUIDs match;
- the manifest is `ready`, generation is non-zero and transaction id is null;
- no journal or publishing marker exists;
- staging, backup and conflicts are absent or empty.

On Windows the helper opens the existing per-UUID layout lock with zero sharing
and `FILE_FLAG_OPEN_REPARSE_POINT` before validating Ready state. That lease is
held across plan hashing, AppCDS cache classification/promotion and the helper
decision. A concurrent persistent-layout publisher therefore makes AppCDS return
STOCK instead of reading a moving profile. The existing AppCDS `cache.lock`
still serializes AppCDS state transitions inside the bound cache.

Legacy instances have no persistent publisher namespace and keep the historical
root-local cache/lock behavior.

## Switching, clone, rename and delete

Switching A -> B -> A uses different instance roots and distinct UUID bindings.
B never rewrites A's plan, state or archive, so an intact A can reuse its own
READY archive when its exact launch plan still matches.

Pandora duplication already remints the persistent UUID. The copier now also
skips the complete `.bootoptim/appcds` subtree while preserving unrelated
`.bootoptim` state. A clone therefore starts with a fresh AppCDS cache and cannot
inherit the source archive merely because the pack bytes are identical.

Renaming an instance carries both the UUID and root-local AppCDS cache. The UUID
binding remains valid. Consumption still requires the exact launch plan; if an
existing plan input is path-sensitive and changes because of the rename, AppCDS
fails closed rather than making that input rename-neutral in this slice.

Deleting a profile removes its cache with the instance root. There is no
launcher-global AppCDS registry that can leak ownership to another profile.

## Migration

No pre-existing unbound archive is reinterpreted as belonging to a persistent
UUID. On first persistent use, a non-empty unbound `.bootoptim/appcds` is renamed
to a unique `appcds-unbound-v0-*` quarantine directory and a fresh bound cache is
created. This may require one fresh training cycle after adopting persistent
identity, but it avoids silently assigning old state to a profile and does not
change TRAIN/READY semantics. Legacy instances are not migrated.

A present binding with a different UUID is never overwritten or adopted; AppCDS
returns STOCK and preserves the evidence.

## Concurrency and recovery

Two helpers for one persistent profile contend on the profile UUID lock lease and
then the AppCDS cache lock; uncertainty returns STOCK. Different UUIDs have
independent instance roots, profile locks and cache state. Interrupted layout
publication, conflicts, corrupt identity/manifest, a missing persistent lock file,
or unsafe filesystem objects make AppCDS ineligible without modifying recovery
evidence.

This slice does not change incremental identity hashing, TRAIN behavior, asset
verification, classpath/module-path ordering, or profile reconciliation.
