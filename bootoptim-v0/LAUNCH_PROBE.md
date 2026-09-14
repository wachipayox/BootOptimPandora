# BootOptim Pandora launch probe v1

This diagnostic is an **opt-in measurement harness** for the Pandora launch path. It does not change AppCDS identity/state, MoreCulling handling, launch arguments, downloads, authentication, or game behavior.

## Activation and artifact

The probe is disabled unless `BOOTOPTIM_LAUNCH_PROBE` is present **before Pandora starts**. Its value is the output JSONL filename. Use a fresh filename for each physical run.

When disabled, the probe does not open, scan, hash, or create any diagnostic file and does not add a subprocess. The normal Pandora/AppCDS launch path is unchanged.

Example PowerShell activation from a temporary test directory:

```powershell
$probe = Join-Path $PWD 'bootoptim-launch-probe-laptop-01.jsonl'
Remove-Item $probe -ErrorAction SilentlyContinue
$env:BOOTOPTIM_LAUNCH_PROBE = $probe
.\BootOptimPandora-v0.exe
```

The GUI **Start** action truncates/starts the selected JSONL file for that launch. Do not launch two instances into the same probe filename concurrently.

After the one physical run, close Pandora and clear the opt-in variable:

```powershell
Remove-Item Env:BOOTOPTIM_LAUNCH_PROBE -ErrorAction SilentlyContinue
Get-FileHash $probe -Algorithm SHA256
Copy-Item $probe <offline-destination>
```

The main agent can then consume the copied JSONL offline together with its SHA-256. The artifact itself never contains the output filename, full instance path, Java path, account UUID/name, access token, command line, JVM arguments, URLs, or file names.

## JSONL schema

Every line is one JSON object with `schema="bootoptim.launch_probe.v1"` and a platform monotonic `mono_ns` timestamp. The Windows implementation uses `QueryPerformanceCounter`; Linux/macOS use `CLOCK_MONOTONIC`. Timestamps are comparable only within the same boot/run and are intentionally not wall-clock timestamps.

Common fields:

- `phase`: fixed phase identifier.
- `event`: `instant`, `begin`, `end`, `ready`, `inclusive_begin`, `inclusive_end`, `network_download_begin`, `unobserved`, or `summary` where applicable.
- `network`: `true` only when Pandora has switched an existing Java-runtime/assets/libraries tracker from verification to its explicit download path.
- `outcome`: fixed `ok`, `error`, or `cancelled` where applicable.

The expected phase vocabulary is:

1. `launch_request` — GUI Start message handed to the backend.
2. `instance_config` — retrieval/reload of the selected `InstanceConfiguration`.
3. `account_selection` — account selection/login acquisition required for launch. No identity is recorded.
4. `prelaunch` — Pandora's existing prelaunch work before the launcher pipeline.
5. `version_loader_resolution` — Minecraft/loader launch-version resolution. This may itself use cached or remote metadata; the probe deliberately does not intercept HTTP or log URLs/tokens, so this span is not labelled as a confirmed download unless an existing explicit download tracker says so.
6. `java_runtime`, `assets_verify_download`, `libraries_classpath_inputs` — concurrent branches of Pandora's existing `try_join4` launch preparation. An explicit `network_download_begin` event means that branch left local verification and entered its existing download path.
7. `classpath_resolution` and `native_extraction` — deliberately **inclusive, overlapping envelopes** beginning only after both assets and libraries have completed and ending when the final Minecraft command is dispatched to the command spawner. Pandora builds the classpath and extracts native JAR entries in the same post-join loop; the low-intrusion probe does not split that loop or claim exclusive CPU time.
8. `wrapper_arguments` — final structured Pandora command/JVM arguments are ready for the spawner. No argument values are persisted.
9. `java_spawn` — OS process-creation call begin/end. A successful end is also the `launcher_pre_java` end boundary.
10. `launcher_pre_java` — end event for the Start→successful Java process-creation interval. Compute the duration as `launcher_pre_java.end.mono_ns - launch_request.mono_ns`; do not add child spans because several overlap.
11. `java_to_menu` — always emitted as `unobserved`, `observed=false`, `duration_ns=null` by this harness. Pandora has no menu-ready signal that can be used without reading/interpreting game output, and this PR intentionally does not read game logs to manufacture a TTMM value.
12. `launch` — fixed terminal error/cancellation events when Pandora's existing modal action reports them.

## One-run laptop protocol

1. Use the Windows artifact from this PR after its Windows Actions build/test job is green and verify `SHA256SUMS.txt` as in the existing v0 protocol.
2. Keep the AppCDS mode and instance state exactly as intended for the run; this probe does not reinterpret an AppCDS training run as a performance result.
3. Set `BOOTOPTIM_LAUNCH_PROBE` to a new local JSONL filename **before starting Pandora**.
4. Start Pandora, select the intended instance/account, press **Start exactly once**, and allow Pandora to reach successful Java creation. Do not modify the instance during the run.
5. Once the Minecraft window exists, the required pre-Java measurement is already complete. Reaching the main menu can be noted separately by the physical operator, but this JSONL deliberately does not claim or infer Java→menu time.
6. Exit normally, remove the environment variable, hash the JSONL, and copy the JSONL plus its hash to the offline handoff location.
7. Inspect spans by monotonic start/end. Treat `java_runtime`, `assets_verify_download`, `libraries_classpath_inputs` as parallel siblings. Treat `classpath_resolution` and `native_extraction` as inclusive overlapping envelopes. Never sum those scopes to derive Start→Java.

## Critical path and current diagnostic hypothesis

Pandora starts Java-runtime preparation, asset verification/download, library verification/download, and log-configuration loading concurrently and waits for all of them before native extraction/classpath assembly. Therefore the longest unfinished sibling is on the critical path; the sibling durations are not additive.

The current code checks the SHA-1 of every asset object in the asset index before reusing it. On an HDD, that full verification fan-out is a plausible explanation for the physically observed long `Verifying integrity of game assets` stall. This PR does **not** optimize or cache that verification. A physical probe run should first establish whether `assets_verify_download` dominates the concurrent group and whether it ever emits `network_download_begin`; only then should a separate optimization PR evaluate incremental verification.

The historical BootOptim process-start→menu numbers and this launcher Start→Java boundary are different metrics. Neither this probe nor an AppCDS training run should be used to convert one into the other.
