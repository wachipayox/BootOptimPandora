# Agent 180 — real prelaunch attribution sidecar

This branch adds **telemetry only** on top of PR #26 head `agent175/prelaunch-mods-attribution-20260915@a0e9c9f3d474d6e820d7d59196242a6cb9610b03`. It does not add cache/reuse, hard links, snapshotting, a new rename, background I/O, or any change to the `mods` / `original_mods` ownership contract.

## Activation

The runtime probe is disabled unless `BOOTOPTIM_PRELAUNCH_ATTRIBUTION` is a non-empty output path. Its value is the path of one independent JSON sidecar. Use a fresh path for every launch.

To correlate the sidecar with the existing PR #7 Start→Java trace, enable both variables **before Pandora starts**:

```powershell
$launch = Join-Path $PWD 'bootoptim-launch-run01.jsonl'
$prelaunch = Join-Path $PWD 'bootoptim-prelaunch-run01.json'
Remove-Item $launch,$prelaunch -ErrorAction SilentlyContinue
$env:BOOTOPTIM_LAUNCH_PROBE = $launch
$env:BOOTOPTIM_PRELAUNCH_ATTRIBUTION = $prelaunch
.\BootOptimPandora-v0.exe
```

When `BOOTOPTIM_PRELAUNCH_ATTRIBUTION` is absent or empty, `BackendState::prelaunch` performs one environment lookup and immediately takes the stock prelaunch path. It does not allocate a report, call a monotonic/CPU clock, add per-file/per-entry telemetry work, create a sidecar, or write a sidecar.

The report is accumulated in memory only when enabled. On the normal launch path it is written only after `Launcher::launch` returns; a successful launch therefore persists it after direct Java process creation, outside the measured `BackendState::prelaunch` span. No background task/thread is created.

## Sidecar schema

Top-level `schema` is `bootoptim.prelaunch_attribution.v1`.

The report contains:

- `run_id`: process id + monotonic start + process-local sequence, identifying this attribution execution without instance/account/path identity.
- `origin="BackendState::prelaunch.enter"` and `endpoint="BackendState::prelaunch.return"`.
- `correlation.launch_probe_schema="bootoptim.launch_probe.v1"`.
- `correlation.launcher_pre_java_begin_mono_ns`: the exact PR #7 root-begin monotonic timestamp when the launch probe is active for the same modal launch; otherwise `null` with `status="unobserved"`.
- `spans[]`: `phase`, `parent`, `inclusive`, observed/unobserved status, monotonic start/end, wall nanoseconds, process-CPU nanoseconds when the platform API succeeds, counters/known bytes, and explicit reasons for unobserved data.
- `endpoint_mono_ns`: captured after the coarse `prelaunch` span closes.
- `flush_context`: where the already-collected report was persisted.

Observed phases cover:

- `prelaunch` (coarse marker retained for correlation only);
- `sync`;
- `load_content`;
- `mods_scan`;
- `apply_modpack_and_collect_mods` as one inclusive resolution call;
- `rotate_mods_to_original_mods`;
- `create_mods_dir`;
- `apply_copies_to_mods_dir`;
- `connector_cache_copy` for a top-level `.connector` extra;
- `extras_copy` for other top-level extras.

`modpack_config_yosbr` and `modpack_extra_file` are deliberately `unobserved`. Their work happens inside the shared `apply_modpack_and_collect_mods` helper, which also has non-prelaunch callers. This diagnostic leaves that helper and its API byte-for-byte stock rather than adding instrumentation plumbing or variable disabled-path overhead to unrelated operations. Their cost remains included in the observed parent `apply_modpack_and_collect_mods` wall/CPU span, but is not independently attributed or inferred.

`restore_prior` is deliberately emitted as `unobserved`: Pandora does not call `restore_mods_folder_if_stopped` inside `BackendState::prelaunch`. Timing a synthetic restore there would change semantics. The Connector restore/merge performed after a game stops is likewise outside the Start→Java/prelaunch endpoint and is not relabeled as prelaunch time.

Bytes are reported only when the production operation already exposes them, such as inline mod bytes already present in the resolved copy plan or `copy_content_recursive` progress. The telemetry does not stat every source or perform a second tree walk merely to manufacture byte totals. Such values remain `unobserved`.

## CPU / clock semantics

Wall timestamps use the same platform monotonic family as PR #7: QueryPerformanceCounter on Windows, CLOCK_MONOTONIC on Linux/macOS, with an Instant fallback only on other targets. Process CPU uses `GetProcessTimes` on Windows and `CLOCK_PROCESS_CPUTIME_ID` on Linux/macOS. If the platform call fails or is unavailable, CPU is `unobserved`, never zero-filled.

## One physical HDD run

1. Use one Windows artifact built from this draft branch. Keep the exact instance, pack, configs, sync targets, disabled children, sandbox setting, account, and Java selection intended for the measurement.
2. In a fresh PowerShell, remove both intended outputs and set both environment variables above **before** launching Pandora.
3. Press Start exactly once. Do not run repair/update, launch a second instance, change sync/modpack/disabled-child/sandbox state, or read/parse Minecraft logs during the run.
4. Accept the launch JSONL only if it satisfies PR #7's root contract: exactly one `launcher_pre_java.begin`, one `java_spawn.begin/end` inside it, and one `launcher_pre_java.end` after spawn.
5. Verify that `correlation.launcher_pre_java_begin_mono_ns` in the sidecar equals the first launch-JSONL record's `mono_ns`. Reject the capture if missing or different.
6. Attribute the coarse `prelaunch` span using the sidecar's non-inclusive observed child spans. Do **not** sum `prelaunch` with its children. Keep the explicitly unobserved `restore_prior`, `modpack_config_yosbr`, and `modpack_extra_file` as unknown rather than assigning them zero.
7. Record the existing PR #7 Start→Java duration separately. This sidecar does not observe Java→menu and makes no TTMM claim.
8. Clear both environment variables and archive/hash both outputs together.

The diagnostic decision is which observed prelaunch phase dominates wall time and, where available, whether that wall time is accompanied by process CPU or exposed copy bytes. Any `unobserved` field remains unobserved rather than being inferred as zero.
