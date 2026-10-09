# Build0.1.10-alpha / 2026-10-09

- Engine local commit8e81b1a55314c7d4576d2f51bcd6c7644ebbc4bf, parent8eaa8bb.
- Pinned upstream5e76d7485d6ca34fe2adf86b31156f2714f5ccd9.
- Branch codex/android-diagnostic-export-recovery-20261009, stacked on draft#85.
- Windows/JDK21.0.4/Gradle8.14.4/SDK35/NDK27.0.12077973.
- Initial Gradle daemon failed before compilation with UDP bind address in use.
  Build-only JVM preferIPv4Stack=true resolved the host bind error; no game JVM
  networking setting was changed.
- :FCL:assembleDebug -Darch=arm64 succeeded in3m7s,92 tasks,28 executed.
- No runtime tests run or physical Android device attached.
- APK appID net.wachiland.launcher, code11/name0.1.10-alpha, API26minimum,
  target34, arm64-v8a, debug variant with persistent certificate/testOnly=false.
- Size182605775 bytes, SHA256
  ad2f8ad66e3908735bfc157a0bce3ffc3ecb24374b8b4f897480261bcdf5f1d6.
- apksigner verified same certificate as previous delivered alphas:
  dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406.
- APK agent manifest points to PerformanceBootstrap; counters are only in the
  separate bootstrap-counter asset. Heavy PerformanceAgent activates from the
  ordinary Java8-compatible MemoryMain wrapper after basic telemetry.
- Complete corresponding engine/reproduction source and checksums accompany APK.
  Signing secrets/local build configuration excluded.

This is diagnostic hardening. The cause of diagnostic10's early SIGKILL remains
unproven. Phone startup, coverage and observer cost are pending. No FPS/temperature
optimization or performance win is claimed. See VIVO-DIAGNOSTIC-10-2026-10-09.md.
