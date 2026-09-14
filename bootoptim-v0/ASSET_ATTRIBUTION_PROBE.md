# Agent 162 — assets attribution probe

This diagnostic is **opt-in** and exists only to attribute physical work inside PR #7's `assets_verify_download` span. It does not optimize, skip, cache, reorder, serialize or otherwise weaken asset verification, and it does not change AppCDS, the PR #8 I/O scheduler, the PR #9/160 USN cache, or PR #10 verification-intent semantics.

## Activation

Use a fresh sidecar path and the PR #7 root probe in the same launcher process:

```powershell
$rootProbe = Join-Path $PWD 'bootoptim-launch-probe-agent162.jsonl'
$assetProbe = Join-Path $PWD 'bootoptim-assets-agent162.json'
Remove-Item $rootProbe,$assetProbe -ErrorAction SilentlyContinue
$env:BOOTOPTIM_LAUNCH_PROBE = $rootProbe
$env:BOOTOPTIM_ASSET_ATTRIBUTION = $assetProbe
.\BootOptimPandora-v0.exe
```

With `BOOTOPTIM_ASSET_ATTRIBUTION` absent or empty, no asset sidecar is created and `do_asset_objects_load` calls Pandora's existing `crate::fs::check_sha1_hash` branch exactly as before.

The sidecar is one final JSON object per configured output path. It contains no asset paths, hashes, filenames, URLs, account identifiers, command lines or tokens. Telemetry serialization, directory creation and write failures are ignored and cannot fail the launch.

## What is attributed

The final snapshot uses schema `bootoptim.asset_attribution.v1` and reports:

- planned object count and planned bytes from the selected asset index;
- local SHA-1 attempts, hits, misses, I/O errors, and bytes actually accepted by the SHA-1 writer;
- successful HTTP response bodies observed as downloaded objects/bytes, network errors, response-size failures and downloaded-body SHA-1 failures;
- whether any asset network request was attempted;
- maximum simultaneous local hash operations and maximum simultaneous asset downloads;
- `warm_ab_contaminated`, which is true after any hash miss, hash I/O error, asset network activity or downloaded object and therefore excludes that capture from a clean warm/no-repair A/B comparison.

The enabled hash path preserves the stock operation shape: `File::open`, `io::copy` into SHA-1, then digest comparison. A small writer wrapper counts bytes accepted by SHA-1 but performs no clock reads. The existing future-per-object shape, disk semaphore size 32 and download semaphore size 8 are unchanged. Download concurrency is observed only after the existing download permit has been acquired, and the diagnostic guard is released before the stock permit is released.

`hash_wall_ns`, `hash_cpu_ns` and `hash_queue_wait_ns` are deliberately `null`. Reliable aggregate values would require per-object timing, blocking-pool instrumentation or semaphore-wait timing, all of which would add the instrumentation perturbation this probe is intended to avoid. `timing_observation` records that limitation explicitly.

The probe performs only two monotonic clock reads per launch: one when the asset probe is created and one at the final snapshot. On Windows it uses the same QPC-derived nanosecond clock family as PR #7, so the sidecar interval can be placed inside the matching PR #7 root/asset span without a clock read per object.

On an error snapshot, `error_snapshot_complete=false`. Stock `try_join_all` may cancel sibling async futures after the first error while already-spawned blocking work can finish independently. The probe does not wait for those siblings because doing so would change launch behavior. Successful `outcome=ok` captures are the attribution target.

## One-run laptop protocol

Use exactly one launch, not an A/B pair:

1. Use only the final green Windows artifact from this PR and verify `SHA256SUMS.txt` before launch.
2. Start from the known repaired/warm asset state intended for attribution; do not deliberately delete or mutate assets.
3. In a fresh PowerShell, delete both intended sidecars, set both variables above, launch Pandora, and press Start exactly once.
4. Allow normal Java creation. Do not start another instance in the same Pandora process.
5. Clear both environment variables after the run and preserve both sidecars together.
6. Validate PR #7 first: require a valid `launcher_pre_java.begin` → `java_spawn` → `launcher_pre_java.end` root and a valid `assets_verify_download` span. Reject the capture if the PR #7 structural contract fails.
7. Require the asset sidecar to have `schema=bootoptim.asset_attribution.v1`, `outcome=ok`, `launch_probe_active=true`, and its monotonic interval to lie within the same PR #7 launch root. Compare the planned object/byte totals with the selected asset index as a consistency check.
8. For a clean warm attribution capture require `warm_ab_contaminated=false`, `any_network=false`, zero hash misses, zero hash I/O errors, and zero downloaded objects/bytes. Otherwise classify the run as repair/download-contaminated and do not use it as a warm scheduling/cache A/B observation.
9. Report these values as attribution only. Do not add overlapping PR #7 sibling spans and do not infer a performance saving from this single diagnostic run.

No optimization, Start-to-Java saving or TTMM saving is claimed by this probe.
