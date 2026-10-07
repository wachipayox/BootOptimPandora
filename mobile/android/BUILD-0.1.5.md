# Android 0.1.5 memory attribution diagnostics — 2026-10-07

## Scope

User requested diagnosis of memory outside Minecraft's Java heap after two
confirmed Android LOW_MEMORY kills. Added separate embedded OpenJDK, Android ART,
whole-process and system sampling to the existing launcher, not a separate app.
No RAM/renderer/modpack setting change or claimed memory fix.

See [mechanism, bounds and next phone gate](diagnostics/MEMORY-SAMPLING-2026-10-07.md)
and [confirmed 4/3 GiB incidents](diagnostics/LOW-MEMORY-2026-10-07.md).

## Build / artifact inspection

Engine commit: `411419dc4b33c17638a77eab2fcb66334aa2f3c4`.
Pinned upstream FCL base: `5e76d7485d6ca34fe2adf86b31156f2714f5ccd9`.

- `:FCL:assembleDebug -Darch=arm64`: BUILD SUCCESSFUL in 1m51s,
  89 tasks, 10 executed / 79 up to date.
- APK `Wachiland-Launcher-Android-0.1.5-alpha-arm64.apk`, 182,562,562 bytes.
- SHA-256 `7c20f0d93c65d1e7147f52f2222a947132f363b073b3e8ac943737916d1a0c14`.
- Signature verifies; same delivery certificate SHA-256
  `dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406`.
- Manifest: `net.wachiland.launcher`, code 6, version `0.1.5-alpha`,
  min API 26, target 34, ARM64 only. In-place upgrade preserves instances/accounts.
- Packaged `assets/game/wachiland-memory-probe.jar` present, 5,232 bytes.
  Main class inspected with javap: classfile version 52 (Java 8).
  Probe is regular OpenJDK bytecode, not Android DEX.

No tests added/run; no physical device attached. Runtime wrapper compatibility,
vendor memory categories and collection cost need the next phone export. A green
build does not establish attribution or that the modpack fits in physical RAM.

## Use

Install over the existing launcher. Memory recording is enabled by default; its
checkbox is in Diagnóstico. Keep current MobileGlues / 3072 MiB / pack unchanged
for one instrumented run. Reproduce, reopen launcher after termination, then
Diagnóstico → Compartir diagnóstico. Export before further launches; only two
memory sessions retained. ZIP includes paired JVM CSV and Android/process/system
JSONL records alongside existing exit report, saved game log and retained traces.

[Previous build evidence](BUILD-0.1.4.md).
