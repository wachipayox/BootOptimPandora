# Agent 168 — AppCDS identity hash-reuse design

Base: PR #19 head `15a6eef952f2463295209b1a9fb56bf722e8cace`, itself stacked exactly on PR #7 final `55f05135396e9682a5d0f76a24e62f21850fbc03`.

This change is design-only. It does **not** skip any SHA-256 read, does not enable AppCDS, and does not change `STOCK`/`TRAIN`/`READY`, launch-plan bytes, archive validation, locking, staging, publication, classpath/module-path order, MoreCulling handling, UI, network, Automodpack, or training. There is no measured saving yet.

## Physical motivation

The HDD capture from PR #19 in explicit `BOOTOPTIM_APPCDS_MODE=plan` attributed almost all AppCDS preflight time to launch-plan construction rather than lock or persistence. The dominant work was repeated strong hashing of the same launch identity bytes: pack inputs, mods, classpath and components. Because `plan` returns before cache classification/promotion, READY archive validation is not part of this capture.

A production candidate may therefore reuse a previously computed AppCDS identity digest only when it can prove that the exact file object whose bytes produced that digest has not changed since the accepted baseline. Size/mtime/existence are never sufficient.

## Safe cache unit

The cache unit is **one regular file artifact**, not an ordered collection and not an entire tree.

Each reusable record must bind:

- the exact encoded path representation already emitted by the launch plan;
- role (`classpath`, `module-path`, `mod`, `pack-input`, `helper`, `launcher`, Java executable, or Java `release`);
- the resulting identity SHA-256 already used by the current builder;
- size only as redundant consistency data, never as authorization;
- local NTFS volume identity;
- NTFS FileId;
- file USN;
- the journal identity/bounds snapshot necessary to establish continuity;
- schema/helper version sufficient to invalidate incompatible cache semantics.

Collections remain reconstructed every launch by the existing code. Classpath/module path preserve their exact JVM order. Mods remain sorted only as the current unordered identity set. Pack-input paths remain discovered and sorted exactly as today. The cache may replace only the content-hash step for an individually proven-unchanged artifact; it must not cache/reorder the collection itself.

This keeps launch-plan serialization byte-identical when every reused digest equals the stock digest.

## Tree inputs and discovery

`config/`, `defaultconfigs/`, `kubejs/` and `scripts/` cannot be represented safely by a single cached tree digest without a stronger directory-membership proof. File creation/deletion/rename must invalidate membership even when all previously known files remain unchanged.

The first candidate therefore still performs the existing directory walk and reparse/type checks on every launch, then applies per-file reuse only to files actually rediscovered in that walk. This preserves:

- exact inclusion/exclusion semantics;
- current path sorting;
- fail-closed handling for symlink/reparse/unsupported entries;
- detection of additions, removals and renames through the fresh walk.

A later tree-level optimization would require journal-backed directory membership evidence for every relevant directory and is intentionally out of scope.

## Canonicalized configuration inputs

The four exact Java-Properties exceptions and exact MoreCulling map exception in `CONFIG_IDENTITY.md` produce an identity digest derived from file contents rather than always `SHA256(raw bytes)`.

They may still be cached at the individual-file level **only** if unchanged-byte identity is proven by the NTFS/FileId/USN contract. Reuse then returns the previously computed final identity digest for that exact file object. The candidate must not infer semantic equality from metadata and must not broaden canonicalization rules.

If the path falls outside the exact authorized canonicalization cases, the cached digest remains the raw-byte SHA-256 already used by stock.

## Files that are not cacheable in v1

The following remain stock-hashed/read:

- `options.txt` resource-pack selection input: current code parses selected lines rather than storing an `Artifact`, so v1 leaves it untouched instead of introducing a second identity representation;
- any reparse point, symlink, directory, unsupported file type, non-NTFS file, inaccessible file, or file whose protected handle cannot be acquired;
- any file on an unknown/changed volume or journal;
- any entry when the helper/session/cache is unavailable, corrupt, partial, incompatible or uncertain;
- READY/training `.jsa` archives: archive validation/promotion is explicitly outside this identity-input cache;
- CLI/other-platform paths: v1 is Windows/NTFS-only and default-off.

Java `release` parsing still runs on the current file contents when its hash is not safely reused. If a future implementation reuses its digest, vendor/version fields must remain byte-identical to stock plan output; the simplest v1 is to leave this small file uncached.

## TOCTOU requirement

No reuse decision may be made from path metadata sampled before opening the file.

For a candidate file, the interposer must:

1. open the exact path itself with `FILE_FLAG_OPEN_REPARSE_POINT` and sharing that denies WRITE and DELETE for the decision lifetime;
2. reject reparse/non-regular/non-NTFS objects;
3. read volume identity and FileId from that still-open handle;
4. obtain journal continuity + file-USN evidence while the protected handle remains open;
5. compare exact volume, journal ID/bounds, FileId and file USN to the cached baseline;
6. reread identity from the same still-open handle before accepting reuse;
7. only then return the cached identity SHA-256;
8. otherwise hash bytes through that same protected handle where possible, then snapshot the resulting baseline before closing it.

Any inability to complete those steps falls back to the stock full hash. A cache hit is an optimization permission, never an integrity authority.

## Relationship to PR #11

PR #11 has the right security shape but the wrong product contract to copy wholesale. Its authenticated ephemeral UAC helper, random local-only named pipe, peer-PID/nonce checks, volume-journal continuity checks and same-handle/final-identity discipline are reusable capabilities. Its asset manifest, expected Mojang SHA-1 fields, `AssetVerificationMode`, canonical `assets/objects` restriction and repair semantics are **not** reusable for AppCDS identity.

Do not duplicate the Windows helper/protocol implementation into the interposer. The safe integration path is to extract/compose the narrow capability layer from #11 into a shared crate/module with operations equivalent to:

- open/authenticate one ephemeral privileged session;
- query one selected local NTFS volume journal state;
- query FileId/USN for a bounded file identity while the non-elevated caller retains its protected handle;
- shutdown.

Pandora, the AppCDS interposer and Java remain non-elevated. Only the narrow helper is elevated, and only when the AppCDS cache experiment is explicitly enabled.

Because PR #11 is stacked on the asset-cache lineage while PR #19 is stacked on PR #7, directly merging #11 into this branch would import unrelated asset semantics and violate the requested separation. Shared capability extraction should be a dedicated prerequisite or carefully rebased integration, not duplicated code.

## Default-off activation

Proposed request flag:

`BOOTOPTIM_APPCDS_IDENTITY_CACHE=1`

Absence or any other value means the existing builder path exactly. Even when requested, all non-Windows platforms, CLI/unknown execution, missing helper pin, helper absence/hash mismatch, UAC cancellation, IPC failure, non-NTFS/reparse, journal uncertainty, cache parse failure, read failure or protected-handle failure use the stock full SHA-256 path.

The cache file itself is untrusted and must be atomically replaced only after a complete launch-plan build. Corrupt/partial records are misses, never errors that block launch.

## Required negative tests before any hash skip is implemented

An implementation PR must prove at minimum:

- cached and stock builders emit byte-for-byte identical `launch-plan.json` for the same inputs;
- content change with unchanged size/mtime -> stock hash and changed plan digest;
- rename -> fresh path record / stock hash;
- delete+replace -> FileId mismatch / stock hash;
- journal ID restamp -> stock hash;
- journal lower-bound discontinuity or `NextUsn` regression -> stock hash;
- reparse point -> stock/ineligible behavior matching current builder;
- protected-handle acquisition failure -> stock hash;
- read/hash failure -> current fail-closed eligibility semantics;
- corrupt/truncated/unknown-schema/partial cache -> stock hash;
- helper missing/hash mismatch/UAC cancel/timeout/protocol/PID/ACL failure -> stock hash;
- canonicalized Properties/MoreCulling unchanged bytes -> cached digest equals stock digest exactly;
- effective canonicalized value change -> no stale digest reuse;
- classpath/module-path order remains unchanged;
- no cache hit is permitted for `options.txt` or JSA archives in v1.

The packaged Windows EXE gate must execute the release interposer with the feature off and prove exact current behavior, then with a synthetic opt-in fixture prove both a first-run stock-hash baseline and a second-run reuse path while comparing the produced plan bytes byte-for-byte.

## Compact physical probe for the future implementation

If a full candidate is not yet ready, the first executable probe should expose only aggregate counters, for example:

- `hash_stock_files`
- `hash_stock_bytes`
- `hash_reused_files`
- `hash_reused_bytes`
- `cache_miss_files`
- one coarse miss-reason count set

No paths, filenames, per-file times, hashes or arguments are written. The existing PR #19 sidecar remains the timing authority.

## Physical acceptance gate

No saving is claimed from hosted CI or hit counts.

Promotion requires a warm physical A/B on the HDD using:

- the same PR #7 valid Start->Java root contract;
- explicit `BOOTOPTIM_APPCDS_MODE=plan`;
- PR #19 preflight sidecar;
- one stock/off run and one otherwise-equivalent cache/on run after a valid baseline;
- byte-for-byte identical `launch-plan.json` and `launch-plan.sha256` between off/on for unchanged inputs;
- separate reporting of `appcds_preflight`, helper `build_launch_plan`, reused/stock aggregate bytes, and outer Start->Java;
- Java->menu excluded from this claim.

Any changed plan bytes, uncertain continuity evidence, or mutation negative-test failure is a NO-GO.

## Current decision

A secure hash-reuse candidate is architecturally viable, but implementing the skip safely on the PR #19 lineage requires first sharing the narrow authenticated NTFS/USN capability from PR #11 without importing its asset-repair contract. That shared capability boundary does not exist on this branch today.

Therefore this PR intentionally stops at a verifiable design instead of adding a second privileged helper or a metadata-only cache. The exact blocker is **code reuse/integration of the already-prototyped authenticated USN capability with the interposer's same-handle file hashing path**. Once that prerequisite is available, the minimal v1 is per-file digest reuse after a fresh collection/tree walk, default-off and fail-closed as specified above.
