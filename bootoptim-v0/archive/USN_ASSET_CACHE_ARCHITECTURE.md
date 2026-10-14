# Agent 159 — Windows NTFS USN asset-cache architecture

Authority/base: `agent155/launch-prejava-probe-20260914@48b12855f054302d53a305f1eab9e768508fb3e2` (PR #7).

This is an architectural result, not an enabled cache and not a timing claim. `AGENTS.md` is absent at the exact base snapshot. The repository README, `bootoptim-v0/README.md`, `bootoptim-v0/CONFIG_IDENTITY.md`, `bootoptim-v0/ROADMAP.md`, and PR #2-#7 were reviewed before this decision.

The physical PR7 HDD observation motivating this work is launcher-only: Start -> `java_spawn` 742.966 s, assets 649.015 s and libraries 614.602 s overlapping. It is not Java-process -> menu / TTMM evidence.

## Decision

Select architecture **(1), an ephemeral elevated capability helper**, for the first physical candidate. Do not install a service and do not elevate the whole launcher yet.

The normal Pandora process remains `asInvoker`. Java/Minecraft is still spawned by that non-elevated Pandora process exactly as before. Only a short helper process receives an Administrator token, after explicit UAC consent, and only while `BOOTOPTIM_ASSET_USN_CACHE=1` (or an equivalent future explicit default-off switch) is active.

The helper's filesystem capability is intentionally smaller than a generic privileged file service:

- accept one local NTFS **volume GUID**, never an asset pathname;
- accept fixed-size NTFS file IDs, never filenames or arbitrary paths;
- query `FSCTL_QUERY_USN_JOURNAL` on that volume;
- open a file by ID with `OpenFileById` only to query its current USN with `FSCTL_READ_FILE_USN_DATA` and identification metadata;
- return only filesystem type/volume identity, journal identity/bounds, file ID and current USN/status;
- no file-content reads, directory enumeration, writes, deletes, rename, registry mutation, network access or command execution.

Microsoft explicitly recommends separating narrowly elevated operations into a helper rather than unnecessarily elevating the main application. `ShellExecuteEx`/`runas` provides the UAC boundary. `FSCTL_QUERY_USN_JOURNAL` is the documented source of the journal identifier and requires Administrator privileges. `FILE_ID_INFO` and the NTFS 64-bit file index provide stable file identity while the file exists, and `OpenFileById` can open the object by that identifier without a pathname.

References:

- https://learn.microsoft.com/windows/win32/secbp/running-with-administrator-privileges
- https://learn.microsoft.com/windows/win32/fileio/using-the-change-journal-identifier
- https://learn.microsoft.com/windows/win32/api/winioctl/ni-winioctl-fsctl_read_file_usn_data
- https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-openfilebyid
- https://learn.microsoft.com/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information
- https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_id_info

## Why not a service yet

A persistent service avoids one UAC consent per cache-enabled launch, but it creates a permanently privileged local IPC endpoint, installer/update/uninstall ownership, service ACLs, binary-replacement rules and version-skew handling. That is a materially larger security and maintenance surface than needed to prove whether USN reuse is worthwhile on the HDD.

Only consider architecture (2) after the ephemeral-helper candidate is physically faster enough to justify removing repeated UAC consent. If that happens, the service must reuse the same narrow protocol and must additionally pin the connecting launcher identity/version and use an installer-owned, Administrator/SYSTEM-writable location.

## Why not elevate Pandora

Elevating Pandora would expand the privileged attack surface to UI, HTTP metadata/download handling, modpack management and all other launcher code. More importantly, a normal child process inherits the elevated token, so Java would also become elevated unless a separate de-elevation/spawn mechanism were added and proven. That conflicts with this experiment's requirement that Minecraft/mods remain non-administrator.

Architecture (3) therefore has no current justification. The narrow helper avoids both the wider privileged code surface and the extra complexity of de-elevating Java.

## IPC and ACL contract

The candidate should use one single-instance local named pipe per invocation.

1. Non-elevated Pandora creates a cryptographically random pipe name, for example `\\.\pipe\BootOptimPandora-USN-v1-<256-bit nonce>`.
2. The pipe is local-only (`PIPE_REJECT_REMOTE_CLIENTS`) and has an explicit DACL for the interactive logon SID plus SYSTEM/Administrators as required for the elevated same-user token. Do **not** use the default named-pipe DACL: Microsoft documents that it grants read access to Everyone and anonymous users.
3. Pandora launches the exact helper binary with `ShellExecuteEx` / verb `runas`. No Java command, account data, instance path or asset path is passed to it.
4. The helper connects to the pipe and validates protocol magic/version and the random nonce. The launcher validates the connected client PID against the process returned by the elevation launch. The helper validates the named-pipe server process ID before accepting requests.
5. Message lengths, item count and response sizes are hard bounded. Unknown protocol fields/versions are fatal to the capability session and cause stock verification.
6. The helper accepts only a syntactically valid local volume GUID (`\\?\Volume{GUID}\`) plus fixed-size file IDs. It must reject drive-relative paths, UNC paths, device paths other than the resolved volume, and every string that could name a file below the volume.
7. The helper verifies the opened volume reports NTFS before issuing the USN operations. ReFS, SMB/UNC, CSV, FAT/exFAT and any unknown filesystem are ineligible in v1 even where individual APIs might exist.

The IPC is metadata-only. No contents or arbitrary user paths cross the privilege boundary.

Microsoft named-pipe security reference:
https://learn.microsoft.com/windows/win32/ipc/named-pipe-security-and-access-rights

## Cache identity v1

Persist only information needed for the decision; asset object paths are derivable from their Mojang SHA-1 and therefore are not persisted.

Top-level identity:

- schema/version = 1;
- exact asset-index SHA-1 used by Pandora;
- resolved volume GUID and NTFS volume serial identity;
- USN journal ID;
- journal `NextUsn` at the successful stock-verification snapshot boundary;
- the journal validity lower bound needed to prove the later query has not crossed a reset/deletion/truncation boundary.

Per asset object:

- expected Mojang asset SHA-1 (also the cache key);
- NTFS file ID;
- last file USN captured only after that exact object passed stock SHA-1 verification.

Size and mtime are never acceptance inputs.

The cache file is **untrusted input**. Unknown schema, duplicate keys/objects, malformed hex, integer overflow, missing entries, extra ambiguous identities or parse/truncation errors invalidate the whole manifest and run the stock asset path. Publication must be temp-file + flush/sync + atomic rename only after every asset in the index has a complete validated snapshot. Partial snapshots are never reusable.

A future production threat model should decide whether the cache also needs an Administrator-owned ACL/MAC. The current launcher executable and instance data are user-controlled, so the v1 physical experiment does not claim protection from a malicious process already executing as that same user; it must nevertheless detect accidental/corrupt cache bytes and every filesystem mutation case listed below.

## TOCTOU contract

A path-stat followed by a later USN query is insufficient. Baseline and fast-path decisions must be tied to an open file object.

For each asset candidate Pandora must open the object itself with `FILE_FLAG_OPEN_REPARSE_POINT` and a share mode that **does not permit write or delete** for the lifetime of the decision. If that open cannot be obtained (including a pre-existing incompatible writer), reparse status is present/ambiguous, the object is not a regular NTFS file, or identity calls fail, this object uses the stock SHA-1 path.

### Baseline/full verification

For a cache miss/ineligible launch:

1. open the asset handle with write/delete sharing denied;
2. reject reparse/unsupported type;
3. hash the unnamed data stream through that **same handle** using Pandora's expected SHA-1;
4. while the handle is still open, obtain volume serial + NTFS file ID;
5. query the helper for journal state and current USN for that file ID;
6. re-read handle identity before closing; any mismatch/error discards the snapshot;
7. only after the complete asset index has passed stock verification/repair may the manifest be published.

If an asset needed download/repair, the normal downloader remains authoritative. The repaired final file must satisfy the same full SHA-1 + same-handle snapshot sequence before it can enter a new cache manifest.

### Reuse/fast path

For each cached object:

1. open the expected hash-derived asset path with write/delete sharing denied and `FILE_FLAG_OPEN_REPARSE_POINT`;
2. require regular-file/no-reparse and the exact cached volume serial + file ID;
3. keep that handle open while the elevated helper obtains the current file USN for the same ID;
4. require exact file-USN equality;
5. require asset-index SHA equality and the volume/journal continuity gates below;
6. re-check handle identity before accepting;
7. only then may the content read/SHA-1 for that object be skipped.

Because any existing writable/deletable conflicting handle prevents acquiring the required freeze handle, concurrent external mutation cannot race between the USN decision and reuse. Uncertainty returns to the stock SHA-1 behavior, never to an optimistic hit.

## Journal continuity gate

A cache hit requires all of the following:

- same volume GUID/serial;
- filesystem exactly NTFS;
- same `UsnJournalID`;
- current `NextUsn` is not less than the cached snapshot `NextUsn`;
- the cached snapshot boundary remains inside the journal's currently valid range (`FirstUsn` / `LowestValidUsn` as appropriate to the returned `USN_JOURNAL_DATA` version);
- all queried file USNs are valid, nonnegative and exactly equal to their cached value.

A journal ID change is a hard miss. Microsoft documents that NTFS restamps this ID when old records are or may be unusable. A reset/delete/recreate, regression, unsupported journal version, lost snapshot boundary or any condition in which continuity cannot be proven is a hard miss for the entire index.

The first implementation should intentionally be conservative about journal wrap/truncation even if a weaker proof could be argued safe: if the snapshot boundary has fallen below the current valid lower bound, run stock SHA-1 for the index and create a new baseline.

## Explicit fail-closed cases

All of these bypass cache reuse and execute Pandora's existing complete asset verification/repair behavior:

- feature absent/default-off;
- user explicitly selected repair/full verification;
- UAC denied/cancelled;
- helper missing, wrong version/hash, failed to start, crashed, timed out or returned malformed data;
- IPC/ACL/peer-PID validation failure;
- non-Windows or non-NTFS filesystem;
- remote/UNC/CSV/ReFS/unsupported volume;
- no USN journal or any USN query failure;
- journal ID change/reset/deletion;
- journal validity/truncation/continuity ambiguity;
- asset-index SHA mismatch;
- cache missing/corrupt/partial/unknown schema;
- asset missing;
- reparse point;
- file ID or volume identity change;
- file USN change;
- inability to acquire the write/delete-denying file handle;
- any identity mismatch before vs after decision.

Repair explicitly ignores a valid cache and does not silently convert into a fast-path launch.

## Required tests before a cache PR

Pure/state-machine tests must prove the decision function returns `FullSha1` for:

- same-size content mutation with mtime restored;
- truncate then restore length/mtime;
- delete/recreate at the same path;
- file-ID replacement;
- journal ID restamp;
- `NextUsn` regression;
- journal lower bound crossing the cached snapshot boundary;
- manifest/index SHA change;
- corrupt/truncated/duplicate cache entries;
- helper missing, UAC denied, helper nonzero exit/timeout, malformed response;
- reparse point;
- non-NTFS volume;
- explicit repair.

Windows integration tests must additionally mutate a temporary NTFS file and demonstrate that the file USN changes even when original size and mtime are restored; delete/recreate must not retain the cached identity. These tests establish API behavior but are not a performance result.

A TOCTOU test must hold an asset handle in the candidate's required share mode and show that an attempted write/delete cannot complete until the decision handle closes (or that inability to acquire that protection forces `FullSha1`).

## Physical gate

Do not enable this by default and do not call it an optimization win until all of the following have been measured on the target HDD laptop.

1. Build the branch with the helper and tests green on Windows.
2. Run one cache-enabled launch that necessarily performs full stock SHA-1 and publishes a baseline only after successful verification.
3. Without changing the pack, run the next launch and record PR7's Start -> Java and `assets_verify_download` boundaries.
4. Mutate one copied/test asset same-size and restore its timestamp; the next launch must perform full SHA-1 for that object/index and repair/reject it. Do not mutate the productive pack for the gate if a disposable assets tree can be used.
5. Exercise delete/recreate and a cache-corruption fixture; both must force stock verification.
6. Exercise UAC cancel/helper absence; launch must continue through the stock asset path.
7. Exercise a journal restamp/reset only on a disposable test volume/fixture, never by destructively resetting the user's production journal; it must force stock verification.
8. Compare the valid-cache HDD run with the unchanged-hash scheduler candidate from agent 158 under comparable cold/warm cache state. The USN path is justified only if the saved content reads exceed UAC/helper/metadata overhead materially.
9. Repeat on the fast PC/SSD to reject a candidate whose elevation/IPC overhead is unacceptable.

These measurements are launcher Start -> Java only. No TTMM claim follows from them.

## Upgrade, consent and uninstall path

For the ephemeral-helper experiment there is no service installation.

- Feature remains default-off and is activated explicitly for the physical gate.
- Each cache-enabled launch that needs privileged USN data produces one normal Windows UAC consent. Denial immediately selects stock verification and still launches the game.
- Launcher/helper protocol versions must match exactly. A newer/older helper is a stock fallback, never best-effort compatibility.
- Helper binary is shipped side-by-side with the matching test launcher and should be hash/version pinned by the caller before elevation.
- Uninstall is removal of the test launcher/helper plus the cache manifest. No service, scheduled task, registry autorun, driver or system journal setting is installed or modified.

If physical evidence later justifies a service, installation and removal must be explicit elevated user actions. The service binary/config directory must be writable only by SYSTEM/Administrators; the IPC ACL must be narrowed to the intended interactive user/logon SID; update must be atomic and version-pinned; an absent/stopped/version-mismatched service must simply make the cache ineligible.

## Implementation boundary

No cache implementation is committed by this report. The helper design is viable enough to proceed, but mixing a partially implemented elevated IPC/cache with PR7 would be less safe than preserving the exact fail-closed boundary above. The next implementation PR should contain the helper, IPC, same-handle asset verifier, manifest state machine and Windows mutation tests together so no intermediate commit can accidentally skip SHA-1 without the privilege/TOCTOU gates.