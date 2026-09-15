# Agent 173 — USN privilege architecture decision

## Context

PR #24 physically proved the v1 asset cache can authenticate an unchanged 3,911-object baseline and omit the corresponding content SHA-1 reads, but its metadata-only helper is launched with `runas` for every eligible session. That makes every normal Start request UAC consent. This branch decides whether to persist privileged authority and implements the safer alternative found while auditing that premise.

This branch starts at PR #24 head `28b997a8adbd3ae71bc1fa73c15b1c5357bec046`. It does not make a TTMM claim and does not change BootOptim integration.

## Decision

**Do not install a privileged Windows service or scheduled task for the v1 USN cache.** The minimum required metadata is available through handles Pandora can own directly, so persistent privilege is unnecessary authority.

The runtime candidate now uses a non-elevated capability:

1. Pandora opens the candidate asset itself with the existing write/delete-denying protected handle and rejects directories/reparse points/non-NTFS exactly as before.
2. `FSCTL_READ_FILE_USN_DATA` is issued on that same protected file handle. No `OpenFileById`, path channel or elevated process is required for the per-file USN.
3. Pandora opens the containing NTFS volume with only `FILE_TRAVERSE` and shared read/write/delete, then uses `FSCTL_QUERY_USN_JOURNAL` before and after the per-file query.
4. The existing manifest acceptance rules remain unchanged: exact volume GUID/serial, journal ID and continuity bounds, FileId, file USN and final same-handle identity are still required before `VerifiedReuse` can omit SHA-1.
5. Any access denial, unsupported filesystem/OS behavior, malformed/short response, journal inconsistency or handle uncertainty falls back to stock SHA-1. It does not trigger an automatic UAC prompt.

A dedicated probe crate on this branch exercises the same Windows capability under a restricted token with Administrators made deny-only and maximum privileges disabled. The active Windows runtime has also been rewired away from the helper; this remains a candidate until the targeted Windows gate and later physical non-admin/HDD gate pass.

## Why a persistent service/task is rejected

A service or highest-privilege scheduled task would eliminate repeated consent only by turning a per-launch ephemeral capability into installed privileged authority. That introduces a materially larger trust and maintenance surface:

- installation/uninstallation must be privileged and crash-safe;
- executable/config directories require machine-writable ACL rejection and protected ownership;
- every normal client connection needs authentication stronger than “local/same user”, otherwise any user process can query the broker;
- binary identity, protocol version and client policy need coordinated rotation;
- update must prevent old-client/new-service and new-client/old-service ambiguity;
- downgrade must be explicitly blocked or made fail-closed, otherwise an older signed/pinned client could reacquire broader behavior;
- a scheduled task additionally has mutable task definition/action/arguments as authority; a service has SCM configuration and a long-lived attack surface;
- orphaned service/task state after launcher removal is a product-security defect, not just clutter.

The current v0 launcher has no service/task installer/updater lifecycle that can own those invariants. Adding a privileged broker before that product boundary exists would expand scope solely to work around a privilege requirement that the direct-handle design does not need on the tested Windows capability path.

If a future Windows version or policy makes the direct capability unavailable on a supported machine, v1 falls back to stock SHA-1. Reconsider a broker only if there is then a measured, supported-machine need large enough to justify a real installer/updater security model.

## Threat model for the direct path

### Untrusted manifest

The existing cache manifest remains advisory input. It never authorizes a hit without fresh kernel evidence. Corruption, partiality, duplicate entries, unknown schema or identity mismatch remains stock verification.

### TOCTOU

The asset file stays open with write/delete sharing denied while its current FileId/USN is obtained and while final handle identity is reread. The direct capability improves the boundary by querying the same file handle instead of asking another process to reopen the FileId.

### Journal reset/truncation

A journal query is taken before and after the file-USN query. ID changes, regression or lower-bound discontinuity remain hard misses. No timestamp/size heuristic is introduced.

### Spoofing / IPC

The active direct path has no privileged IPC endpoint, so helper path/hash, pipe nonce, peer PID or helper binary identity cannot authorize a normal-launch reuse decision. Historical helper code may remain in ancestry/packaging while this branch is validated, but the production path must not silently fall back to it or prompt for UAC.

### Update / rotation / uninstall

The direct capability has no separately installed authority to rotate or uninstall. It ships as ordinary Pandora code. Updating replaces Pandora by the launcher's normal update mechanism; old/new cache records are still constrained by schema and exact runtime evidence. Removing Pandora leaves only the unprivileged manifest beside launcher-managed assets, which is already untrusted and can be ignored/deleted without OS cleanup.

## Compatibility boundary

The candidate stays Windows + NTFS only. Other filesystems, unsupported record versions, inability to open the volume with the minimal access mask, unavailable journal, enterprise policy changes, protected-handle failure or unexpected kernel responses are ordinary cache misses.

Hosted Windows proves only API/semantic behavior. The final supported-machine gate must include a real non-admin account on Windows 10/11 (not merely an administrator token with UAC filtering) before promotion.

## Validation tiers

Per the Pandora iteration rule:

- **Tier 1:** format/check/unit tests for the affected Rust backend plus the restricted-token capability probe. This proves compilation, pure fail-closed rules and the direct Windows API sequence only; it is not performance evidence and is not a physical non-admin test.
- **Tier 2:** targeted Windows dev semantics for the wired `AssetUsnCacheSession`: direct seed/reuse plus mutation/fallback behavior without any helper/UAC dependency.
- **Promotion tier:** only after a coherent candidate exists, restore ordinary release/matrix workflows, package the matching Pandora artifact, and perform the physical HDD gate. Release or dev CI duration is never performance evidence.

## Required promotion tests

Before promotion, the runtime candidate must prove:

- normal GUI Start reaches a direct capability session with no elevation request;
- `--run-instance`, default/legacy and FullVerification remain stock SHA-1-only;
- same-size mutation, truncate/restore and delete/recreate fail closed;
- journal restamp/regression/discontinuity fail closed;
- corrupt/partial manifest fails closed;
- inability to open/query the volume or file USN fails closed without UAC;
- no helper path/hash/IPC input can make an otherwise ineligible launch eligible;
- schema/runtime update mismatch falls back rather than consuming uncertain state.

Only after those semantic gates pass should a release artifact be built for a physical unchanged seed -> reuse comparison. The already observed 103.990 s -> 34.805 s asset-phase result motivates the work but is evidence for PR #24's helper-backed implementation, not for this direct implementation and not for TTMM.
