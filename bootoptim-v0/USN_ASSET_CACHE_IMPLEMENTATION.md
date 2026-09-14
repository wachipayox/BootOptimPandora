# Agent 160 — USN asset-cache implementation foundation

Base: `agent159/usn-assets-cache-research-20260914@7fa54866d73146f7894b392ced5c8ee6bb6704da`.

This change deliberately stops at a **compilable fail-closed foundation**. It does not claim a cache hit, performance improvement, TTMM saving, or a requirement for administrator privileges. Pandora's existing asset SHA-1 verification remains authoritative on every launch.

## Why this PR does not partially wire the elevated path

`USN_ASSET_CACHE_ARCHITECTURE.md` requires four properties at the exact instant SHA-1 would otherwise be skipped:

1. a per-invocation elevated helper with the narrow volume-GUID/FileId/USN capability only;
2. an authenticated local-only named pipe with explicit DACL, random nonce and both peer-PID checks;
3. a Pandora-owned file handle opened with `FILE_FLAG_OPEN_REPARSE_POINT` while WRITE/DELETE sharing is denied;
4. journal + volume + FileId + USN evidence obtained while that same protected file object is still held, followed by a final identity re-check.

Landing only some of those pieces would create a dangerous intermediate state in which later code could accidentally treat partial metadata as permission to skip content hashing. The next implementation step is therefore one indivisible block: **helper + pipe authentication + same-handle verifier + atomic manifest publication + Windows mutation/TOCTOU tests**. Until that entire block exists and its tests pass, the production integration latch must remain false.

## Code in this foundation

`crates/backend/src/asset_usn_cache.rs` adds:

- explicit opt-in property `BOOTOPTIM_ASSET_USN_CACHE=1`; absence or any other value is off;
- schema-v1 cache structures for exact asset-index SHA-1, volume GUID/serial, USN journal identity/bounds and per-object SHA-1/FileId/last-USN;
- strict manifest validation: unknown/duplicate struct fields are rejected by Serde, schema and hash/FileId formats are checked, partial asset counts fail, duplicate object SHA-1s fail, invalid/negative USNs fail, malformed/truncated JSON fails;
- a pure reuse decision function requiring every identity/continuity/TOCTOU gate from the architecture;
- explicit miss reasons for UAC/helper/timeout/protocol/ACL/PID failures, non-NTFS, reparse/non-regular files, freeze-handle failure, index/volume/journal/FileId/USN changes, journal regression/truncation and final handle-identity change;
- `AssetUsnCacheRuntime::can_skip_sha1`, which is intentionally hard-wired to `false` in this PR. Setting the opt-in variable cannot change asset verification behavior.

The module is compiled into `backend`; it is not hooked into `do_asset_objects_load` yet because there is no complete privileged evidence provider. Consequently this branch performs exactly the same SHA-1 path as its base.

## Automated tests in this PR

Portable state-machine tests prove `FullSha1` for:

- feature off and explicit repair;
- same-size/restored-mtime mutation represented by changed file USN;
- truncate/restore represented by changed file USN;
- delete/recreate/FileId replacement;
- journal-ID restamp, `NextUsn` regression and lower-bound crossing the cached snapshot;
- asset-index, volume and final handle-identity changes;
- helper missing, UAC denial, helper crash, timeout, malformed protocol, invalid pipe ACL and invalid peer PID;
- non-NTFS, reparse, non-regular file and inability to acquire the write/delete-denying freeze handle;
- corrupt/truncated/unknown-schema/partial/duplicate cache manifests;
- the runtime safety latch remaining false even when the feature is explicitly requested.

The existing repository build workflow is the compile/test gate. No separate workflow is needed for the portable foundation.

## Required Windows tests in the next indivisible block

Before `can_skip_sha1` may ever return true, Windows CI/capability tests must use a temporary NTFS tree and prove all of the following against real handles and a real helper session:

- same-size content mutation with original mtime restored changes the file USN and causes stock SHA-1;
- truncate/restore causes stock SHA-1;
- delete/recreate cannot retain the accepted FileId identity;
- journal discontinuity/restamp fixture causes full-index miss;
- corrupt/partial cache cannot be consumed;
- helper absent, UAC cancelled/denied, helper crash, timeout and malformed protocol all continue launch through stock verification;
- reparse points and non-NTFS volumes are ineligible;
- explicit repair ignores a valid cache;
- with a writer/delete-compatible handle already open, inability to obtain the required protected handle forces stock SHA-1;
- while the candidate protected handle is held, conflicting write/delete cannot race the decision;
- baseline SHA-1 and FileId/USN snapshot are taken from the same protected handle, and hit identity is re-read before close;
- named pipe uses a random per-invocation name, `PIPE_REJECT_REMOTE_CLIENTS`, explicit DACL and validated helper/server PIDs;
- helper accepts only the selected local volume GUID and fixed-size FileIds, with no arbitrary path/content operations.

## Activation and fallback contract

The future candidate remains default-off. `BOOTOPTIM_ASSET_USN_CACHE=1` may only request the experiment; it never overrides an eligibility failure. UAC cancellation, helper absence/failure, invalid pipe/session evidence, non-NTFS/reparse, cache corruption, identity mismatch, I/O error or repair request must execute Pandora's existing complete asset SHA-1/download-repair behavior and must not block launch.

No service is installed and Pandora/Java must remain non-elevated. Only the ephemeral capability helper may receive the elevated token.

## Physical acceptance gate after the full block lands

Do not call the cache an optimization until Windows CI is green and the following is measured on the target HDD laptop:

1. first cache-enabled launch performs full stock SHA-1 and publishes a complete baseline only after every object is verified;
2. unchanged second launch records Start -> Java and `assets_verify_download` with the PR7 probe;
3. disposable same-size+mtime-restored mutation, delete/recreate and cache-corruption fixtures all force stock SHA-1/repair;
4. UAC cancel/helper absence still launches through stock verification;
5. journal reset/restamp testing is performed only on a disposable test volume/fixture;
6. compare the valid-cache result with scheduler 158 under comparable cold/warm cache state;
7. repeat on the fast PC/SSD to measure elevation/IPC overhead.

These are launcher Start -> Java measurements only. They are not Java-process -> menu / TTMM results. Administrator elevation must not become mandatory unless that physical comparison demonstrates a material launcher-only benefit that justifies the consent cost.
