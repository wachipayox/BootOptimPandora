# BootOptim launcher v0 prototype

This directory is an intentionally narrow measurement prototype. It is **not** a general launcher, rebrand, account implementation, updater, downloader, or product release.

## Provenance and scope

- Host launcher: Moulberry/PandoraLauncher, MIT.
- Audited upstream commit and fork `master` base: `4eb6c7849561151695288443c106519774ee05ea`.
- Working repository: private `wachipayox/BootOptimPandora`.
- Existing root `LICENSE` is retained. See `NOTICE.md` for prototype provenance.
- BootOptim `agent/integration-current` is not modified by this prototype.

Pandora continues to own its UI, Microsoft authentication, instance management, downloads, process lifecycle and update behavior. The prototype only prepares AppCDS state immediately before Pandora's normal structured Java spawn.

## Why the integration is structured

Pandora's `PandoraCommand` stores the executable and every argument as `OsStr`/`OsString`; the prototype never serializes the Java command through a shell. When `BOOTOPTIM_LAUNCH_INTERPOSER` points to the helper, `PandoraCommand::spawn` runs that helper as a short **preflight** process with the direct Java executable and JVM argv passed as distinct OS arguments. The helper returns only one fixed decision on stdout: `STOCK`, `TRAIN`, or `READY`.

Pandora then adds zero or two AppCDS JVM flags and spawns the **original Java executable directly** through its normal spawner. Java therefore remains the process represented by `PandoraProcess`: the existing stdin pipe, stdout/stderr handling, WM_CLOSE/TerminateProcess behavior and Windows Job Object ownership are preserved. The helper never becomes the long-lived parent/proxy for Minecraft.

The hook is deliberately inert when:

- the environment variable is absent or the helper does not exist;
- this is not the Pandora Minecraft `com.moulberry.pandora.LaunchWrapper` launch;
- a user wrapper command precedes Java;
- Pandora uses its separate sandbox/elevated spawn path;
- the helper fails, returns an unknown decision, or any eligibility check fails.

Those cases launch stock. v0 does not guess wrapper/sandbox semantics.

Pandora supplies Minecraft game arguments, including the authentication token, **after process creation over stdin** to `LaunchWrapper`. The preflight helper runs with stdin disconnected and sees only the final JVM-side command that already existed before spawn; it never reads the post-spawn game/auth protocol. Its stdout is captured by Pandora and is restricted to the fixed decision word. Pandora logs only fixed BootOptim state labels.

## Canonical launch plan

Each preflight writes an instance-local `.bootoptim/appcds/launch-plan.json`. The plan is canonical and has no timestamp/PID. It records:

- exact Java executable path representation, executable SHA-256, `<java-root>/release` SHA-256, vendor and version;
- OS/architecture and helper version;
- Pandora upstream commit and hashes of the running Pandora executable and helper;
- every final JVM argv position and a fingerprint of non-sensitive values; sensitive-looking values are `REDACTED` and never persisted;
- final classpath entries **in their original order**, each with path, size and SHA-256;
- top-level mod JAR fingerprints. Their plan representation is sorted only to canonicalize this unordered fingerprint set; it does not change launch/FML ordering;
- presence only, never contents, for `JAVA_TOOL_OPTIONS`, `_JAVA_OPTIONS`, and `JDK_JAVA_OPTIONS`. Any presence makes AppCDS ineligible because the final hidden JVM command would not be fully known.

`launch-plan.match` becomes `MATCH` only when the previous plan bytes and current plan bytes are exactly equal. AppCDS auto mode refuses generation until that proof exists.

### Important Pandora upstream limitation

At the pinned upstream commit, `LaunchRuleContext::collect_libraries` deduplicates libraries in `std::collections::HashMap` and then iterates `deduplicated_libraries.into_values()`. v0 **does not sort or otherwise normalize this order** because that could change classpath precedence.

Therefore the real PC test is authoritative: if two otherwise identical Pandora launches produce different `launch-plan.json` bytes/classpath order, AppCDS activation is **NO-GO** for this host revision. The preflight remains useful as a launch-plan proof and Pandora continues to launch stock. A later mismatch is also safe: the exact plan hash is checked on every READY consumption and a mismatch becomes `STALE` with no archive flags.

## AppCDS state machine

Cache ownership is per instance: `<instance>/.bootoptim/appcds/`. No file is written into the Java installation and no global Java/OS setting is changed.

- `ABSENT`: no ready archive. With a proven deterministic plan in Windows auto mode, preflight writes `training.meta` and returns `TRAIN`. Pandora directly starts Java with `-Xshare:auto -XX:ArchiveClassesAtExit=<instance>/.bootoptim/appcds/training.jsa`.
- `GENERATING`: persisted while a training result is pending. A concurrent/early launch that sees `training.meta` without `training.complete` launches stock and does not start another training writer, even if HotSpot has already created a partial or final-looking `training.jsa`.
- `READY`: only a later exact preflight with matching `training.meta`, `training.complete`, and a non-empty `training.jsa` may promote to hashed `ready.jsa`/`ready.meta`; that same launch may then consume it with `-Xshare:auto -XX:SharedArchiveFile=<ready>`.
- `STALE`: launch-plan, ready metadata or archive hash/size mismatch. Launch stock; do not consume or overwrite the ready archive.
- `FAILED`: malformed/orphan cache state, conflicting staging, a completion marker without a usable archive, or ineligible identity. Launch stock. v0 favors inspection/manual reset over silently rewriting ambiguous state.

A cross-process Windows file lock protects preflight/cache transitions. Lock contention launches stock. Ready promotion uses non-existing rename targets; metadata is staged and synced first. If metadata promotion fails after archive rename, the new ready archive is removed. `AutoCreateSharedArchive` is never used.

The training archive is produced by the exact Java process at VM exit. Pandora keeps that Java process as its normal direct `PandoraProcess`. Only after `wait`/`try_wait` observes a **clean exit code 0** does Pandora write `training.complete`. The helper requires that marker before promotion. A crash, force-kill, failed spawn, still-running Java, or archive file appearing before Pandora has observed clean termination cannot become `READY`. `state.meta` therefore remains `GENERATING` after training until a later exact preflight sees all three training artifacts and performs the promotion.

v0 rejects AppCDS activation when it sees Java/JVMTI agents, `-XX:+AllowArchivingWithJavaAgent`, any existing `SharedArchiveFile`/`ArchiveClassesAtExit`/`AutoCreateSharedArchive`/`-Xshare:*` setting, an unhashable/non-file classpath entry, missing identity input, or hidden JVM option environment variables. `AutoCreateSharedArchive` is never used.

## Reproducible Windows build/test

From this repository revision, with stable Rust/MSVC installed:

```powershell
cargo build --release --frozen --target x86_64-pc-windows-msvc
cargo test --manifest-path bootoptim-v0/interposer/Cargo.toml --locked --target x86_64-pc-windows-msvc
cargo build --manifest-path bootoptim-v0/interposer/Cargo.toml --release --locked --target x86_64-pc-windows-msvc
```

Outputs:

- patched Pandora: `target/x86_64-pc-windows-msvc/release/pandora_launcher.exe`;
- helper: `bootoptim-v0/interposer/target/x86_64-pc-windows-msvc/release/bootoptim-launch-interposer.exe`.

The `BootOptim v0 interposer` Actions workflow performs these gates on `windows-latest` and uploads both binaries plus `SHA256SUMS.txt` as `bootoptim-pandora-v0-windows-x86_64`.

Unit tests cover SHA-256, structured Unicode/space arguments, exact classpath string preservation, sensitive-value redaction, exclusive locks, incomplete staging, promotion and rollback-visible stale states, byte-for-byte plan comparison, JAR hash change, Java absolute-path change, Java executable change, stale no-consume behavior, and agent/conflicting-CDS rejection.

## Good-PC protocol — no timing claim yet

Use the Windows Actions artifact. Do this on the good PC only after CI is green. Do not modify the productive modpack for the mutation tests; those identity-change cases are automated in unit tests.

1. Extract `BootOptimPandora-v0.exe`, `bootoptim-launch-interposer.exe`, and `SHA256SUMS.txt` into a temporary test folder. Verify both SHA-256 values with `Get-FileHash`. Keep the normal Pandora executable untouched; run the v0 executable side-by-side.
2. In **one temporary PowerShell session** set:
   ```powershell
   $env:BOOTOPTIM_LAUNCH_INTERPOSER = (Resolve-Path .\bootoptim-launch-interposer.exe).Path
   $env:BOOTOPTIM_APPCDS_MODE = 'plan'
   .\BootOptimPandora-v0.exe
   ```
   Launch the intended instance normally. This launch is stock Java behavior; the helper only records the JVM launch plan before Pandora directly starts Java.
3. After exit, copy `<instance>\.bootoptim\appcds\launch-plan.json` to `launch-plan.first.json` outside the cache. Repeat the same plan-only launch without changing Java/instance/Pandora. Require `launch-plan.match` to contain exactly `MATCH` and compare the two files byte-for-byte (`Compare-Object` or hashes). If they differ, stop: **NO-GO for AppCDS activation on this Pandora revision**. Do not reorder the classpath to make it pass.
4. Verify fallback before training: close v0, set `BOOTOPTIM_LAUNCH_INTERPOSER` to a nonexistent path, relaunch v0 once, and confirm the instance still starts through Pandora's stock path. Restore the valid helper path afterward. This does not edit instance/Java configuration.
5. Only after the two-plan gate passes, set `$env:BOOTOPTIM_APPCDS_MODE = 'auto'` and launch once. This is the **training/generation run**, not a performance result. Pandora should log `BOOTOPTIM_INTERPOSER status=generating activation=training`. Exit Minecraft normally after reaching the intended menu. After Pandora observes the Java exit, require `training.meta`, a non-empty `training.jsa`, and `training.complete`; `state.meta` is expected to remain `GENERATING` until the next preflight. If the process was killed/crashed or `training.complete` is absent, do not continue to consumption.
6. Launch once more with the exact same tuple and auto mode. Its preflight must require all three training artifacts, promote the archive, then Pandora should log `BOOTOPTIM_INTERPOSER status=ready activation=enabled` and directly spawn Java with the ready archive. Require `state.meta=READY`, `ready.jsa`, and `ready.meta`; `training.meta`, `training.jsa`, and `training.complete` should be gone. Only this run is a candidate archive-consumption observation. Do not infer a startup saving from generation or from this single consumption run.
7. Confirm Pandora's normal Stop/Close behavior still controls this Java process during the test. This is a direct-child invariant of the v0 design; a build that leaves Java running after Pandora stops it is a hard NO-GO.
8. If Java, its absolute path, a classpath JAR, mod JAR, Pandora/helper binary, or the final ordered classpath changes, the plan changes. READY is not used; state becomes `STALE` and the launch is stock. v0 intentionally does not silently retrain a stale cache. Delete the instance-local `.bootoptim/appcds` directory only when deliberately starting a new training campaign.

No laptop run is requested by this prototype. A later performance campaign, if authorized, must separate launcher/setup wall time, process-start to usable menu, training cost, and cold/warm cache state.

## Uninstall / restore

Close Pandora and Minecraft. In the PowerShell session:

```powershell
Remove-Item Env:BOOTOPTIM_LAUNCH_INTERPOSER -ErrorAction SilentlyContinue
Remove-Item Env:BOOTOPTIM_APPCDS_MODE -ErrorAction SilentlyContinue
```

Delete the temporary `BootOptimPandora-v0.exe` and `bootoptim-launch-interposer.exe`. Delete `<instance>\.bootoptim\` if the local plans/archive are no longer wanted. Start the user's original Pandora executable. No Java installation, registry key, global environment variable, OS setting, Microsoft credential, instance launcher setting, or product modpack file is changed by the prototype, so no further restoration is required.
