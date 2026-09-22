# Agent 205 — composed AppCDS two-run candidate

Base authority: `agent/integration-current@7a821e8d5f89c71677d149b3140e21f4ed171ec4`.

## Composition

The composition order is deliberate rather than a blind stack of PR heads:

1. PR #50 supplies PR #48's persistent-profile namespace/lease semantics plus the per-instance AppCDS toggle and explicit exact-local-clone candidate flow.
2. PR #47 replaces the proof-only launch with immediate TRAIN, while preserving the persistent invalidation tombstone and requiring a later independently rebuilt identity before promotion/consumption.
3. PR #51 supplies the opt-in incremental identity digest cache through direct unprivileged NTFS/USN.

The #47/#50 overlap in `prepare_launch` is resolved explicitly. A pending training campaign wins over exact-clone adoption; exact-clone adoption is considered only when training reconciliation is clean. The persistent profile lease remains held across identity reconstruction and the AppCDS decision.

The #50/#51 overlap is also explicit: `appcds_profile_uuid` remains part of the canonical serialized plan, while only classpath, module-path, mod JAR and raw pack-input file digests are eligible for USN reuse. Java executable/release, helper/launcher components, semantic-canonicalized configuration, resource-pack selection and READY archive hash/size validation retain their strong paths.

## Activation and failure policy

`BOOTOPTIM_APPCDS_IDENTITY_CACHE=1` remains default-off and requires launcher-owned normal-GUI authority. CLI/legacy/unknown authority, force-stock, corrupt/incomplete manifest, reparse/capability uncertainty, journal discontinuity/restamp/regression, FileId/file-USN mismatch, same-handle identity change, lock contention, publication uncertainty or unsupported filesystem never authorizes digest reuse. No broker, service or UAC path is introduced.

Membership and ordering of classpath/module-path/mods/pack inputs are rebuilt on every preflight. Reuse is per file only after exact role/path/volume/journal/FileId/file-USN/final-handle evidence matches.

## Hosted correctness tiers

The focused Windows interposer suite contains a composed two-run proof:

- first run: zero reused eligible digests, positive strong eligible hashing, identity manifest publication, first plan observation, and TRAIN;
- simulated clean exit: non-empty `training.jsa` plus exact `training.complete`;
- second unchanged run: byte-identical launch plan, positive direct-USN digest reuse, zero strong eligible digest reads, matching training identity, promotion, and READY in that same second run.

Inherited/focused negative coverage includes same-size + restored-mtime mutation, delete/recreate, rename/path change, corrupt/partial manifest, reparse/capability failure, journal restamp/regression/discontinuity, FileId/file-USN/same-handle mismatches, lock contention and GUI/CLI/force-stock authority. PR #48/#50 coverage retains namespace binding, profile lease exclusion, clone isolation, destination UUID remint, exact-clone strong archive hash/size validation, toggle default true, explicit false and re-enable behavior.

Hosted CI is correctness/packaging evidence only. It is not HDD or TTMM performance evidence.

## Physical two-run protocol for the principal agent

Use only the checksummed Windows artifact produced by the Agent 205 final workflow. Do not reset profile layout, assets cache, libraries state, Quickplay or unrelated launcher caches.

1. Close Pandora and Minecraft. Clear only the target profile's AppCDS campaign/identity-cache state needed for a fresh AppCDS campaign. Preserve the persistent `profile.namespace` binding and profile UUID.
2. In one PowerShell session set `BOOTOPTIM_APPCDS_MODE=auto` and `BOOTOPTIM_APPCDS_IDENTITY_CACHE=1`. Start the target instance from the normal GUI.
3. Run 1: record Start→Java and Java→usable-menu separately. Require normal-GUI authority, strong eligible identity hashing, identity-manifest publication and TRAIN. Reach the intended menu and close normally. Require a non-empty `training.jsa` and exact clean-exit `training.complete`.
4. Run 2 without changing Java, launch arguments, pack/configuration or resource selection. Record Start→Java and Java→menu separately. Require membership/order reconstruction, direct-USN reuse of the large eligible digests rather than a ~1.23 GiB full rehash, byte-identical plan identity, confirmation/promotion and READY consumption in this same second run.
5. Negative same-size/restored-mtime: on a fresh seeded campaign mutate one eligible file without changing size and restore mtime. The affected file must strong-hash; stale identity must not consume READY.
6. Negative delete/recreate: delete and recreate one eligible file at the same path, even with identical bytes. FileId evidence must prevent reuse for that file.
7. Do not destructively reset the user's real USN journal merely for this smoke. Journal-discontinuity behavior remains hosted/synthetic coverage.

Agent 205 does not run the laptop and does not merge integration.
