# Build evidence, Android 0.1.11 alpha

Modified engine: 978ce61363d68c403556857d30192bda0b348c86 on
codex/android-profiler-bootstrap-fix-20261009, based on delivered 0.1.10
8e81b1a55314c7d4576d2f51bcd6c7644ebbc4bf. Complete patch against pinned
FCL upstream 5e76d7485d6ca34fe2adf86b31156f2714f5ccd9 passes reverse
applicability inspection in the modified source checkout.

JDK21, Android platform35, NDK27.0.12077973, arm64, assembleDebug:
BUILD SUCCESSFUL in 1m22s; 92 tasks, 25 executed, 67 up-to-date.
Initial host attempt failed Gradle's UDP lock-service bind before compilation.
Build-client/daemon preferIPv4Stack resolved that host issue. It is a build-only
setting, not a game networking/renderer/performance change. Local log:
C:/BootOptimBench/android-0111-build-ipv4.log.

Delivered APK: Wachiland-Launcher-Android-0.1.11-alpha-arm64.apk,
182607667 bytes.
SHA256: 7792cd3bd0e839b559f973920f6109e7c3837db625f75a49ae681c62f9212d09.
APK signature verified with the same delivery certificate SHA256:
dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406.
Manifest: net.wachiland.launcher, code12, 0.1.11-alpha, minAPI26,
target34, ARM64 only. Compatible application ID/certificate permits an in-place
update; retain installed instances and worlds.

Static bytecode/asset inspection: packaged probe/counter JARs exactly match the
compiled JARs; counters occur only in their separate bootstrap JAR, minimal
PerformanceBootstrap is still Premain-Class. PerformanceAgent.class contains
no appendToBootstrapClassLoaderSearch reference, resolves bootstrap counters
before transformations, records startup_bootclasspath readiness and checks shared
class identity. Source merges Cacio and probe paths into one bootclasspath option.
Source archive contains the full corresponding engine, reproduction patch and
licenses; signing credentials/local configuration/build outputs are excluded.

No runtime tests were run. Physical Android startup, hook coverage and profiler
observer cost remain pending. This eliminates the exact native call implicated
in the user's 0.1.10 SIGSEGV, not evidence of a general FPS/heat/memory fix.
See diagnostics/ANDROID-PROFILER-NATIVE-CRASH-2026-10-09.md.
