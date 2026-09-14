# Agent 160 — authenticated NTFS/USN asset-cache runtime

Runtime base: `agent161/asset-verification-intent-20260914@635171f4241320c0de4507fbb67ed45f1cfabd17` (PR #10). That base contains PR #9's fail-closed manifest/identity foundation and the stock-owned `AssetVerificationMode::{Normal, FullVerification}` propagation needed to distinguish eligible normal launches from full verification.

This runtime remains **default-off**, Windows/NTFS-only and experimental. It does not claim a Start -> Java saving, TTMM saving, or a requirement for administrator privileges. Only the narrow metadata helper is elevated; Pandora and Java/Minecraft remain non-elevated.

## Exact authorization boundary

`AssetUsnCacheRuntime::can_skip_sha1()` can return true only when all of the runtime and evidence gates have already produced `ReuseDecision::VerifiedReuse`:

- `BOOTOPTIM_ASSET_USN_CACHE=1`;
- `AssetVerificationMode::Normal` exactly;
- canonical launcher-managed `.../assets/objects` layout (not `resources` and not `virtual/legacy`);
- exact asset-index SHA-1 and expected Mojang object SHA-1;
- local filesystem exactly NTFS;
- protected Pandora file handle opened with `FILE_FLAG_OPEN_REPARSE_POINT` and only `FILE_SHARE_READ`, so write/delete sharing is denied during the decision;
- no final-component reparse point and a regular file;
- exact volume GUID and serial;
- exact `UsnJournalID` and provable journal continuity;
- exact NTFS FileId and file USN;
- unchanged identity when Pandora re-reads the still-open handle immediately before acceptance.

Size, mtime and path existence are never acceptance inputs. Any unavailable, malformed, ambiguous or mismatching evidence executes Pandora's SHA-1 path.

`FullVerification`, including PR #10's default/legacy/CLI/unknown launch paths, never consumes the cache. A future explicit repair action must remain `FullVerification` at its authoritative origin.

## Elevated helper and trust root

`bootoptim-usn-helper` is a Windows GUI-subsystem helper launched per eligible session with `ShellExecuteExW(..., "runas", ...)`. It is not a service and installs no driver, task, registry autorun or persistent privileged process.

The helper capability is deliberately narrow:

- one exact local `\\?\Volume{GUID}\` per session;
- fixed-size 16-byte FileIds only;
- `FSCTL_QUERY_USN_JOURNAL`;
- `OpenFileById`;
- `FSCTL_READ_FILE_USN_DATA`;
- metadata responses only.

There is no file-path, content, directory-enumeration, write, delete, rename, registry, network or command-execution request.

The runtime helper path comes from `BOOTOPTIM_ASSET_USN_HELPER`, but that pathname is **not** a trust root. The Windows build first compiles the helper, computes its SHA-256 and compiles Pandora with that digest in `BOOTOPTIM_ASSET_USN_HELPER_SHA256_PIN`. At runtime `BOOTOPTIM_ASSET_USN_HELPER_SHA256` must be strict lowercase SHA-256 and must equal the embedded pin. Pandora then hashes the actual helper bytes while holding a read handle that denies write/delete replacement; those bytes must also match the embedded pin before elevation. A missing/invalid build pin, runtime mismatch, reparse/directory helper, open/hash failure or replacement ambiguity falls back before elevation.

## Pipe authentication and bounded failure

Each helper invocation gets a cryptographically random nonce and named pipe. Pandora creates the pipe with `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE` and a protected DACL limited to the current user plus Administrators and SYSTEM. Pandora checks the connected client PID against the exact process handle returned by `ShellExecuteExW`; the helper checks the named-pipe server PID. Both sides also validate fixed protocol magic/version, nonce and handshake PID.

Messages are fixed-size. Connect and response waits are bounded. Helper/session failure is latched for the launch so a failed helper cannot cause one timeout per asset.

Teardown sends the shutdown message best-effort and closes the pipe. It intentionally does not call `FlushFileBuffers`: waiting for a hung helper to consume shutdown bytes would violate the stock-fallback contract.

## Journal race closure

A file-USN response must not combine metadata from two journal eras. For every `File` request the helper:

1. queries and validates the journal state;
2. opens the FileId and reads its current USN;
3. queries the journal again;
4. requires the same nonzero `UsnJournalID` and nondecreasing `FirstUsn`, `LowestValidUsn` and `NextUsn`;
5. returns the **second** journal state with the file USN.

A journal reset/restamp, regression, invalid lower bound or query failure inside that window returns a capability failure and therefore stock SHA-1.

## Same-handle baseline and reuse

On a cache miss, Pandora hashes an existing eligible object through the same protected handle used for identity checks. A valid SHA-1 may then be snapshotted through the helper while the handle is still held, followed by a final handle-identity reread.

If an object needs download/repair, Pandora's existing downloader remains authoritative. The protected handle is released before the write. After all normal asset tasks finish, the final object is reopened and must pass full SHA-1 plus the protected-handle/FileId/USN sequence before entering a baseline.

On a candidate hit, Pandora holds the protected handle while the helper obtains current journal/FileId/USN evidence and then re-reads the handle identity. Content SHA-1 is omitted only after the pure decision function returns `VerifiedReuse` and the runtime gate confirms normal mode + explicit opt-in.

## Manifest publication

The schema-v1 manifest is untrusted input and is accepted only for the exact asset-index SHA-1, exact volume identity and complete set of unique physical Mojang object hashes. Unknown schema/fields, malformed hashes/FileIds, negative or inconsistent USNs, duplicate/partial entries and truncated/corrupt JSON invalidate reuse.

A new manifest is published only after every expected physical object has a valid snapshot and a final journal query still belongs to the initial journal. Publication uses a random create-new temp file, `sync_all`, then `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`. Any capability failure prevents publication.

The v1 runtime is restricted to the canonical `assets/objects` layout, so it never creates USN state inside `resources` or `virtual/legacy` trees.

## Automated gates

Portable tests cover the complete decision matrix, manifest rejection, normal-vs-full-verification intent, fixed-size protocol/path-channel rejection and journal transition consistency.

Windows tests/capability checks cover, where the hosted filesystem supports them:

- protected-handle writer/delete exclusion;
- same-size content mutation with restored mtime changing real NTFS USN;
- truncate/restore with restored mtime changing real NTFS USN;
- delete/recreate changing real NTFS FileId;
- reparse-point ineligibility;
- strict lowercase helper-digest syntax.

The cross-platform build workflow runs the cache/protocol/intent tests. On Windows it builds the helper first, computes its SHA-256, injects that digest into the Pandora build and then performs the full release build. The BootOptim v0 workflow likewise packages the matching helper, Pandora executable and `SHA256SUMS.txt` for the physical gate.

Hosted CI cannot exercise interactive UAC consent/cancellation or destructively restamp the host journal. Those remain physical/disposable-fixture gates rather than claimed hosted coverage.

## Physical acceptance gate

Do not call this an optimization win until current-head Windows CI is green and the target-machine protocol has been completed.

1. Use the matching Pandora/helper pair produced by CI and verify `SHA256SUMS.txt`.
2. Run stock and candidate from comparable asset/cache states; candidate activation requires `BOOTOPTIM_ASSET_USN_CACHE=1`, the packaged helper path, and the exact helper SHA-256.
3. The first eligible candidate run must still perform complete SHA-1 and may publish a baseline only after successful verification.
4. Only the unchanged second run is a reuse candidate.
5. Exercise disposable same-size+mtime-restored mutation, truncate/restore, delete/recreate, corrupt manifest, helper absence/hash mismatch, UAC cancel, timeout/malformed protocol and journal restamp/discontinuity; every case must fall back to stock SHA-1/repair.
6. Confirm `resources` and `virtual/legacy` remain stock-only and produce no cache state.
7. Compare HDD Start -> Java / `assets_verify_download` against stock and PR #8 under equivalent cold/warm states, separating unmatched repair/download work.
8. Repeat on SSD/fast PC to measure elevation/IPC overhead.

These measurements are launcher Start -> Java only. They are not Java-process -> menu / TTMM evidence, and no mandatory-admin claim follows from this implementation.