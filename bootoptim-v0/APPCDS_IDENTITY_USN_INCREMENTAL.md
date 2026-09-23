# AppCDS incremental identity via direct NTFS/USN

Status: **candidate, default-off**. This document describes Agent 203's implementation on
\`agent/integration-current@7a821e8d5f89c71677d149b3140e21f4ed171ec4\`.
It does not claim a startup-time or TTMM improvement.

## Boundary

The canonical AppCDS launch plan remains the semantic authority. Every preflight still
rebuilds the live classpath, module-path, top-level mod set and pack-input tree, including
their existing ordering/canonicalization rules. Only the SHA-256 result of an individual
raw regular file can be reused.

Reuse is authorized only when all of the following are true for the exact role + encoded
path record:

- the candidate is explicitly enabled with \`BOOTOPTIM_APPCDS_IDENTITY_CACHE=1\`;
- the caller is Pandora's normal GUI launch authority; command/CLI/legacy defaults remain
  unknown and therefore stock;
- the file is opened by a read-only protected handle with no write/delete sharing;
- it is a regular NTFS file and not a reparse point;
- volume GUID and volume serial match;
- the USN journal ID is unchanged, current journal bounds still cover the stored snapshot,
  and \`next_usn\` has not regressed;
- FileId and file-USN match the stored record;
- final identity is re-read from the same protected handle and is unchanged.

No size, mtime, ChangeTime, TTL or watcher event is an acceptance predicate.

The incremental cache is deliberately limited to classpath, module-path, mod and raw
pack-input files. The Java executable, helper/launcher components, selected config files
with semantic canonicalization, Java \`release\`, resource-pack selection and READY archive
verification remain on their stock hashing paths. This keeps accessory/cross-volume
components from becoming a prerequisite for reuse of the large byte-sensitive identity
inputs and does not change the bytes produced by \`launch-plan.json\`.

## Shared direct capability

\`bootoptim-v0/ntfs_usn_direct.rs\` is the single policy-free implementation of the direct
Windows primitive extracted from merged PR #45. Assets and AppCDS both consume it. It
contains no hash policy, asset/AppCDS manifest policy, privilege escalation, helper
process, service or \`ReadDirectoryChangesW\` authority.

The primitive owns:

- protected file handles and final same-handle identity checks;
- direct NTFS volume opening;
- \`FSCTL_QUERY_USN_JOURNAL\`;
- \`FSCTL_READ_FILE_USN_DATA\`;
- volume GUID/serial and FileId extraction;
- atomic replace used by callers after their own policy/locking decisions.

The existing restricted-token probe remains a release gate and demonstrates that the
direct journal/FileId/file-USN operations do not require a UAC broker.

## Manifest and locking

The AppCDS manifest is \`.bootoptim/appcds/identity-manifest-v1.txt\` (or the cache
directory supplied by a later namespace caller). It is versioned and complete: a session
publishes it atomically only if every cache-eligible raw input in that rebuilt plan has
usable final evidence. Unknown, corrupt, partial or oversized manifests are ignored and
the relevant inputs are strongly hashed.

When reuse is requested, the existing AppCDS \`cache.lock\` is acquired before identity
work and held through manifest publication and the existing AppCDS state decision. Lock
contention performs a stock strong plan rebuild and returns stock activation; it never
falls through to metadata-only reuse.

\`BOOTOPTIM_APPCDS_IDENTITY_FORCE_STOCK=1\` is a one-way invalidation input for an
update/repair/download orchestration layer. It cannot enable reuse. The current normal
launch also remains safe without that hint because fresh membership is rebuilt and any
changed file must fail FileId/file-USN evidence, but future repair flows should set the
hint for an intentionally conservative full-stock campaign.

## Composition with open AppCDS work

This change does not copy or modify PR #47, #48 or #50.

- #47 can change FIRST/MISMATCH -> TRAIN behavior independently; identity digest
  selection occurs before the existing state machine.
- #48/#50 can provide a profile-specific cache directory. The identity cache accepts the
  caller's AppCDS cache directory and imposes no profile/UUID naming policy.
- State files, archive ownership, clone/toggle semantics and profile namespaces remain
  those PRs' responsibility.

## Test contract

The candidate gate covers exact stock/seed/reuse plan bytes, same-size mutation with
restored mtime, delete/recreate, rename/path and role keys, journal reset/regression/
discontinuity, FileId/file-USN changes, final same-handle failure, reparse/capability
failure, corrupt/partial manifests, lock contention, CLI-default authority,
force-stock/update signal, existing asset USN tests and the restricted-token direct probe.

CI is correctness/packaging evidence only and is not a performance measurement.

## Laptop diagnosis: long USN filename (2026-09-23)

The error-diagnostics candidate reported `eligible=1163 reused=0 strong=1163`,
with `pre-evidence` and `post-evidence` both failing at `file-query`, raw OS
error 122, and the same stable input token. The token mapped locally through the
candidate launch plan to one long mod JAR filename (124 UTF-16 code units).
Windows error 122 is `ERROR_INSUFFICIENT_BUFFER`; the shared direct USN helper
was asking `FSCTL_READ_FILE_USN_DATA` to return a `USN_RECORD_V2` including its
inline filename into a fixed 256-byte buffer. This explains why the same mod
could not produce evidence before or after hashing, blocking complete-manifest
publication. The candidate increases that fixed buffer to 1024 bytes and adds
a Windows regression test with a filename longer than the old capacity.

This was a diagnostic run, not an AppCDS performance result: the candidate
launcher/helper identity differed from the build that created the existing
READY archive, so the interposer marked it `STALE / identity-mismatch` and
`FIRST_OR_MISMATCH`. BootOptim logged `main_menu_reached` at 405829 ms. The run
does not establish that the archive was consumed or that AppCDS improved startup.

## Physical protocol after a green artifact

Use the exact checksummed Windows artifact and keep Java->menu separate from launcher
pre-Java timing.

1. **Seed / first identity:** enable the candidate on normal GUI Start with no previous
   identity manifest. Expect strong hashes and atomic manifest publication.
2. **Confirmation run:** start again without changing the pack. Rebuild the same plan
   membership/order; matching records may reuse per-file digests. Confirm the resulting
   \`launch-plan.json\` bytes are identical to the stock/seed identity.
3. **Consumption run:** only after AppCDS' existing independent identity/state rules say
   the archive is consumable, measure launcher->Java from the same marker used by prior
   probes. Record Java->menu separately.
4. Repeat negative mutations (same-size/restored-mtime and delete/recreate) before any
   product-default decision. A miss must strongly hash the changed file and preserve the
   canonical plan result.

Do not extrapolate PR #45 asset timings to AppCDS and do not report these runs as TTMM.
