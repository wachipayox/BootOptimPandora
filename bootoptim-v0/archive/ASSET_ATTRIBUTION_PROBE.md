# BootOptim asset attribution probe v1

`bootoptim.asset_attribution.v1` is an opt-in diagnostic sidecar for the launcher phase `assets_verify_download`.
It is not an optimization and it does not authorize reuse, repair, download, scheduling, AppCDS, or identity decisions.

## Opt-in

Set `BOOTOPTIM_ASSET_ATTRIBUTION` to a **new output path for each launch**. If the variable is absent or empty, no sidecar is created and asset verification follows the existing path unchanged.

The sidecar is written with `create_new`; an existing path is never overwritten. Failure to create or write diagnostic output does not alter launch behavior.

For correlation with the launch root, also set `BOOTOPTIM_LAUNCH_PROBE` to a fresh JSONL path. `launch_probe_active=true` means that launch-root instrumentation was armed in the same launcher process.

## Schema

The JSON object has `schema="bootoptim.asset_attribution.v1"`, `phase="assets_verify_download"`, an `outcome`, monotonic begin/snapshot timestamps, `launch_probe_active`, planned object/byte totals, aggregate SHA-1 counters, aggregate download/network/error counters and concurrency maxima. It intentionally does not persist asset paths, hashes, filenames, URLs, account data or tokens.

`hash_wall_ns`, `hash_cpu_ns`, and `hash_queue_wait_ns` remain null in v1. No timing is inferred from aggregate counters.

## Composition with the USN cache

In the agent170 composition, the sidecar observes the SHA-1 work that actually executes after the PR #11 decision. It never participates in that decision:

- If USN reuse is disabled, unavailable, ambiguous, FullVerification, CLI/legacy-authority, non-NTFS, helper/IPC-invalid, journal-invalid, FileId/USN-mismatched, or otherwise not a verified hit, Pandora executes its full SHA-1 fallback and the sidecar counts that SHA-1.
- On Windows, a USN miss continues to hash the same protected file handle. Attribution observes that same-handle read rather than reopening the pathname.
- Only a PR #11 `VerifiedReuse` can return before SHA-1. Such reuse therefore appears as no SHA-1 attempt/bytes for that asset. The sidecar itself cannot create this outcome.
- Network/download observations remain aggregate diagnostics. A run with any network, hash miss, hash I/O error, downloaded object, size failure, download-hash failure, or network error is not a clean reuse comparison.

The USN feature remains separately default-off under `BOOTOPTIM_ASSET_USN_CACHE=1`; attribution does not set or imply that variable.

## Valid comparison evidence

Every compared run must provide both a fresh launch-root JSONL and a fresh asset sidecar. The launch root must satisfy `LAUNCH_PROBE.md`: one `launcher_pre_java.begin`, one matching end, and the real Java spawn inside that root, with no `inclusive_*` fields. The sidecar must have the expected schema, `outcome=ok`, `launch_probe_active=true`, and zero network/miss/error/download contamination for a clean warm comparison.

Do not translate these diagnostics into saved time or TTMM without separate physical before/after evidence under the project measurement contract.
