# Android 0.1.7 alpha — Java object attribution diagnostic

Built 2026-10-07 on Windows, JDK 21.0.4, SDK 35, NDK 27.0.12077973,
CMake 3.22.1, Gradle 8.14.4, `:FCL:assembleDebug -Darch=arm64 --no-daemon`.
Compilation/package successful in 1m36s, 89 tasks (26 executed, 63 up-to-date).
Only existing deprecated API/Gradle warnings; no tests or physical game run.

Engine upstream: `5e76d7485d6ca34fe2adf86b31156f2714f5ccd9`.
Modified engine: `feeac2bcbaeb55abae14936a238c394eb0219eb1`
on `codex/android-java-object-attribution-20261007`.
Base delivered 0.1.6: `6b9df7460a58b59e3472af9d6d9a3e88b62e4918`.
Whole binary patch applies against that upstream and passes reverse applicability
inspection against the modified checkout. This is a separate diagnostic stacked
on draft #82, not a validated production optimization.

Delivered APK `Wachiland-Launcher-Android-0.1.7-alpha-arm64.apk`: 182572018 bytes.
SHA256: `b236d07d2fa45dd021fd7ec25a510a5c71302006c75cce29b6d18e46e7792dda`.
Signature verified: same delivery certificate SHA256
`dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406`.
Manifest: `net.wachiland.launcher`, versionCode 8, 0.1.7-alpha, min API 26,
target 34, ARM64 only; no testOnly flag. These do not prove runtime equivalence.

Packaged memory probe remains Java8 classfile 52 inside an asset JAR, independent
of Android DEX. Bundled JRE21 module image includes JFR/DiagnosticCommand classes
and profile settings; native support on the phone is still unproven. Offline
JFR-report source compiles with JDK21. No synthetic/runtime tests were performed.

Deep attribution is off by default, visibly armed for one launch. It samples
creation stacks for up to 20 minutes and preserves complete 30-second JFR snapshots.
On a verified current-session ModernFix menu marker with sufficient recent
Android headroom it requests occupied/live histograms. Live census requests one
full GC and may pause; all operation durations/skips are recorded. Missing API,
marker or headroom leaves basic counters running. No root traversal, heap dump,
cache deletion, mod, renderer or quality change. See
`JAVA-OBJECT-ATTRIBUTION-2026-10-07.md` for limits and physical gate.

Complete modified-engine source and reproduction bundle are distributed in the
matching 0.1.7 source ZIP, with licenses and no private signing/local config files.
APK/source checksums are included in the adjacent SHA256 file.

Pending: install over previous APK, enable deep diagnostic, reach menu and wait
>=30 seconds, attempt world creation and share ZIP before another launch. No
measured Java savings or per-mod retained ownership claimed before that evidence.
