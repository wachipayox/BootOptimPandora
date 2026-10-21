# Agent 198 — USN asset-cache promotion onto persistent-layout integration

Base authority: `agent/integration-current@3410b4c47eec0f47012d3c33bbd5e1d58b669e14`. The branch was first composed at `4aa955d87eaadecc407abe7a5737ff4469a877bf` and then refreshed by an explicit merge after integration advanced one documentation-only commit.

This is a clean recomposition, not a merge/rebase of the historical USN stack. The semantic source is the clean PR #29 slice, which in turn carried the reviewed PR #27 direct unprivileged NTFS implementation. PR #31's metadata-only fast-rebootstrap policy is deliberately excluded because this promotion contract requires SHA-1 fallback when cache/journal/volume identity is uncertain.

## Promotion decision

The cache is enabled by default for normal GUI Start. Set `BOOTOPTIM_ASSET_USN_CACHE=0` to disable it for diagnosis or rollback; `=1` explicitly enables it. This switch does not weaken the fail-closed verification contract: every uncertain identity/capability still uses stock SHA-1 verification.

The direct capability performs no UAC, helper, service, named-pipe IPC or privileged per-Start action. GUI `start_instance` is the only caller granted `AssetVerificationMode::Normal`. Default/CLI/legacy/unknown callers remain `FullVerification` and therefore execute stock SHA-1 verification.

## Integrity contract retained

SHA-1 may be omitted only for `VerifiedReuse` after all of these agree: asset-index SHA-1, expected object SHA-1, canonical launcher `assets/objects` scope, NTFS volume GUID/serial, journal ID and continuity bounds, FileId, file USN, protected regular no-reparse handle, and final same-handle identity.

Missing/corrupt/incompatible/partial manifest, non-NTFS or unknown volume, direct query/open failure, journal reset/regression/discontinuity, malformed USN data, reparse/nonregular object, FileId/USN mismatch, handle identity change, worker failure or publication uncertainty all retain the stock verification/download/repair path. Downloaded bodies remain size + SHA-1 checked before write. Explicit/full verification is never bypassed.

The manifest remains launcher-global under `assets/objects/.bootoptim-usn-assets-v1.json`; it is intentionally outside per-profile persistent layout state. Persistent profile locks do not authorize this cache. Concurrent complete manifests are atomically replaced; each later reuse still revalidates live volume/journal/FileId/USN, so a stale/racing complete snapshot can only lose reuse and fall back. Different asset-index identity also invalidates reuse.

## Historical evidence and claim boundary

The physical PR #27 pair established an assets-phase change from 163.319 s seed to 2.313 s reuse, i.e. **-161.006 s in `assets_verify_download` only**. The reuse run reported 3,911 direct USN verifications, zero asset-content SHA-1 reads, zero hash bytes and zero asset network/downloads. Mutation, delete/recreate and corrupt-manifest trials failed open to stock verification/repair. This is not a median and does **not** prove a -161.006 s TTMM saving.

Stock hot evidence had 3,911 SHA-1 asset reads / 824.9 MB and 90.292 s in `assets_verify_download`. Phase timings must not be added to inclusive Start→Java or Java→menu measurements.

## Metrics

Opt-in diagnostic sidecar: `BOOTOPTIM_ASSET_USN_PROBE=<fresh path>`.

Origin: `AssetUsnCacheSession::begin` at the canonical assets verification setup, after the asset index is known and before per-object verification tasks.

Endpoint: `AssetUsnCacheSession::finish` after all asset verification/download tasks complete.

The v2 sidecar reports requested/authority/layout state, direct capability mode (`direct_unprivileged_ntfs`), `elevation_requested=false`, session/publication state, and aggregate verified-reuse versus stock-SHA1 counts. It contains no asset paths, hashes, FileIds, USNs, credentials, URLs or command lines.

The existing launcher probe remains the separate Start→Java boundary. Java→menu/TTMM must be measured separately and never inferred from the asset sidecar.

## CI tier

Focused hosted gate only:
- Rust formatting for the composed cache/authority files;
- `cargo check -p bridge -p backend -p frontend --tests --frozen` on Linux;
- authority tests and backend asset-cache tests;
- Windows backend check + real NTFS direct-integration tests;
- restricted-token direct-capability probe;
- release launcher build only after the focused Linux/Windows semantic jobs pass.

CI cache/runtime duration is not performance evidence.

## Physical smoke and release follow-up

Use the exact checksummed Windows artifact and normal GUI Start on the HDD laptop, with the asset cache enabled by default (or explicitly `BOOTOPTIM_ASSET_USN_CACHE=1`) and a fresh USN probe path per run.

1. Seed: full SHA-1 baseline, direct capability active, no UAC/helper, manifest published.
2. Immediate unchanged reuse: 3,911 eligible reuses, zero stock asset SHA-1 reads/bytes and no asset network/downloads.
3. Same-size/restored-mtime mutation: affected object must fail closed to SHA-1/repair.
4. Delete/recreate: affected object must fail closed via FileId change.
5. Corrupt/truncated manifest: full stock SHA-1 fallback and safe republish.
6. Hosted tests cover journal discontinuity/unknown volume; do not destructively reset the user's real USN journal merely for smoke.
7. Repeat capability seed/reuse under a genuine standard non-admin Windows account.
8. Record `assets_verify_download`, Start→Java and Java→menu independently. Do not claim TTMM from the historical -161.006 s assets-phase observation.
