# AppCDS preflight attribution probe

Agent 167 diagnostic branch. This probe is **diagnostic only**. It does not change AppCDS identity, classpath/module-path ordering, cache locking, state transitions, staging, promotion, archive validation, training, or Java launch semantics.

## Activation

The probe is disabled unless `BOOTOPTIM_APPCDS_PREFLIGHT_PROBE` names a fresh output file. When the variable is absent, the interposer calls the existing `build_launch_plan` implementation directly; no probe clocks or sidecar writes run on that path.

When enabled, the helper writes exactly one aggregate JSON object after the preflight decision has been computed. The output path is opened with `create_new`; an existing file is never overwritten. Probe write/open failures are ignored and cannot change `STOCK`, `TRAIN`, or `READY`.

Example PowerShell setup for one run:

```powershell
$env:BOOTOPTIM_APPCDS_PREFLIGHT_PROBE = "C:\\temp\\appcds-preflight-01.json"
$env:BOOTOPTIM_APPCDS_MODE = "plan"
```

Use a new sidecar path for every launch. Remove the probe variable after attribution.

## Schema

Schema: `bootoptim.appcds_preflight_probe.v1`.

The sidecar contains only:

- AppCDS mode (`plan` or `auto`) and helper decision (`STOCK`, `TRAIN`, `READY`, or `ERROR`);
- monotonic nanosecond durations from Rust `Instant`;
- top-level preflight buckets: cache-directory setup, launch-plan construction, one-shot lock attempt, plan publication, `classify_cache` when reached, and training-archive promotion when reached;
- launch-plan construction buckets: Java/release identity, classpath hashing, module-path hashing, top-level mod hashing, pack-input tree walk/hash, resource-pack/manifest digest work, helper+launcher component hashing, argv/eligibility work, and plan serialization;
- aggregate file counts and byte totals already present in the generated plan for classpath, module path, mods, pack inputs, helper/launcher components.

It contains no paths, filenames, arguments, URLs, account identifiers, tokens, archive hashes, plan hashes, config values, or per-file timing.

The helper records only a small fixed number of monotonic clock reads per launch. It does not clock individual files. The diagnostic sidecar write occurs after the decision and its `total_ns` snapshot; Pandora's outer PR #7 `appcds_preflight` span still includes helper startup/teardown and the small sidecar write, so compare the two rather than treating them as identical clocks.

## Exact path interpretation

`BOOTOPTIM_APPCDS_MODE=plan` executes:

1. construct the complete launch plan, including strong SHA-256 reads;
2. attempt the cache lock once, without retry or sleep;
3. atomically publish `launch-plan.json`, `launch-plan.sha256`, and `launch-plan.match` using the existing synchronized writes;
4. return `STOCK`.

`plan` returns **before** `classify_cache`. Therefore it does not hash `ready.jsa`, inspect/promote completed training output, or create a new training request.

`BOOTOPTIM_APPCDS_MODE=auto` performs the same complete plan construction/publication first. Only after exact plan stability and eligibility does it call `classify_cache`. A valid READY archive is checked by size and a full SHA-256 of `ready.jsa` on every such preflight. A completed training archive is also fully SHA-256 hashed during `promote_archive` before the atomic READY transition.

Training generation itself is not preflight work: `TRAIN` only prepares metadata and makes Pandora add `ArchiveClassesAtExit`; HotSpot writes the archive at JVM exit. That generation cost must be reported outside normal Start-to-Java. Java-to-menu is also a separate boundary.

## Why the 245.192 s observation is not yet an AppCDS cache result

A warm/clean Mojang asset result proves only that the asset-object verification state was warm. The AppCDS plan reads a different input set: Java/release, final classpath/module-path artifacts, top-level mod JARs, `config/`, `defaultconfigs/`, `kubejs/`, `scripts/`, `options.txt`, and the helper/launcher binaries. Those files can be cold on an HDD even when `assets/objects` is warm.

If the 245.192 s capture used `plan`, READY archive verification and promotion are excluded by control flow. The leading source hypothesis is therefore physical read/seek/antivirus cost while constructing the strong launch identity, plus the three synchronized plan-publication writes. If it used `auto`, `classify_cache` can additionally contain a full READY archive read, and a one-time post-training promotion can contain a full training archive read.

No saving follows from this source analysis alone.

## Physical decision protocol

Use the exact same packaged Pandora/interposer pair and the PR #7 root-valid launch probe. Keep AppCDS mode explicit and record it with the run. Use one fresh AppCDS sidecar per launch.

For a `plan` capture, require `schema=bootoptim.appcds_preflight_probe.v1`, `mode=plan`, `decision=STOCK`, no `classify_cache`/`promote_archive` bucket, and a valid PR #7 outer `appcds_preflight` interval. Reject the diagnostic if the sidecar is absent or reused.

The first acceptance/rejection metric is the fraction of helper `total_ns` and outer `appcds_preflight` attributable to `build_launch_plan`, and inside it the dominant category duration together with that category's aggregate file/byte count. A repeated pair of otherwise unchanged `plan` launches is especially useful: if the first is hundreds of seconds and the second collapses while inventory is identical, that supports storage/cache/AV effects rather than lock/state-machine delay. If `persist_plan` dominates, the three synchronized atomic publications are the next narrow target. If no bucket accounts for the outer span, helper process startup/teardown or uninstrumented state I/O is implicated by the outer-minus-inner gap.

For `auto`, evaluate separately. If `classify_cache` dominates READY launches, the exact physical candidate to investigate is archive-integrity validation; do not skip or weaken that hash without a new invalidation/TOCTOU proof. If `promote_archive` dominates, classify it as one-time post-training promotion cost, not recurring normal launch cost.

Do not combine preflight with training generation or Java-to-menu, and do not infer hardware-equivalent savings from hosted CI.
