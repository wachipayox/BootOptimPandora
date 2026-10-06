# Android 0.1.6 heap headroom candidate — 2026-10-07

## Scope

User requested the simplest credible solution after the physical phone repeatedly
exhausted available RAM. Default Java minimum is now 512 MiB instead of being
forced equal to the selected maximum. Explicit minimum/JVM overrides remain
supported. Maximum policy, mods, renderer and visual quality are unchanged.
Diagnostics are lighter, with optional G1 old/young occupancy counters.

See [evidence, implementation, alternatives and phone gate](diagnostics/ANDROID-HEAP-HEADROOM-2026-10-07.md).
This candidate is not yet a proven memory fix or a claim that the pack fits.

## Build / artifact inspection

Engine commit: `6b9df7460a58b59e3472af9d6d9a3e88b62e4918`.
Pinned upstream FCL base: `5e76d7485d6ca34fe2adf86b31156f2714f5ccd9`.

- `:FCL:assembleDebug -Darch=arm64`: BUILD SUCCESSFUL in 1m31s,
  89 tasks, 25 executed / 64 up to date.
- APK `Wachiland-Launcher-Android-0.1.6-alpha-arm64.apk`, 182,563,118 bytes.
- SHA-256 `8fb2e332b72ae0d5766f388b2144081ecb0dd8c2362a2cb8f658919a7846664c`.
- Signature verifies; same delivery certificate SHA-256
  `dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406`.
- Manifest: `net.wachiland.launcher`, code 7, version `0.1.6-alpha`,
  min API 26, target 34, ARM64 only. In-place upgrade preserves instances/accounts.
- Embedded-JVM probe javap inspection: classfile version 52 (Java 8), CSV schema 2.
  Regular OpenJDK bytecode, not Android DEX.
- Complete source patch reverse check succeeds against the committed engine.

No tests added/run and no Android device connected. Compilation and static
artifact inspection do not prove phone startup, lower RSS or world usability.
Desktop exact-pack startup CI cannot reproduce this Android runtime/driver gate.
Candidate remains separate from production integration pending physical evidence.

## Use / decision gate

Install over the existing APK. Keep MobileGlues and 3072 MiB maximum, the same
pack and enabled memory recording. Effective command should contain Xms512m and
Xmx3072m unless an explicit user override exists. Try menu plus representative
world if it reaches the menu. After failure/success, Diagnóstico → Compartir
diagnóstico before more launches overwrite the retained last two sessions.

Compare process-origin elapsed times and resource phases, not launcher setup or
nonmatching sample instants. Check occupied/committed heap, RSS, available system
RAM and Android exit reason. If still LOW_MEMORY, use the G1 occupied counters
and native/graphics trends to choose the next allocation owner to investigate;
raising Xmx is not the default response.

[Previous build evidence](BUILD-0.1.5.md).
