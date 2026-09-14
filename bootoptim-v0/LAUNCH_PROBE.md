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

The GUI **Start** action truncates/starts the selected JSONL file for that launch. Run exactly one probed launch at a time; a second Start while the first probe is active is ignored by the probe rather than truncating the first artifact.

After the one physical run, close Pandora and clear the opt-in variable:

```powershell
Remove-Item Env:BOOTOPTIM_LAUNCH_PROBE -ErrorAction SilentlyContinue
Get-FileHash $probe -Algorithm SHA256
Copy-Item $probe <offline-destination>
```

The main agent can then consume the copied JSONL offline together with its SHA-256. The artifact itself never contains the output filename, full instance path, Java path, account UUID/name, access token, command line, JVM arguments, URLs, or file names.

## JSONL schema

Every line is one JSON object with `schema="bootoptim.launch_probe.v1"` and a platform monotonic `mono_ns` timestamp. Windows uses `QueryPerformanceCounter`; Linux/macOS use `CLOCK_MONOTONIC`. Timestamps are comparable only within one boot/run and are intentionally not wall-clock timestamps.

Common fields:

- `phase`: fixed phase identifier.
- `event`: `instant`, `begin`, `end`, `inclusive_begin`, `inclusive_end`, `observed`, or `unobserved` where applicable.
- `network`: `true` only for a fixed `network_request` / `network_download` observation.
- `source`: a fixed non-sensitive source label. No URL, hostname, filename, account identifier, or token is persisted.
- `outcome`: fixed `ok`, `error`, or `cancelled` where applicable.

The expected phase vocabulary is:

1. `launch_request` — GUI Start message handed to the backend.
2. `instance_config` — retrieval/reload of the `InstanceConfiguration` immediately after that concrete Start dispatch. No instance identity/path is written. Pandora has no separate monolithic config-validator call in this path; later semantic checks stay in their stock phases.
3. `account_selection` — account selection/login acquisition required for launch. No identity is recorded. If Pandora actually enters its `Logging in` tracker rather than using an already-valid cached/offline account, the probe emits `network_request source="account_login"`.
4. `prelaunch` — Pandora's existing prelaunch work before the launcher pipeline. Any existing progress tracker whose title explicitly switches to `Downloading ...` but is not one of the Java/assets/library trackers emits `network_download source="tracked_other"`.
5. `version_loader_resolution` — Minecraft/loader launch-version resolution. The pinned parent `Launching` progress contract closes this phase only after `create_launch_version` has returned: Vanilla `2/7`, Fabric `5/10`, Forge/NeoForge `8/13`. Forge/NeoForge may create nested Java/library verifier trackers before that boundary; they therefore cannot be mistaken for the later outer group. The Forge/NeoForge `1/13` boundary is immediately before `create_forgelike` constructs the join containing the remote installer SHA-1 request, so the probe emits `network_request source="loader_sha1"` once for that route. This is an instantaneous route observation, not a network-duration estimate.
6. `java_runtime`, `assets_verify_download`, `libraries_classpath_inputs` — the observable top-level concurrent preparation branches in Pandora's existing `try_join4`. Because the parent progress boundary above is reached before those futures are polled, top-level attribution is race-free even if the asset-index metadata fetch stalls before the asset tracker itself is created. A custom Java path may legitimately have no `java_runtime` tracker.
7. `network_download` — an instantaneous observation when an existing tracker enters Pandora's explicit download path. Fixed `source` values are `assets`, `libraries`, `java_runtime`, or `tracked_other`. A long asset span with no `source="assets"` observation is therefore evidence that the measured asset time was local verification rather than asset download.
8. `network_request` — an instantaneous observation for route-level HTTP that can be established without inspecting URLs or payloads. Fixed sources in v1 are `account_login` and `loader_sha1`.
9. `classpath_resolution`, `native_extraction`, and `wrapper_arguments` — deliberately **inclusive and overlapping** post-join envelopes. They begin only on the pinned parent progress increment that occurs after the complete `try_join4` returns: Vanilla `6/7`, Fabric `9/10`, Forge/NeoForge `12/13`. Therefore the untracked `log_configuration` sibling has already completed at this boundary. They end when Pandora has constructed the final structured Minecraft command. Do not treat these scopes as exclusive timings or sum them.
10. `appcds_preflight` — optional span emitted only when the existing BootOptim v0 helper actually reaches its preflight subprocess. It measures helper execution separately from wrapper preparation and Java spawn without changing AppCDS decisions.
11. `java_spawn` — direct Java OS process-creation begin/end on Pandora's command-spawner thread. It is emitted only when the direct executable is `java`, `java.exe`, or `javaw.exe` and the Pandora LaunchWrapper marker is present. A custom external wrapper is therefore not falsely labelled as Java creation.
12. `launcher_pre_java` — successful end boundary for Start→direct Java process creation. Compute the duration as `launcher_pre_java.end.mono_ns - launch_request.mono_ns`; do not add child spans because several overlap.
13. `java_to_menu` — emitted as `unobserved`, `observed=false`, `duration_ns=null` immediately after successful direct Java creation. Pandora has no menu-ready signal usable here without reading/interpreting game output, and this PR intentionally does not read game logs to manufacture a TTMM value.
14. `launch` — fixed terminal error/cancellation event when Pandora's existing modal action reports one before successful Java creation.

### Network coverage boundary

The probe records actual explicit downloader transitions from Pandora progress trackers plus the two route-level network requests above. It does **not** hook `reqwest` globally or inspect URLs, so small metadata refreshes are not claimed as individually timed HTTP spans. Their elapsed time remains inside the owning launch phase. This keeps the diagnostic low-intrusion and avoids attributing unrelated background HTTP to the measured launch. For the HDD question, the important distinction is direct: the asset verifier's span and whether it ever emitted `network_download source="assets"`.

### Why the parent progress contract is pinned

This PR measures one exact Pandora base, not arbitrary future launcher versions. In that base, `launch_tracker.set_total(6)` covers the six outer launch steps, while loader-specific `create_launch_version` work extends the total by `+1` Vanilla, `+4` Fabric, or `+7` Forge/NeoForge. The outer code then increments once immediately after `create_launch_version`, once in each Java/assets/libraries branch, once immediately after `try_join4`, and once after game process creation. Unit tests pin all three observed shapes (`7`, `10`, `13`) and both semantic boundaries (`total-5`, `total-1`). If that contract changes in a future rebase, tests/docs must change with it rather than silently shifting attribution.

## One-run laptop protocol

1. Use the Windows artifact from this PR only after its Windows Actions build/test job is green and verify `SHA256SUMS.txt` as in the existing v0 protocol.
2. Keep the AppCDS mode and instance state exactly as intended for the run; this probe records an existing AppCDS preflight separately but does not reinterpret an AppCDS training run as a performance result.
3. Set `BOOTOPTIM_LAUNCH_PROBE` to a new local JSONL filename **before starting Pandora**.
4. Start Pandora, select the intended instance/account, press **Start exactly once**, and allow Pandora to reach successful direct Java creation. Do not modify the instance during the run and do not configure an external wrapper for this measurement.
5. Once the Minecraft Java process has been created, the required pre-Java measurement is complete. Reaching the main menu can be noted separately by the physical operator, but this JSONL deliberately does not claim or infer Java→menu time.
6. Exit normally, remove the environment variable, hash the JSONL, and copy the JSONL plus its hash to the offline handoff location.
7. Inspect spans by monotonic timestamp. Treat `java_runtime`, `assets_verify_download`, and `libraries_classpath_inputs` as parallel siblings. Treat `classpath_resolution`, `native_extraction`, and `wrapper_arguments` as inclusive overlapping post-join envelopes. `appcds_preflight`, when present, is later and serial before `java_spawn`. Never sum overlapping scopes to derive Start→Java.

## Critical path and current diagnostic hypothesis

Pandora starts Java-runtime preparation, asset verification/download, library verification/download, and log-configuration loading concurrently and waits for all of them before post-join classpath/native/wrapper preparation. Therefore the longest unfinished sibling is on the critical path; sibling durations are not additive. The log-configuration branch has no dedicated child span in this revision, but the `total-1` post-join boundary guarantees that any log-config tail is finished before the later inclusive scopes begin.

The current code checks the SHA-1 of every asset object in the asset index before reusing it. On an HDD, that full verification fan-out is a plausible explanation for the physically observed long `Verifying integrity of game assets` stall. This PR does **not** optimize or cache that verification. A physical probe run should first establish whether `assets_verify_download` dominates the concurrent group and whether any `network_download` event with `source="assets"` is observed; only then should a separate optimization PR evaluate incremental verification.

The historical BootOptim process-start→menu numbers and this launcher Start→Java boundary are different metrics. Neither this probe nor an AppCDS training run should be used to convert one into the other.
