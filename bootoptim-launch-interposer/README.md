# BootOptim launch interposer v0

Narrow measurement prototype for Pandora Launcher. This is not a launcher rebrand and does not implement authentication, downloads, updates, instance management or UI.

## Provenance and license

- Host fork: `wachipayox/BootOptimPandora`.
- Upstream: `Moulberry/PandoraLauncher`.
- Upstream/fork base commit: `4eb6c7849561151695288443c106519774ee05ea`.
- Pandora license at that commit: MIT, copyright (c) 2025 Moulberry. The repository `LICENSE` remains unchanged.
- BootOptim authority consulted for this prototype: `agent/integration-current@b3f0c5f6462a359483883741ac16d2f154868900`, including PR #266, #280 and #281.

The helper is an opt-in wrapper for Pandora's existing structured launch path. Pandora already inserts its configured wrapper before the Java executable and appends Java/arguments as separate `PandoraCommand::arg` values. The helper therefore receives:

```text
bootoptim-launch-interposer.exe [helper options] <java executable> <original Java argv...>
```

It never reparses a command-line string and never sorts/rebuilds the classpath. The only wrapper string Pandora parses is the existing instance `wrapper_command` configuration used to locate this executable and select its mode.

## Safety model

The current working directory supplied by Pandora is used as the game/instance-local root. All state lives under `.bootoptim/appcds/`; nothing is written to the Java installation or global OS configuration.

The canonical `launch-plan.json` contains Java canonical path + executable hash + JDK `release` hash/vendor/version, OS/architecture, helper hash/version, the exact classpath entries in the exact supplied order plus each file hash/size, ordered hashes of JVM arguments, Pandora's LaunchWrapper main class, and a canonical identity list of instance `mods/**/*.jar`. Mod paths are sorted only while constructing the identity manifest; that ordering is never returned to Pandora or NeoForge. Post-main/game arguments are neither persisted nor logged, so access tokens and other game arguments are not copied into the plan.

Existing `-javaagent`, `-agentlib`, `-agentpath`, `-XX:+AllowArchivingWithJavaAgent`, `-Xshare:*`, `-XX:ArchiveClassesAtExit=*` or `-XX:SharedArchiveFile=*` JVM configuration causes fail-open stock launch rather than being overridden.

Cache states are `ABSENT`, `GENERATING`, `READY`, `STALE`, and `FAILED`. A cross-process exclusive file lock protects cache mutation. A busy lock launches stock immediately. Training writes a unique same-directory staging archive and only promotes a non-empty archive after Java exits successfully. Consumption requires both `ready.json` and `appcds-ready.jsa`, exact launch-plan SHA-256, helper version and archive SHA-256. Any mismatch is `STALE` and launches stock without CDS flags.

The helper inherits stdin/stdout/stderr for the Java child and emits no Java arguments. Its own state is written to files only.

## Modes

`--mode plan` is the default and always launches stock. It writes the deterministic plan and current cache state but never adds CDS flags.

`--mode train` launches the original Java command prefixed only with:

```text
-Xshare:auto
-XX:ArchiveClassesAtExit=<instance-local unique staging file>
```

This is training/provisioning and must never be reported as a first-launch speed improvement. Promotion happens only after a successful Java exit.

`--mode auto` consumes only a `READY` exact-match archive and prefixes:

```text
-Xshare:auto
-XX:SharedArchiveFile=<instance-local ready archive>
```

`ABSENT`, `STALE`, `FAILED`, lock contention or helper setup errors launch the original Java command unchanged.

## Reproducible build

Windows x86-64, from repository root:

```powershell
cd bootoptim-launch-interposer
cargo test
cargo build --release
Get-FileHash .\target\release\bootoptim-launch-interposer.exe -Algorithm SHA256
```

CI workflow `.github/workflows/bootoptim-interposer.yml` runs formatting/tests/build on `windows-latest` and uploads `bootoptim-launch-interposer-windows-x86_64` containing the executable and `SHA256SUMS.txt`.

## Good-PC protocol (do not skip phases)

Do not modify the production modpack first. Use the Pandora instance selected for this prototype and close Pandora before editing its instance configuration.

1. Copy `bootoptim-launch-interposer.exe` to a stable file location owned by that test instance. Do not place it under the JDK/JRE installation. Record its SHA-256 from `SHA256SUMS.txt`.
2. Enable Pandora's existing **Wrapper command** for the test instance with a quoted absolute executable path and `--mode plan`, for example: `"D:\Pandora Test\BootOptim Tools\bootoptim-launch-interposer.exe" --mode plan`.
3. Launch twice through Pandora in `plan` mode, reaching the same intended point each time. After each launch copy `.bootoptim\appcds\launch-plan.json` aside as `plan-a.json` and `plan-b.json`. Compare byte-for-byte with `fc /b plan-a.json plan-b.json` (or hashes). They must be identical before AppCDS activation is permitted. Do not publish the plan because it contains local Java/classpath paths.
4. While still in `plan` mode, deliberately alter one identity input in a disposable copy/test fixture (for example one JAR byte or selected Java path), launch, and confirm `.bootoptim\appcds\state.json` reports `STALE` when a prior ready tuple exists. Restore the input and verify the original deterministic plan. This phase must remain stock.
5. Change only the wrapper suffix to `--mode train`. Launch normally. Training is not a benchmark sample. Exit Minecraft normally so `ArchiveClassesAtExit` can finish. After exit, require `state.json=READY`, `ready.json`, and non-empty `appcds-ready.jsa`. A killed/crashed training run must not produce a consumable `READY` tuple.
6. Change only the wrapper suffix to `--mode auto`. Launch again. Before making any performance claim, verify the plan is byte-identical to the trained tuple and state remains `READY`. Separately validate the same resource/reload contract used by BootOptim exact-pack work. Keep launcher/setup wall time and Java process-start -> menu time as separate measurements.
7. Concurrency gate: with one launch still running under `train` or `auto`, start a second launch of the same instance. The second helper must fail open to stock because the cache lock is held; it must not mutate/promote the cache.

If Pandora's wrapper configuration cannot represent the quoted helper path correctly on the target Windows installation, stop: that is a **NO-GO for activation**. Do not work around it by alphabetizing/rebuilding classpath or concatenating the Java command into a shell string.

## Uninstall / restore

Close Minecraft and Pandora. Disable/clear the instance **Wrapper command** (or restore its exact pre-test value), then delete the copied `bootoptim-launch-interposer.exe` and the instance-local `.bootoptim` directory. No Java files, registry keys, environment variables, account data, launcher updater state, or global OS settings are changed by this prototype.
