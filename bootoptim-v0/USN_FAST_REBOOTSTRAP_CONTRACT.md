# Asset USN fast-rebootstrap contract

## Provenance and scope

Agent 177 starts exactly from PR #27 head `48a07528dff5b8249c8284545e4cc26efb23b49a` (`agent173/usn-unprivileged-capability-20260915`). The active capability remains PR #27's direct, non-elevated Windows/NTFS FileId + USN query path. This change is limited to the launcher asset-object verification/cache policy and its telemetry/tests.

It does not change AppCDS, classpath/module-path, mods, modpack update semantics, UI/login, Java launch, or background I/O. It does not add a service/helper/UAC requirement.

## Default/request policy

`BOOTOPTIM_ASSET_USN_CACHE=1` remains the explicit, default-off request switch inherited from PR #27. Once requested on the canonical launcher `assets/objects` tree, **every launch authority uses the same fast policy**, including GUI normal launch, CLI/legacy/default/unknown authority and the historical `FullVerification` marker. `FullVerification` no longer means “hash every asset at startup” when this cache policy is requested.

There is deliberately no `strict_full_sha1` setting and no startup path in this policy that cryptographically scans all asset objects.

## Decisions

### `verified_reuse`

A complete valid manifest, exact asset-index/object identity, same volume, continuous same USN journal, same FileId, same file USN and unchanged protected handle authorize reuse exactly as in PR #27. No content SHA-1 is read.

### `fast_rebootstrap`

Missing, malformed, partial, schema-incompatible, index-incompatible or otherwise unusable manifest metadata is not repaired by hashing all objects. For each existing candidate, Pandora opens the exact object as a regular non-reparse file with write/delete sharing denied, queries current FileId/USN metadata where available, and records a fresh baseline. Journal-era loss/regression/discontinuity likewise starts a new baseline rather than auditing object contents.

This is intentionally **not a cryptographic statement about historical bytes**. If an object was already corrupt before the manifest was lost/corrupted or before the new journal baseline, fast rebootstrap can adopt that corruption and it can persist until a later detectable change or an object repair/download occurs. File size and mtime are not used as cryptographic evidence.

If the direct USN capability itself cannot be established, an existing canonical regular non-reparse candidate may still be used for that launch so capability loss cannot trigger a mass hash pass. That degraded run cannot publish new USN authority. Telemetry reports the degraded fast state; it must never be described as verified reuse.

### `individual_repair_verification`

With a usable same-era manifest, FileId/USN/handle changes or a missing/nonregular/reparse candidate invalidate only that object. `verify_existing` returns a miss and Pandora's existing repair/download path runs. The downloaded body keeps the stock expected-size check and SHA-1 check against the published Mojang object hash before it is written. After a successful phase, only metadata for that final individually verified object is added to the USN baseline.

Thus SHA-1 remains allowed and required for the object being downloaded/repaired; it is never used to audit the rest of the cache.

## Publication and failure model

A manifest is published only after every expected object has a current metadata snapshot and the final volume/journal query is coherent with the session. Fast rebootstrap populates those snapshots without reading content. Objects repaired during the run already passed the normal per-download SHA-1 check before metadata publication.

A capability failure is session-latched and prevents manifest publication. It does **not** cause `finish()` to perform a compensating content pass.

## Telemetry

`BOOTOPTIM_ASSET_USN_PROBE=<fresh path>` advances to `bootoptim.asset_usn_cache_probe.v3` and reports:

- `policy="fast_rebootstrap"`;
- `verified_reuse_files`;
- `fast_rebootstrap_files`;
- `individual_repair_verification_files`;
- `stock_sha1_files` (expected to remain zero for a requested canonical fast-policy launch; retained to expose regressions/feature-off behavior);
- direct capability/session/publication state and `elevation_requested=false`.

`fast_rebootstrap` and `individual_repair_verification` are deliberately separate counters: the former carries reduced historical-integrity guarantees; the latter means the normal single-object repair/download verification path was requested.

## Strictness boundary

There is no global strict startup mode in this candidate. The strict cryptographic boundary is per-object only: a missing object or a FileId/USN/handle invalidation reaches `individual_repair_verification`, and the downloaded/repaired object is SHA-1 verified before acceptance. No CLI/legacy/unknown authority can silently request a full-cache audit.

## Required validation

Focused backend tests must prove:

1. corrupt manifest -> `fast_rebootstrap` with no content SHA-1 observation, followed by a valid metadata manifest;
2. unchanged next run -> `verified_reuse` with no content SHA-1;
3. same-size/restored-mtime mutation after rebootstrap -> `individual_repair_verification` on the next run via changed USN, without hashing the existing object;
4. delete/recreate after rebootstrap -> individual repair via changed FileId;
5. telemetry distinguishes `fast_rebootstrap`, `verified_reuse` and `individual_repair_verification`.

Hosted CI is semantic/packaging evidence only. Physical acceptance measures only `assets_verify_download`; it must not be reported as TTMM. A corrupt/missing-manifest physical run should show fast rebootstrap and zero stock SHA-1 object reads, while a subsequent single-object mutation should request exactly one repair/download verification.
