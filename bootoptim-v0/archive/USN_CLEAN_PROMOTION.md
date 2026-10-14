# Clean USN assets-cache promotion candidate

This line is recomposed from stock `master@4eb6c7849561151695288443c106519774ee05ea`. PR #29 (`agent179/usn-clean-promotion-20260916@31cbeec450f0f3da4b503d5631a2f27a403e6bf5`) is the exact parent for this policy-only follow-up. Fast-rebootstrap semantics are selected from PR #28 by diff/file content only; no experimental ancestry is merged or rebased.

## Active Windows policy

- PR #29's direct unprivileged NTFS capability remains the only active authority. There is no elevated helper, named-pipe protocol, or UAC path.
- Canonical Windows `assets/objects` uses the same fast policy for GUI, CLI/legacy/default/unknown authority, and the historical `FullVerification` marker. `BOOTOPTIM_ASSET_USN_CACHE` is accepted only as a compatibility no-op and cannot restore a full-cache SHA-1 pass.
- Valid compatible manifest plus continuous same-volume journal/FileId/USN/protected-handle identity gives `verified_reuse` with no content SHA-1.
- Missing, corrupt, incompatible, partial, or journal-invalid metadata gives `fast_rebootstrap`: existing protected regular non-reparse objects are adopted into a fresh FileId/USN baseline without reading their contents.
- FileId/USN/handle change under a usable baseline, or a missing/nonregular/reparse object, gives `individual_repair_verification`: that object alone enters Pandora's normal download/repair path, where the downloaded body is size checked and SHA-1 checked against its published hash before write.
- Direct capability/session failure degrades quickly for the current launch using only a protected regular non-reparse candidate and does not publish new USN authority. A verifier worker `JoinError` is an individual miss and never compensates by hashing the existing pathname.

## Integrity boundary and residual risk

Fast rebootstrap is intentionally weaker than a cryptographic seed. Corruption that already existed before the manifest was lost/corrupted, or before a new journal baseline is established, can be adopted and may persist until a later detectable metadata change or explicit repair/download. File size and mtime are not cryptographic evidence and are not used to authorize reuse.

The cryptographic boundary is per repaired object only. There is no `strict_full_sha1` mode and no active or dormant Windows startup path that audits every existing asset body. This follow-up adds no helper/UAC runtime, AppCDS/interposer, UI/login/mod/update/classpath behavior, or background I/O.

## Publication and telemetry

A manifest is published only when every expected object has a current metadata snapshot and the final journal query remains coherent. A latched capability failure prevents publication. Activation telemetry remains observational and advances to schema v3 only to distinguish `verified_reuse_files`, `fast_rebootstrap_files`, `individual_repair_verification_files`, and `stock_sha1_files`; it still reports `capability_mode=direct_unprivileged_ntfs` and `elevation_requested=false` and never authorizes reuse.

## Validation boundary

Focused validation is `cargo fmt -p backend -- --check`, `cargo check -p backend --tests --frozen`, Windows `cargo test -p backend asset_usn_cache --frozen -- --nocapture`, and activation-probe coverage. Required behavior is corrupt manifest -> zero content hashes and fast rebootstrap; unchanged next run -> verified reuse; subsequent same-size/restored-mtime mutation or delete/recreate -> exactly individual repair without hashing the existing pathname; disabled/error capability -> no false baseline publication.

Hosted checks are semantic evidence only. Physical follow-up must report the assets phase separately and must not claim TTMM from these tests.
