# Agent 199 — AppCDS incremental identity: no-UAC persistence blocker

Base authority: `agent/integration-current@3410b4c47eec0f47012d3c33bbd5e1d58b669e14`, refreshed before this investigation on 2026-09-21.

## Scope

This note covers only the AppCDS launch-plan identity rebuild cost. It does not implement or change:

- asset verification (Agent 198);
- immediate TRAIN state-machine policy (Agent 200);
- profile namespace selection (Agent 201);
- Java-to-menu behavior or any TTMM claim.

The requested end state is a versioned local identity manifest which can reuse the **existing canonical launch-plan digest bytes** for unchanged files while failing closed to the stock full SHA-256 path on any uncertainty.

## Current integration behavior

The current interposer still constructs the full strong launch plan before taking the AppCDS cache lock.

For every preflight it reads/hashes:

- Java executable and `<java-root>/release`;
- every final classpath entry in JVM order;
- every final module-path entry in JVM order;
- every top-level mod JAR;
- every regular file rediscovered under `config/`, `defaultconfigs/`, `kubejs/`, and `scripts/`;
- Pandora and the interposer itself;
- `options.txt` resource-pack selection is separately read/parsed.

The final plan representation already has the right semantic boundary: unchanged inputs must keep the exact current artifact sizes/digests/order/canonicalized config digests so the resulting `launch-plan.json` bytes are unchanged.

The physical task evidence reports roughly 1.23 GiB reread and 168–377 s in `appcds_preflight` on the HDD laptop. PR #19 independently attributed the dominant preflight cost to `build_launch_plan` strong hashing rather than plan persistence/locking. These are launcher/pre-Java costs only. They are not Java-to-menu or TTMM results.

The requested BootOptim evidence path `docs/research/pandora-prejava-probe-2026-09-14.md` is not present in the current `wachipayox/BootOptim` `agent/integration-current` recursive tree, and direct/default-branch path history also returns no file. The numeric physical evidence supplied for this task and the PR #19 attribution remain sufficient to identify the bottleneck, but this branch does not invent a replacement copy of that missing research file.

## Prior AppCDS cache lineage

PR #20 defined the correct cache unit as one regular-file artifact keyed by exact role + encoded path, with fresh collection discovery every launch.

PR #21 extracted an NTFS/USN capability with the right security semantics:

- protected non-elevated file handle;
- NTFS volume identity;
- FileId;
- file USN;
- journal identity/bounds continuity;
- same-handle final identity;
- full-hash fallback on every capability uncertainty.

PR #23 added an explicit GUI launch authority on top of that lineage.

However, PR #21's `CapabilitySession::launch` starts `bootoptim-usn-helper.exe` through `ShellExecuteExW(..., "runas", ...)` for each capability session. Porting that runtime directly into the current short-lived preflight would therefore require elevation for each Start, which this task explicitly forbids.

## Why the obvious no-UAC substitutes are not sufficient

### FileId alone

FileId is excellent for delete/recreate/replacement detection, but an in-place write keeps the same file object/FileId. Therefore `volume + FileId + size` cannot authorize digest reuse.

### Size / LastWriteTime / ordinary metadata

The task explicitly rejects optimistic metadata reuse. A same-size write with restored timestamps must still miss. These fields can only be redundant consistency checks.

### Per-file USN without journal identity

`FSCTL_READ_FILE_USN_DATA` can expose the file's last USN, but a cached numeric USN is meaningful only inside a known continuous journal generation.

Microsoft documents that the NTFS journal has a 64-bit identifier which changes when existing USN records may become unusable, and that deleting/recreating the journal restarts USN numbering. The current journal identifier is obtained with `FSCTL_QUERY_USN_JOURNAL`.

Microsoft also documents that change-journal operations require administrator privileges. Therefore a normal per-Start process cannot prove the required `journal_id / lowest_valid_usn / next_usn` continuity after launcher/process restart without a privileged broker.

References:

- <https://learn.microsoft.com/en-us/windows/win32/fileio/using-the-change-journal-identifier>
- <https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_query_usn_journal>
- <https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_read_file_usn_data>

### ChangeTime is not a journal-equivalent authority

NTFS `ChangeTime` is useful diagnostic metadata, but Windows exposes `FILE_BASIC_INFO` through `SetFileInformationByHandle`; setting basic timestamps requires `FILE_WRITE_ATTRIBUTES`, not administrator-only journal authority. It therefore cannot replace journal-generation continuity for a fail-closed persistent digest cache.

References:

- <https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_basic_info>
- <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle>

## Verified blocker

With the current product constraints, there is no safe persistent cross-process fast path to implement:

1. the manifest must survive Start/process boundaries;
2. unchanged content digests may be reused only from a reliable file/volume change proof;
3. journal reset/discontinuity must force stock full hashing;
4. Start must not request UAC;
5. no already-installed privileged broker/service exists in the current integration branch.

Using PR #21 as-is violates (4). Omitting journal identity violates (2) and (3). Size/mtime/ChangeTime-only reuse violates the explicit fail-closed contract.

For that reason this branch deliberately does **not** add a metadata-only manifest or skip a single production SHA-256 read.

## Safe implementation choices that would unblock the manifest

Exactly one additional product/architecture decision is required.

### A. Long-lived privileged journal broker installed/authorized once

A narrowly scoped service or equivalent broker can expose only:

- current NTFS volume serial + journal ID/bounds;
- FileId + file USN for a caller-held protected handle / bounded request;
- no arbitrary file read/write and no asset/update policy.

The launcher/interposer remains non-elevated. Installation/authorization may require one explicit administrative action, but normal Start does not.

With that primitive, the v1 manifest can be the PR #20 unit:

`schema + role + encoded_path + digest + size + volume_serial + journal_id/bounds + FileId + file_usn`.

Every launch still rediscovers classpath/module-path/mod/config membership in current order, then reuses only records whose exact evidence remains continuous. Any broker error, volume mismatch, journal restamp/regression/discontinuity, FileId/USN mismatch, reparse ambiguity, manifest corruption, update/repair/download signal, or protected-handle mismatch executes the current full hash.

### B. Session-local non-elevated watcher cache

Pandora can start `ReadDirectoryChangesW` watchers before the first strong baseline and attach a random launcher-session epoch plus monotonic dirty generation to the manifest. Buffer overflow, watcher loss, unsupported directories, process restart, update/repair/download, or any uncertainty forces full hash.

This can make second/subsequent Starts in one uninterrupted launcher session incremental, but **cannot safely reuse the persistent manifest after Pandora/Windows restart**. The first Start of every new watcher session remains O(total pack bytes), so this is materially weaker than the stated persistent product requirement.

### C. Relax the journal-continuity requirement

Not recommended. A `FileId + per-file USN + metadata` cache could be faster without elevation, but cannot prove journal-generation continuity and therefore does not satisfy the task's fail-closed contract.

## Manifest, locking, and publication once option A exists

No profile-namespace policy is defined here; Agent 201 owns that composition. The AppCDS code only needs a caller-supplied cache directory.

The safe publication order is:

1. perform fresh collection discovery and reparse/type checks;
2. for every artifact, hold a protected handle while deciding reuse or computing the stock digest;
3. build the **same current canonical launch-plan bytes**;
4. take the existing AppCDS cross-process cache lock;
5. revalidate any manifest-generation/broker session evidence required by the chosen capability;
6. atomically replace a versioned `identity-manifest.v1` only after the complete plan build succeeds;
7. publish the existing `launch-plan.json`, SHA, match and state transitions under the same lock;
8. on lock contention, manifest corruption, incomplete staging or publication error, return STOCK and do not promote partial cache state.

A corrupt or unknown-schema manifest is a cache miss, not an activation error. The strong stock builder remains the semantic source of truth.

## Required tests for the eventual implementation

The production candidate must prove:

- unchanged inputs: stock and cached builders emit byte-for-byte identical `launch-plan.json`;
- same-size mod/config/classpath write: no stale digest reuse;
- classpath and module-path order unchanged;
- config canonicalization keeps the exact current digest bytes;
- deletion/recreation: FileId mismatch -> full hash;
- rename/path change: no record match -> full hash;
- journal ID restamp -> full hash;
- lower-bound discontinuity / NextUsn regression -> full hash;
- reparse/symlink/non-regular input -> existing fail-closed eligibility;
- corrupt/truncated/unknown/partial manifest -> full hash;
- update/repair/download signal -> full hash for the affected launch;
- lock contention -> STOCK, no partial manifest publication;
- crash between staging and replace -> old complete manifest remains authoritative or cache is treated as miss.

## Cost model, separate from TTMM

Current unchanged Start cost is still O(total bytes hashed). On the supplied HDD case that means roughly 1.23 GiB of content reads and the observed 168–377 s `appcds_preflight` range.

With option A and an unchanged pack, expected content-read cost becomes O(changed bytes) after fresh O(number of discovered files) metadata/evidence queries. Classpath/module-path/config tree discovery still occurs because collection membership/order remains part of identity. No numerical startup saving is claimed until a physical A/B exists.

With option B, the same asymptotic reduction is possible only after a full baseline inside one uninterrupted launcher session; first Start after launcher/process restart remains full-hash.

These are launcher/pre-Java cost expectations only. They are explicitly not TTMM, Java-to-menu, AppCDS archive-consumption, or gameplay performance claims.
