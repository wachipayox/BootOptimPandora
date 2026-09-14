# BootOptim launch verification I/O coordinator candidate

This candidate is deliberately narrower than an integrity cache. It changes only the scheduling of local SHA-1 file reads in Pandora's launch pipeline; it does not skip, memoize, weaken or replace any stock integrity check.

## Base and scope

The implementation is based exactly on `agent155/launch-prejava-probe-20260914` commit `48b12855f054302d53a305f1eab9e768508fb3e2`. `AGENTS.md` is absent at that revision.

Pandora's outer launch joins managed Java-runtime preparation, asset verification/download, library verification/download and log-configuration loading concurrently. The three high-volume integrity paths each already have an independent local `disk_semaphore` with 32 permits. Therefore, before Tokio's blocking-pool scheduling is considered, up to 96 asset/library/runtime tasks can independently reach local SHA-1 verification. The physical PR #7 HDD trace shows why that shape matters: `assets_verify_download` was 649.015 s and `libraries_classpath_inputs` was 614.602 s, with the spans almost completely overlapping. They must not be added as sequential cost; the slowest unfinished sibling is on the concurrent group's critical path.

The shared budget covers calls to `crate::fs::check_sha1_hash` whose compile-time caller is `crates/backend/src/launch/mod.rs` (or the crate-relative `src/launch/mod.rs`). That includes the high-volume on-disk SHA-1 reads from:

- `do_asset_objects_load`;
- `do_libraries_load` for artifacts that have a stock SHA-1;
- `do_java_runtime_load`.

It also covers the small number of other launch-module calls through the same common helper, such as log-configuration / Forge launch checks. Calls from content installation, updater code or another source file do not match the callsite gate and retain stock scheduling. If a future source layout no longer matches the pinned launch path, the gate is not entered and scheduling is stock (fail-open).

The following are intentionally outside the budget:

- network requests and response-body reads;
- the existing per-domain download semaphores;
- SHA-1 computation over already-downloaded in-memory response bytes;
- decompression and writes/repair after a failed local verification;
- metadata fetches, AppCDS, MoreCulling identity, classpath/module-path construction, native extraction, UI, account handling, updater logic and the PR #7 probe.

## Activation

The coordinator is **off by default**. It is enabled only when the launcher process starts with:

```text
BOOTOPTIM_VERIFY_IO_LIMIT=<N>
```

where `N` is an exact decimal integer from `1` through `32`. Missing, non-Unicode, malformed, zero or out-of-range values mean **stock scheduling**. The value is read once per launcher process; restart the launcher between A/B conditions.

There is deliberately no automatic HDD/SSD detection in this candidate. Storage capability detection has enough ambiguity (tiered storage, USB/SATA bridges, network-backed paths, virtual disks, per-instance paths) that guessing could regress a normal machine. The physical gate can instead compare explicit limits such as `1`, `2`, `4` and stock. With the variable absent, there is no serialization and the existing 32-per-domain scheduling remains intact, so SSD/normal-CPU users are not opted into an HDD hypothesis.

## Fairness and deadlock argument

The coordinator uses a process-local FIFO token queue plus a fixed permit count. A waiter may enter only when it is at the queue head and a permit is free. After admission it is removed from the queue, and release wakes all waiters so the next FIFO token can proceed. This provides no-starvation ordering among calls that have reached the common helper.

The existing asset/library/runtime semaphores remain unchanged and still bound each domain to 32 admitted disk tasks. A task acquires the shared budget only inside the synchronous SHA-1 helper running under the existing `spawn_blocking`; it holds no coordinator permit while awaiting network I/O. There is no reverse acquisition path back into a per-domain disk semaphore, so the new wait has no lock cycle. Permit destruction occurs on every normal return and Rust unwind path.

Waiting happens on Tokio blocking workers rather than the async reactor. That is intentional for this minimal candidate: it avoids blocking the async executor while preserving all existing `spawn_blocking` callsites. The physical A/B gate must still reject the candidate if the extra blocking-worker queue itself proves harmful.

## Integrity and repair semantics

`check_sha1_hash` still opens the same path, streams the full file into the same SHA-1 implementation and compares the same 20-byte expected digest. The coordinator wraps that operation; it does not inspect mtime, size or existence and does not cache a result.

The launch callsites are unchanged. A valid file still returns `true`. A mismatch still returns `false`. A missing/unreadable file still returns the same I/O error; the existing launch callsites use `unwrap_or(false)` and therefore enter their unchanged download/repair branches. Downloaded content still undergoes the existing size/hash validation before write. Errors from networking, wrong response size, wrong downloaded hash, decompression and writes are unchanged.

Unit tests pin valid/mutated/missing-file SHA-1 behavior, fail-open property parsing, launch-vs-updater callsite gating, and concurrent completion under a limit without exceeding that limit. CI runs these tests before the normal cross-platform build.

## Physical A/B gate

Use the PR #7 launch probe unchanged. Do not use this candidate to claim Java-to-menu or TTMM improvement; this experiment ends at `java_spawn`.

1. Build the candidate from this branch and keep the same instance, Java, account state and launcher/probe configuration.
2. Reach a stable repaired state before timing. Because the PR #7 capture observed `network_download source=assets` but does not record its bytes or duration, do not compare a repair/download run against a no-download run as a scheduling result.
3. Restart Pandora for each condition. For stock, remove `BOOTOPTIM_VERIFY_IO_LIMIT`. For candidates, set one explicit value (recommended first sweep: `1`, `2`, `4`).
4. Alternate conditions rather than doing all stock then all candidate runs. Keep cold/warm OS-cache state comparable and record it explicitly.
5. Require the same successful pre-Java integrity outcome and Java creation. Any missing repair, changed failure behavior, hang/deadlock or integrity discrepancy is an immediate NO-GO.
6. Compute click-to-Java from the probe's `launcher_pre_java` boundary. Inspect `assets_verify_download`, `libraries_classpath_inputs` and `java_runtime` as overlapping siblings; never add their durations. Compare the critical-path envelope and total click-to-Java wall time.
7. Reject limits that regress a normal SSD/fast-PC control. The feature remains default-off until a storage-selection policy can be justified from physical evidence.

No physical performance saving is claimed by this branch.