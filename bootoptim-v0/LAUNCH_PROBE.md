# BootOptim Pandora launch probe v1

This diagnostic is an **opt-in measurement harness** for the Pandora launch path. It does not change AppCDS identity/state, MoreCulling handling, launch arguments, downloads, authentication, or game behavior.

## Activation and artifact

The probe is disabled unless `BOOTOPTIM_LAUNCH_PROBE` is present. Its value is the output JSONL filename. Use a fresh filename for every physical run and set the variable **before starting Pandora**.

```powershell
$probe = Join-Path $PWD 'bootoptim-launch-probe-laptop-01.jsonl'
Remove-Item $probe -ErrorAction SilentlyContinue
$env:BOOTOPTIM_LAUNCH_PROBE = $probe
.\BootOptimPandora-v0.exe
```

When disabled, the probe creates no diagnostic file and adds no subprocess. Probe activation no longer permanently caches an early absent environment variable: an absence check cannot create the split state in which only the later command-spawn writer is active.

After the single run:

```powershell
Remove-Item Env:BOOTOPTIM_LAUNCH_PROBE -ErrorAction SilentlyContinue
Get-FileHash $probe -Algorithm SHA256
Copy-Item $probe <offline-destination>
```

The JSONL never stores the output filename, full instance path, Java path, account UUID/name, access token, command line, JVM arguments, URLs, or downloaded filenames.

## Mandatory root contract

A successful trace that reaches direct Java process creation is valid only when all of the following are true:

1. The **first non-empty JSONL record** is `phase="launcher_pre_java", event="begin"`.
2. That root begin occurs exactly once and precedes every launch child event.
3. Every observed span has one begin before one end; timestamps never decrease in file order.
4. `java_spawn.begin` and `java_spawn.end` both occur exactly once inside the open root.
5. `launcher_pre_java.end` occurs exactly once, after `java_spawn.end`.
6. `java_to_menu` follows as `event="unobserved", observed=false, duration_ns=null`.

A trace missing either root boundary, containing a child outside the root, or containing a span end without its begin is **invalid and must not be used as a measurement**.

The root starts at the concrete launch request. GUI Start uses `StartInstance`; the name-based launch path is also armed before backend launch and adopts the concrete launch modal when it becomes available. `send` and `send_with_serial` are both covered.

The command-spawn layer is deliberately unable to create a late-only valid trace. Before it emits `appcds_preflight` or `java_spawn`, it requires the existing JSONL to begin with the root record above. If no root exists, the late writer emits no measured Java-spawn trace.

## Observable phases

Each line has `schema="bootoptim.launch_probe.v1"` and a platform monotonic `mono_ns`. Windows uses `QueryPerformanceCounter`; Linux/macOS use `CLOCK_MONOTONIC`.

The useful phase vocabulary is:

1. `launcher_pre_java.begin` — mandatory root at launch request.
2. `launch_request` — launch handoff instant.
3. `instance_config` — instance configuration retrieval/reload.
4. `account_selection` — account selection/login acquisition.
5. `prelaunch` — existing Pandora prelaunch work.
6. `version_loader_resolution` — loader/version resolution. The pinned parent progress contract closes it at Vanilla `2/7`, Fabric `5/10`, Forge/NeoForge `8/13`. Forge/NeoForge `1/13` may emit `network_request source="loader_sha1"`.
7. `java_runtime`, `assets_verify_download`, `libraries_classpath_inputs` — observable top-level concurrent preparation branches. A configured Java can legitimately leave `java_runtime` unobserved.
8. `network_download` / `network_request` — fixed-source observations only; no URLs or payloads are stored.
9. `classpath_resolution`, `native_extraction`, `wrapper_arguments` — **not timed**. They are always explicit `unobserved` markers because Pandora exposes no trustworthy independent boundaries for them.
10. `appcds_preflight` — optional helper span when the existing preflight actually runs.
11. `java_spawn` — direct Java OS process-creation begin/end. It requires executable basename `java`, `java.exe`, or `javaw.exe` and Pandora's `LaunchWrapper` marker.
12. `launcher_pre_java.end` — mandatory successful root end after Java creation.
13. `java_to_menu` — explicit `unobserved`; this harness does not parse game output to manufacture menu-ready timing.

If `prelaunch`, `version_loader_resolution`, `assets_verify_download`, `libraries_classpath_inputs`, or `java_runtime` never appear at all before command construction, the probe records that absence as `unobserved` before Java spawn. It never invents a start/end duration. If a real begin was observed but its matching real end is missing, the trace remains contract-invalid rather than being repaired with a synthetic end.

## Network boundary

Explicit Pandora downloader transitions emit fixed `source` values: `assets`, `libraries`, `java_runtime`, or `tracked_other`. Account login and the pinned loader SHA-1 route can emit fixed `network_request` observations. The probe does not globally hook `reqwest`.

## CI and packaged-binary gate

CI tests the full trace contract on Linux and Windows, direct-Java filtering, disabled-environment behavior, existing deterministic/preflight regressions, and the patched Pandora release build. After the release build, CI inspects the exact `pandora_launcher.exe` that is copied into the downloadable artifact and refuses publication unless that binary contains the launch-probe schema plus the `launcher_pre_java` and `java_spawn` markers. This prevents a green library-only test from being mistaken for a packaged launcher containing the diagnostic route.

## One-run laptop protocol

1. Download only the Windows artifact produced by the final green PR #7 workflow. Extract `BootOptimPandora-v0.exe`, `bootoptim-launch-interposer.exe`, and `SHA256SUMS.txt` together.
2. Verify both executable SHA-256 values against `SHA256SUMS.txt` **before starting Pandora**. Do not reuse hashes from an older PR #7 artifact.
3. Open a fresh PowerShell in that extracted directory. Remove the intended JSONL path if it exists, set `BOOTOPTIM_LAUNCH_PROBE`, then start `BootOptimPandora-v0.exe` from that same PowerShell. Do not set/change the variable after Pandora is already running.
4. Keep the intended AppCDS/helper settings unchanged for the measurement. Press **Start exactly once** on the intended instance; do not use an external Java wrapper.
5. After successful Java creation and normal completion of the launch modal, close the processes normally, clear the environment variable, and hash/copy the JSONL offline.
6. Before analysing durations, validate the structural sequence below. Reject the capture immediately if it does not match.

Required success skeleton (other observed child/network records may appear only between the root boundaries):

```text
launcher_pre_java.begin
launch_request.instant
instance_config.begin
... instance_config.end ...
... prelaunch begin/end OR prelaunch.unobserved ...
... version_loader_resolution begin/end OR unobserved ...
... assets_verify_download begin/end OR unobserved ...
... libraries_classpath_inputs begin/end OR unobserved ...
classpath_resolution.unobserved
native_extraction.unobserved
wrapper_arguments.unobserved
[appcds_preflight.begin
 appcds_preflight.end]
java_spawn.begin
java_spawn.end
launcher_pre_java.end
java_to_menu.unobserved
```

Use `launcher_pre_java.end.mono_ns - launcher_pre_java.begin.mono_ns` only after this contract passes. Do not sum overlapping child spans. This document makes no performance claim.
