# BootOptim launcher roadmap

The launcher owns local instances and their live game directories. The private
Distribution service owns only published global profiles and immutable
revisions. A global profile can have many child profiles. Children pin one
parent revision; inheritance can continue through any number of ancestors.
The client may create global-to-local and local-to-local branches. Local
overlays and locally created profiles stay on the user's machine and are never
uploaded to Distribution.

This roadmap describes the private BootOptim launcher and pack updater. It does
not target Modrinth/CurseForge pack expansion or general third-party modpack
installation.

## Integrated baseline (2026-09-26)

- Pandora PR #66 is merged into `agent/integration-current` at
  `a4aae071d6062adae9bcffe592ca81c541650a82`. Each private instance keeps its
  existing `.minecraft` live and writable across launches; Start no longer
  rotates/rebuilds `mods/`, expands a third-party pack, or recopies extras.
  This preserves runtime libraries/caches such as MCEF and Analog Audio.
- PR #64 promotes the launch-fast library policy and enables the asset USN
  cache by default. Neither feature implements global profile updates.
- Pandora PRs #67/#68 integrate persistent profile identity, lineage,
  ownership tracking, recoverable reconciliation, revision-delta application,
  and the explicit Repair modpack backend. Old PRs #32–39 are stale historical
  candidates; do not merge their branches over the integrated implementation.
- Distribution profile APIs are implemented in its repository and served
  through configured HTTPS mode. Pandora PR #69 adds verified catalog and
  revision reads, signed-object downloads, and initial global-instance
  creation. The follow-up branch `codex/global-profile-updates-20260926` adds
  incremental updates for direct global instances. These client slices have
  not been runtime-validated against the configured server.

## Product invariants

- Start uses the instance's persistent `.minecraft`. Profile install/update
  applies only the private pack's files; no generic Modrinth/CurseForge
  expansion or per-launch staging copy is needed.
- Local-only files and edits are preserved by default. Global and derived
  instances can offer a separate **Repair modpack** action that performs a full
  parity check against the resolved managed tree. It is unavailable to a
  wholly local instance with no global ancestor.
- The existing **Repair game files** action is distinct from modpack repair
  and belongs under instance Settings/Maintenance, away from the frequent Start
  action.
- Updates use immutable revision history and a per-profile ownership manifest.
  A no-op update and Start must not walk/hash all of `.minecraft`; verify only
  changed destinations and use explicit full parity only for Repair modpack.
- Inherited entries track their source revision and current ownership. A child
  can add, replace, change, or remove a mod, config, resource pack, shader pack,
  or other supported file. Ancestor updates flow to descendants only where
  the child still inherits that entry.
- Per-file policy includes enforced, default-on-first-install, and user-owned
  modes. Supported config formats may add option-level selectors. A conflict
  where a user changed an enforced file applies the new enforced value while
  retaining the user's prior copy in recoverable conflict storage.
- Reconcile/install/update is transactional per profile, crash recoverable,
  and isolated from other profiles. Global content blobs may be deduplicated,
  but mutable destination files and recovery state are never shared.
- Distribution stores and publishes global data only. Local branch metadata,
  private overlays, user saves, and player filesystem inventories stay local.

## Delivery phases

### 1. Persistent layout foundation — integrated

The integrated #67/#68 implementation provides durable UUID identity,
per-profile locks, ownership manifests, conflict retention, transaction
recovery, and persistent `.minecraft` reconciliation outside Start. Preserve
its invariants while building client-facing update and repair workflows.

### 2. Build global profile administration and publication

Distribution provides private HTTPS profile APIs, signed immutable revisions,
and object downloads. The local publishing/signer workflow and polished admin
management still need completion. Keep the private release key on the
publishing machine. See Distribution's roadmap and protocol contract for API
and signing details.

### 3. Add Pandora global/local branch management — in progress

The client covers HTTPS trust settings, global catalog browsing,
signature/digest verification, initial global-instance creation, and
incremental updates for direct global instances. Updates resolve the newly
selected signed revision and reuse locally tracked unchanged entries; changed
objects alone are fetched and passed into the existing delta reconciler.
Remaining work: create local children from global/local parents, expose branch
creation and applied/available lineage state in each instance's
Profiles/Updates settings, support descendants of local parents, and let admins
create global children. Local overlays stay on the user's machine.

### 4. Complete delta updates and explicit modpack repair — in progress

For direct global instances, the first update action resolves the selected
revision history and stages/reconciles only changed managed paths through the
existing persistent-layout transaction. A no-op update avoids object downloads
and does not scan `.minecraft`; it still reads and verifies signed revision
metadata. Remaining work: show update availability/history before applying,
surface per-file policy and progress, support local-descendant delta
propagation, and provide **Repair modpack** beside update history under
instance Settings/Maintenance. Repair explicitly checks parity and restores
managed paths from verified content. Preserve local additions unless the user
resolves a conflict or a policy explicitly enforces the path.

## Acceptance path

1. Install a synthetic global root containing one mod, one config, one
   resource pack, and one removable file.
2. Create a local child; change the config, replace a mod, add a shader pack,
   and keep an unrelated local file. Confirm local state stays client-only.
3. Publish an ancestor update. Verify only still-inherited changed paths apply;
   child overrides/removals and unrelated files remain as configured.
4. Edit a forced local file, publish a new forced value, and verify the new
   value applies while the old local copy remains recoverable.
5. Interrupt a reconciliation and prove the next maintenance open recovers
   safely. A separate Repair modpack detects/restores deliberate corruption.
6. Compare a no-op update and Start with the historical full walk: no full
   `.minecraft` scan/hash is allowed on either path. Validate launch to menu
   and representative in-world behavior after updates.

## Other launcher work

The following roadmap items remain separate from profile distribution:

- self-contained prerequisite installation with consent and clear recovery;
- optional narrow and reversible Windows security integration;
- offline/non-premium development accounts with clear labeling;
- incremental game-asset verification and explicit repair;
- honest separation of launcher preparation and Java-to-menu timings.

Do not let these independent items block the distribution/profile architecture.
