# Android 0.1.8 alpha — thermal recording and platform policy candidate

Built 2026-10-07 on Windows, JDK21.0.4, SDK35, NDK27.0.12077973,
CMake3.22.1, Gradle8.14.4, :FCL:assembleDebug -Darch=arm64 --no-daemon.
Final source build successful in49s,89tasks (8executed,81up-to-date), after
initial candidate build1m47s. Existing deprecation warnings only. No tests
added/run; no attached phone or physical game/thermal validation.

Engine upstream: 5e76d7485d6ca34fe2adf86b31156f2714f5ccd9.
Modified engine: 5fa87bda6fec1230f50777a64b263571c4ab5cca on
codex/android-thermal-diagnostics-20261007.
Base delivered0.1.7: feeac2bcbaeb55abae14936a238c394eb0219eb1.
Complete binary source patch passes reverse applicability inspection against
the modified checkout. Separate candidate stacked on diagnostic PR83.

Delivered Wachiland-Launcher-Android-0.1.8-alpha-arm64.apk:182859239bytes.
SHA256:cb1d6eace82196dd81a63e08fdd375517b702ae783e213b1898603eb111eb527.
APK signature verified; same delivery certificate SHA256:
dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406.
Manifest: net.wachiland.launcher, code9,0.1.8-alpha,minAPI26,target34,
ARM64only. Compatible signer/app ID allows installation in place; no claim of
runtime equivalence based on compilation or signature checks.

Includes independent10s thermal/charging/battery/process-CPU samples and thermal
status callbacks in the existing local ZIP. Modo térmico defaults on: request
sustained performance when supported;60Hz surface vote onAPI31+. Disable in
Diagnóstico to restore the previous window policy. No actual game FPS limiter,
resolution/renderer/pack/RAM/CPU-count change or vendor-protection override.
Requests do not prove heat reduction; battery temperature is not die temperature.

Leave deep Java attribution off for the next temperature trial. Its code/probe
is preserved from0.1.7. Full corresponding modified engine source/reproduction
ZIP includes licenses, excludes credentials/build outputs, and has checksums
adjacent to the APK. See diagnostics/ANDROID-THERMAL-2026-10-07.md for lifecycle,
metric boundaries, known evidence and the physical acceptance gate.

BootOptim, mods, server and installed pack were not modified. Pending: install
over previous APK, same MobileGlues/3072MiB/FancyMenu, attempt world setup, share
diagnostic ZIP before another game launch. No temperature savings claimed yet.