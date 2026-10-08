# Build 0.1.9-alpha / 2026-10-08

- Engine local commit: `8eaa8bbebd92e4562612b1e1eefc9c8043ac62cf`.
- Pinned upstream: `5e76d7485d6ca34fe2adf86b31156f2714f5ccd9`.
- Parent thermal engine: `5fa87bda6fec1230f50777a64b263571c4ab5cca`.
- Reproduction branch: `codex/android-frame-diagnostics-20261008`, stacked on the
  thermal diagnostic branch (draft PR #84). Experimental diagnostic; not merged.
- Windows JDK 21.0.4, SDK 35, build tools 35.0.0, NDK 27.0.12077973,
  Gradle 8.14.4. `:FCL:assembleDebug -Darch=arm64` successful (47 s, 92 tasks,
  8 executed, 84 up-to-date on final incremental package). No runtime tests run.
- Java memory companion stays Java 8; separate optional agent/counters target
  Java 17 and are packaged as APK assets, not Android DEX classes.
- APK assets inspected: agent JAR excludes counters; bootstrap JAR contains
  counters only. Supplied Java 17/21/25 ARM64 runtime archives contain libinstrument.
- APK package `net.wachiland.launcher`, versionCode 10, versionName 0.1.9-alpha,
  minSdk 26, targetSdk 34, native ABI arm64-v8a, debug variant/testOnly=false.
- APK size 182885369 bytes.
- APK SHA-256: `2733b2177199ab252f8360daebd311aaab2b897fadc071a337da2a36d239716e`.
- Signature verified with apksigner; certificate SHA-256 matches previous alphas:
  `dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406`.
- Complete corresponding engine/reproduction source ZIP, license notices and
  checksums are delivered alongside the APK. Signing credentials are excluded.

Physical phone validation is pending: launch compatibility, hook coverage and
measurement overhead/usefulness. Building and inspecting the APK does not prove
FPS improvement. See [diagnostic design](ANDROID-FRAME-DIAGNOSTICS-2026-10-08.md).
