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
- final `--module-path` / `-p` entries **in their original JVM order**, each with path, size and SHA-256; duplicate/ambiguous module-path declarations or non-file entries make AppCDS ineligible;
- top-level mod JAR fingerprints. Their plan representation is sorted only to canonicalize this unordered fingerprint set; it does not change launch/FML ordering;
- a strong local snapshot of regular files under `config/`, `defaultconfigs/`, `kubejs/` and `scripts/`, sorted only in the identity representation. An unexpected symlink/reparse/unsupported entry in these roots makes AppCDS ineligible rather than following an ambiguous tree;
- `resource_pack_selection_sha256`, derived only from `resourcePacks`/`incompatibleResourcePacks` in `options.txt`, so unrelated graphics/keybind option edits do not invalidate the archive;
- `pack_manifest_sha256`, a canonical digest over top-level mod JAR identity, the launch-affecting pack-input snapshot and resource-pack-selection fingerprint. In v0 this is a **local strong invalidation manifest**, not a signed distribution manifest and never an automatic repair authority;
- presence only, never contents, for `JAVA_TOOL_OPTIONS`, `_JAVA_OPTIONS`, and `JDK_JAVA_OPTIONS`. Any presence makes AppCDS ineligible because the final hidden JVM command would not be fully known.

The exact-pack architecture research requires pack-manifest/config identity in addition to Java/classpath/mod identity. v0 therefore refuses AppCDS activation if the local pack manifest cannot be established (for example missing `mods/`, no top-level mod JAR, missing/unparseable `options.txt` resource-pack selection, or unreadable/ambiguous pack-input tree). Stock launch remains available.

Java module options whose exact CDS semantics are not part of this v0 are explicitly fail-closed: `--upgrade-module-path`, `--patch-module`, and `--limit-modules` (including `--option=value` forms) make activation ineligible. They are recorded through the argv fingerprint but never interpreted into an AppCDS-compatible tuple.

`launch-plan.match` becomes `MATCH` only when the previous plan bytes and current plan bytes are exactly equal. Hash calculation is read-only; the cross-process cache lock is acquired **before** publishing `launch-plan.json`, `launch-plan.sha256`, `launch-plan.match` or any state transition, so concurrent preflights cannot make the match file describe another launch. Lock contention returns `STOCK` without publishing a plan/state update. `launch-plan.match` is diagnostic rather than a generation gate: a first eligible identity in Windows auto mode may train immediately. `training.meta` persists that training identity, and no archive may be promoted or consumed until a later preflight independently rebuilds the same identity. Any intervening identity mismatch invalidates the pending training result and launches stock.

### Deterministic library/classpath order contract

The pinned upstream `LaunchRuleContext::collect_libraries` selected duplicate winners in a `HashMap` and then emitted `into_values()`, so hash-table iteration randomized the effective library/classpath order. v0 now uses the map only for coordinate lookup and keeps a separate encounter-ordered winner vector. No alphabetical/path/Maven sort is introduced.

The exact contract is: rules are evaluated first; duplicate coordinates use the pre-existing version comparator unchanged; an older later entry is ignored; an equal or newer later entry remains the winner exactly as before, retires the previous slot, and is emitted at the later winning entry's own encounter position. Thus the final sequence is the encounter/resolution order of the winning library occurrences, while the relative precedence of non-duplicate winners is unchanged. Artifact order inside each winning library remains main artifact first and selected native classifier second, exactly as before.

Tests cover repeated equivalent collections, increasing/decreasing/equal duplicate versions, classifier/native emission and rule-excluded duplicates. CI additionally reruns the deterministic-order test in independent test processes. `launch-plan.match` remains useful for diagnostics, but READY safety is anchored to the independently rebuilt current identity matching `training.meta` during promotion and `ready.meta` during later reuse; any mismatch remains fail-closed for AppCDS and fail-open to stock launch.

## AppCDS state machine

Cache ownership is per instance: `<instance>/.bootoptim/appcds/`. No file is written into the Java installation and no global Java/OS setting is changed.

- `ABSENT`: no ready archive. The first eligible identity in Windows auto mode writes `training.meta` and returns `TRAIN` immediately, even when `launch-plan.match` is `FIRST_OR_MISMATCH`. Pandora directly starts Java with `-Xshare:auto -XX:ArchiveClassesAtExit=<instance>/.bootoptim/appcds/training.jsa`.
- `GENERATING`: persisted while a training result is pending. A concurrent/early launch that sees the same `training.meta` without a valid `training.complete` launches stock and does not start another training writer, even if HotSpot has already created a partial or final-looking `training.jsa`. A later different identity invalidates/discards the pending training result and launches stock.
- `READY`: only a later independently rebuilt exact identity matching `training.meta`, an exact clean-exit `training.complete`, and a non-empty `training.jsa` may promote to hashed `ready.jsa`/`ready.meta`; that same later launch may then consume it with `-Xshare:auto -XX:SharedArchiveFile=<ready>`. Subsequent reuse requires the current identity and ready archive hash/size to match `ready.meta`.
- `STALE`: pack/Java/argv/config identity or ready metadata/archive hash/size mismatch. Launch stock; do not consume or overwrite the ready archive. A mismatching pending training campaign is invalidated so it cannot become trusted if the old identity reappears later.
- `FAILED`: malformed/orphan training state, corrupt training metadata/completion, conflicting staging, a completion marker without a usable archive, or ineligible identity. Launch stock. Ambiguous training files are invalidated/isolated rather than promoted.

A cross-process Windows file lock protects plan publication and cache transitions. Lock contention launches stock. Ready promotion uses non-existing rename targets; metadata is staged and synced first. If metadata promotion fails after archive rename, the new ready archive is removed. `AutoCreateSharedArchive` is never used.

The training archive is produced by the exact Java process at VM exit. Pandora keeps that Java process as its normal direct `PandoraProcess`. Only after `wait`/`try_wait` observes a **clean exit code 0** does Pandora write `training.complete`. The helper requires that marker before promotion. A crash, force-kill, failed spawn, still-running Java, or archive file appearing before Pandora has observed clean termination cannot become `READY`. `state.meta` therefore remains `GENERATING` after training until a later exact preflight sees all three training artifacts and performs the promotion.

v0 rejects AppCDS activation when it sees Java/JVMTI agents, `-XX:+AllowArchivingWithJavaAgent`, any existing `SharedArchiveFile`/`ArchiveClassesAtExit`/`AutoCreateSharedArchive`/`-Xshare:*` setting, `--upgrade-module-path`, `--patch-module`, `--limit-modules`, an unhashable/non-file classpath or module-path entry, missing pack/config identity, missing other identity input, or hidden JVM option environment variables. `AutoCreateSharedArchive` is never used.

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

The `BootOptim v0 interposer` Actions workflow is pinned to `windows-2022` for the delivery gate and uploads both binaries plus `SHA256SUMS.txt` as `bootoptim-pandora-v0-windows-x86_64`. It also contains an independent Ubuntu helper-test job solely to distinguish workflow/code failures from hosted-runner provisioning failures. **Do not use a local/manual artifact for the good-PC gate while the Windows Actions job has not executed real steps and completed successfully.**

Unit tests cover deterministic winning-library encounter order (including duplicate versions, natives/classifiers and excluded rules), Windows `CREATE_NO_WINDOW` pipe preservation, SHA-256, structured Unicode/space arguments, exact classpath and module-path string/order preservation, module-path JAR hash invalidation, sensitive-value redaction, exclusive same-process and cross-process locks, immediate first-identity training, concurrent/pending and partial-archive stock fallback, clean-exit plus later-identity promotion, intervening-identity training invalidation, corrupt training metadata/completion no-promote behavior, incomplete staging, promotion and rollback-visible stale states, byte-for-byte plan comparison, classpath JAR changes, Java absolute-path/binary changes, launch-affecting config changes, resource-pack-selection changes, stale no-consume behavior, Java/JVMTI agent/conflicting-CDS rejection, and explicit fail-closed handling for `--upgrade-module-path`, `--patch-module`, and `--limit-modules`.

## Good-PC protocol — no timing claim yet

Use the Windows Actions artifact **only after its Windows build/test job is green and `SHA256SUMS.txt` has been verified from that artifact**. Do not modify the productive modpack for mutation tests; those identity-change cases are automated in unit tests. `BOOTOPTIM_APPCDS_MODE=plan` remains available as a stock-only diagnostic, but a two-run plan proof is no longer a prerequisite for training.

1. Extract `BootOptimPandora-v0.exe`, `bootoptim-launch-interposer.exe`, and `SHA256SUMS.txt` into a temporary test folder. Verify both SHA-256 values with `Get-FileHash`. Keep the normal Pandora executable untouched; run the v0 executable side-by-side.
2. In **one temporary PowerShell session** set:
   ```powershell
   $env:BOOTOPTIM_LAUNCH_INTERPOSER = (Resolve-Path .\bootoptim-launch-interposer.exe).Path
   $env:BOOTOPTIM_APPCDS_MODE = 'auto'
   .\BootOptimPandora-v0.exe
   ```
   For a fresh eligible AppCDS cache, the first launch may still publish `launch-plan.match=FIRST_OR_MISMATCH`, but it must return `TRAIN` immediately and Pandora should log `BOOTOPTIM_INTERPOSER status=generating activation=training`. This removes the previous proof-only game launch.
3. Exit Minecraft normally after reaching the intended menu. Only after Pandora observes the direct Java process exit with code 0 may `training.complete` exist. Require `training.meta`, a non-empty `training.jsa`, and exact contents `complete\n` in `training.complete`. A crash, force-kill, failed spawn, still-running Java, or archive that appears without that marker remains non-consumable and the next launch must fall back to stock.
4. Launch once more with the exact same Java/classpath/module-path/JAR/config/resource-selection identity and auto mode. This later preflight independently rebuilds identity, must match `training.meta`, promotes to hashed `ready.jsa`/`ready.meta`, and may then return `READY` for that same launch. Require `state.meta=READY`; the training files should be gone. Only this second launch is a candidate archive-consumption observation.
5. If the later identity differs, or if training metadata/completion/archive state is malformed, the helper must return `STOCK`, must not create/use `ready.jsa`, and must invalidate/discard the pending training campaign. Do not weaken the identity inputs to force a match.
6. Verify ordinary fallback independently: close v0, set `BOOTOPTIM_LAUNCH_INTERPOSER` to a nonexistent path, relaunch once, and confirm the instance still starts through Pandora's stock path. Restore the valid helper path afterward. This does not edit instance/Java configuration.
7. Confirm Pandora's normal Stop/Close behavior still controls the Java process. Java must remain Pandora's direct tracked child; a build that leaves Java running after Pandora stops it is a hard NO-GO.
8. If Java, its absolute path, a classpath JAR, module-path JAR/order, mod JAR, launch-affecting config/script, resource-pack selection, Pandora/helper binary, or the final ordered classpath changes after a READY archive exists, that READY archive is not consumed for the changed identity. The stale ready archive is retained for an exact identity returning later; v0 does not silently overwrite it with a new campaign.

No laptop run is requested by this prototype. A later performance campaign, if authorized, must separate launcher/setup wall time (including strong hashing), process-start to usable menu, training cost, and cold/warm cache state.

## Uninstall / restore

Close Pandora and Minecraft. In the PowerShell session:

```powershell
Remove-Item Env:BOOTOPTIM_LAUNCH_INTERPOSER -ErrorAction SilentlyContinue
Remove-Item Env:BOOTOPTIM_APPCDS_MODE -ErrorAction SilentlyContinue
```

Delete the temporary `BootOptimPandora-v0.exe` and `bootoptim-launch-interposer.exe`. Delete `<instance>\.bootoptim\` if the local plans/archive are no longer wanted. Start the user's original Pandora executable. No Java installation, registry key, global environment variable, OS setting, Microsoft credential, instance launcher setting, or product modpack file is changed by the prototype, so no further restoration is required.
