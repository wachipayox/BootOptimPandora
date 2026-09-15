# Agent 166 — PR7 root probe + PR10/PR11 USN runtime composition

## Exact provenance

This branch is a deliberate two-parent composition, not a reconstruction from binary strings:

- USN/runtime parent: PR #11 head `ccb319ed7a0fe5cd31f4465acaca5d695a3a2341`;
- PR #11 is stacked on PR #10 head `635171f4241320c0de4507fbb67ed45f1cfabd17`, which carries PR #9's foundation plus `AssetVerificationMode` intent propagation;
- launch-measurement parent: PR #7 final `55f05135396e9682a5d0f76a24e62f21850fbc03`;
- the lines diverge after common probe ancestor `48b12855f054302d53a305f1eab9e768508fb3e2`.

The merge keeps PR #11's asset/USN runtime tree and resolves the launch-probe files to final PR #7 semantics. The only extra source wiring is the packaged executable selftest boundary, derived from Agent 165's executable-spawn gate, plus CI/documentation. `crates/backend/src/launch/mod.rs` remains byte-identical to PR #11.

## Deliberate exclusions

This composition does not include PR #8's verification I/O scheduler/FIFO candidate, PR #14's asset-prefix `create_dir` deduplication, Automodpack changes, metadata/network policy changes, UI changes, AppCDS redesign, classpath reordering, or unrelated optimizations.

The USN fast path remains default-off. `BOOTOPTIM_ASSET_USN_CACHE=1` is only an explicit request and cannot authorize reuse unless PR #11's complete `Normal`-mode, canonical-layout, helper-pin, NTFS/journal/FileId/USN and same-handle gates succeed. `FullVerification`, GUI `Normal` without opt-in, CLI/legacy/default/unknown intent, helper/session uncertainty, non-NTFS/reparse, journal discontinuity, FileId/USN mismatch, inability to hold/recheck the protected handle, corrupt/partial state, or any other uncertainty retains Pandora's stock full SHA-1 behavior.

Only the ephemeral capability-minimal USN helper may be elevated. Pandora remains non-elevated and still creates Java/Minecraft itself; Java/Minecraft do not inherit the helper token. No service, driver, scheduled task, registry autorun or persistent privileged process is introduced.

## Packaged executable E2E gate

The Windows v0 workflow builds the AppCDS interposer, builds and hashes the USN helper, injects that helper digest as Pandora's compile-time pin, and builds the release `pandora_launcher.exe`.

Before artifact assembly, CI executes that exact release EXE twice against one fresh JSONL. The first invocation enters the real `--run-instance` / `BackendHandle` path with an opt-in CI-only sentinel, which arms the final PR #7 root and emits observed asset/library spans without invoking any production instance. The second invocation enters the existing internal command route, verifies the same JSONL already starts with `launcher_pre_java.begin`, constructs a real `PandoraCommand` for a disposable copy of `cmd.exe` named `java.exe`, sets a real current directory, and calls `PandoraCommand::spawn()`. The workflow points `BOOTOPTIM_LAUNCH_INTERPOSER` at the just-built interposer and uses plan mode, so the real AppCDS preflight is exercised before the fake Java spawn.

No new dependency is introduced between `bridge` and `command`: the two packaged invocations compose through the on-disk root contract exactly as the real bridge/command boundary does. `Cargo.lock` therefore remains unchanged and `--frozen` continues to gate the repository.

CI then independently parses the emitted JSONL and fails unless:

- every record is current `schema="bootoptim.launch_probe.v1"`;
- `launcher_pre_java.begin` is the first record and occurs exactly once;
- `launcher_pre_java.end` occurs exactly once;
- `java_spawn.begin/end` each occur exactly once and close before the root end;
- `appcds_preflight.begin/end` each occur exactly once and the preflight exits successfully, proving the matching built interposer path was exercised;
- classpath/native/wrapper remain one explicit `unobserved` event each;
- no `inclusive_begin` or `inclusive_end` legacy marker exists.

The workflow prints `launch_probe_active=true` only after those executable/JSONL checks pass. A binary-string marker scan remains supplemental and cannot substitute for executing the package.

The uploaded artifact is named with the exact PR head SHA and contains `BootOptimPandora-v0.exe`, `bootoptim-launch-interposer.exe`, `bootoptim-usn-helper.exe`, the executed packaged-probe JSONL, `COMMIT.txt`, and `SHA256SUMS.txt`.

## Automated semantic gates

The normal build matrix remains Linux x86-64, Windows x86-64 and macOS arm64. It runs final launch-probe tests, direct-Java filtering, `AssetVerificationMode` propagation, PR #11's USN runtime/fail-closed tests and fixed-size protocol tests before the release build. Windows additionally builds/pins the helper and the v0 delivery workflow executes the packaged EXE/interposer gate above.

These gates are correctness evidence, not physical performance evidence. Interactive UAC consent itself is not exercised by hosted CI; refusal/absence/failure stays fail-closed and belongs in the physical protocol as well.

## Physical acceptance boundary

The prior valid laptop capture established the measurement contract and reported Start→Java 317.864 s with assets 232.559 s and libraries 71.683 s overlapping. Those child phases must never be summed.

This branch makes no saving or TTMM claim. A performance decision still requires a warm physical A/B on the target laptop using this exact green artifact, the valid root JSONL and the assets sidecar/clean warm-state evidence. The baseline and candidate runs must use comparable instance/Java/network/filesystem-cache state, and any download/repair/contaminated run is not a clean warm USN comparison.

For the opt-in candidate, verify the packaged executable/helper hashes first. The first eligible opt-in run must establish a complete baseline through stock SHA-1; only a subsequent unchanged `Normal` GUI run may exercise `VerifiedReuse`. `FullVerification`, no opt-in, helper missing/hash mismatch/UAC cancel, corruption, mutation, delete/recreate, journal restamp/discontinuity and other uncertainty fixtures must continue to stock verification/repair.
