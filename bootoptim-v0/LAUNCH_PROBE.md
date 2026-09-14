# BootOptim Pandora launch probe v1

This diagnostic is an **opt-in measurement harness** for the Pandora launch path. It does not change AppCDS identity/state, MoreCulling handling, launch arguments, downloads, authentication, or game behavior.

## Activation and artifact

The probe is disabled unless `BOOTOPTIM_LAUNCH_PROBE` is present **before Pandora starts**. Its value is the output JSONL filename. Use a fresh filename for each physical run.

When disabled, the probe does not open, scan, hash, or create any diagnostic file and does not add a subprocess. The normal Pandora/AppCDS launch path is unchanged. Probe activation is cached once per Pandora process, so setting the variable after Pandora has already started is intentionally unsupported.

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
- `event`: `instant`, `begin`, `end`, `inclusive_begin`, `inclusive_end`, `observed`, or `unobserved` where applicable.
- `network`: `true` only for the fixed `network_download` observation emitted when Pandora enters one of its existing explicit download paths.
- `source`: fixed `assets`, `libraries`, or `java_runtime` only for `network_download`; no URL or filename is persisted.
- `outcome`: fixed `ok`, `error`, or `cancelled` where applicable.

The expected phase vocabulary is:

1. `launch_request` — GUI Start message handed to the backend.
2. `instance_config` — retrieval/reload of the selected `InstanceConfiguration`. The marker is tied to the concrete backend dispatch for that Start request without writing instance identity or path. Pandora has no separate monolithic config-validator call in this path; later semantic checks remain in their stock phases rather than being relabelled as config validation.
3. `account_selection` — account selection/login acquisition required for launch. No identity is recorded.
4. `prelaunch` — Pandora's existing prelaunch work before the launcher pipeline.
5. `version_loader_resolution` — Minecraft/loader launch-version resolution. NeoForge/Forge may perform Java or library verification while constructing the launch version; those nested trackers remain inside this phase and are **not** mistaken for the later top-level parallel group.
6. `java_runtime`, `assets_verify_download`, `libraries_classpath_inputs` — the three top-level concurrent branches of Pandora's existing `try_join4` launch preparation. Their begin/end spans start only after version/loader resolution has completed.
7. `network_download` — an instantaneous observation with fixed `source` when an existing Java-runtime/assets/libraries tracker enters Pandora's download path. It is not a claimed network-duration span. Such an observation can legitimately occur during `version_loader_resolution` for nested Forge/NeoForge work or during the later top-level parallel group.
8. `classpath_resolution`, `native_extraction`, and `wrapper_arguments` — deliberately **inclusive, overlapping post-join envelopes**. They begin at the conservative post-join boundary and end when Pandora has constructed the final structured Minecraft command. Pandora interleaves classpath construction/native extraction before command assembly; this diagnostic does not restructure that code to manufacture exclusive timings.
9. `appcds_preflight` — optional span emitted only when the existing BootOptim v0 helper reaches its actual preflight subprocess. It measures that helper execution separately from wrapper preparation and Java spawn; no helper decision or AppCDS state-machine semantics are changed by the probe.
10. `java_spawn` — OS process-creation call begin/end on Pandora's command-spawner thread.
11. `launcher_pre_java` — successful end boundary for Start→Java process creation. Compute the launcher-pre-Java duration as `launcher_pre_java.end.mono_ns - launch_request.mono_ns`; do not add child spans because several overlap.
12. `java_to_menu` — emitted as `unobserved`, `observed=false`, `duration_ns=null` immediately after successful Java creation. Pandora has no menu-ready signal usable here without reading/interpreting game output, and this PR intentionally does not read game logs to manufacture a TTMM value.
13. `launch` — fixed terminal error/cancellation event when Pandora's existing modal action reports one before successful Java creation.

### Why version/loader resolution is not closed on the first verifier tracker

The exact NeoForge/Forge path can create Java-runtime and library trackers while the launcher version itself is still being assembled. The probe therefore uses Pandora's existing parent `Launching` progress contract to identify the boundary where only the five stock outer steps remain. Unit tests pin the observed Vanilla/Fabric/Forge-like totals used by this revision. This adds no file reads, hashes, HTTP interception, or subprocesses.

## One-run laptop protocol

1. Use the Windows artifact from this PR after its Windows Actions build/test job is green and verify `SHA256SUMS.txt` as in the existing v0 protocol.
2. Keep the AppCDS mode and instance state exactly as intended for the run; this probe records an existing AppCDS preflight separately but does not reinterpret an AppCDS training run as a performance result.
3. Set `BOOTOPTIM_LAUNCH_PROBE` to a new local JSONL filename **before starting Pandora**.
4. Start Pandora, select the intended instance/account, press **Start exactly once**, and allow Pandora to reach successful Java creation. Do not modify the instance during the run.
5. Once the Minecraft Java process has been created, the required pre-Java measurement is complete. Reaching the main menu can be noted separately by the physical operator, but this JSONL deliberately does not claim or infer Java→menu time.
6. Exit normally, remove the environment variable, hash the JSONL, and copy the JSONL plus its hash to the offline handoff location.
7. Inspect spans by monotonic start/end. Treat `java_runtime`, `assets_verify_download`, and `libraries_classpath_inputs` as parallel siblings. Treat `classpath_resolution`, `native_extraction`, and `wrapper_arguments` as inclusive overlapping envelopes. `appcds_preflight`, when present, is later and serial before `java_spawn`. Never sum overlapping scopes to derive Start→Java.

## Critical path and current diagnostic hypothesis

Pandora starts Java-runtime preparation, asset verification/download, library verification/download, and log-configuration loading concurrently and waits for all of them before post-join classpath/native/wrapper preparation. Therefore the longest unfinished sibling is on the critical path; the sibling durations are not additive. The log-configuration branch has no dedicated progress tracker in this revision, so its cost remains included in the wait for the parallel join but is not falsely exposed as an exclusive child span.

The current code checks the SHA-1 of every asset object in the asset index before reusing it. On an HDD, that full verification fan-out is a plausible explanation for the physically observed long `Verifying integrity of game assets` stall. This PR does **not** optimize or cache that verification. A physical probe run should first establish whether `assets_verify_download` dominates the concurrent group and whether any `network_download` event with `source="assets"` is observed; only then should a separate optimization PR evaluate incremental verification.

Source inspection also shows that Forge/NeoForge launch-version construction performs a remote installer-SHA1 request inside `version_loader_resolution`. That is a separate optimization hypothesis only; this PR does not cache, eliminate, or reorder it.

The historical BootOptim process-start→menu numbers and this launcher Start→Java boundary are different metrics. Neither this probe nor an AppCDS training run should be used to convert one into the other.
