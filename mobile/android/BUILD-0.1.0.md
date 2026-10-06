# Android alpha build evidence — 2026-10-06

- Desktop base: `8c9d18ca9021103f4735b1a36f57d781091c7755`.
- Mobile upstream base: FCL `5e76d7485d6ca34fe2adf86b31156f2714f5ccd9`.
- Modified engine commit: `d05b048` on `codex/wachiland-android-prototype`.
- Reproduction bundle branch: `codex/android-prototype` in BootOptimPandora.
- Toolchain: Eclipse Temurin JDK 21.0.4, Gradle 8.14.4, Android platform 35,
  NDK 27.0.12077973, CMake 3.22.1. IPv4 JVM option was required by the Windows host
  to avoid a Gradle daemon UDP file-lock socket bind failure.
- Final command: `gradlew.bat :FCL:assembleDebug -Darch=arm64 --no-daemon --console=plain`.
- Result: **BUILD SUCCESSFUL**, final incremental build 1m25s, 86 tasks
  (22 executed / 64 up to date). No unit, instrumentation or game tests were run.
- APK: `Wachiland-Launcher-Android-0.1.0-alpha-arm64.apk`, 184,886,577 bytes.
- APK SHA-256:
  `5491a63e3b5467cb1fe00787e3269b5ec264bd7e4b37329665609202dbc4e9f6`.
- `apksigner verify --verbose --print-certs`: verifies, v2 signature, one signer,
  RSA 4096. Dedicated certificate, SHA-256 fingerprint
  `dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406`.
- `aapt dump badging`: package `net.wachiland.launcher`, version `0.1.0-alpha`,
  code 1, min SDK 26, target SDK 34, native ABI `arm64-v8a`.
- `aapt dump xmltree`: nine explicit activities use sensorLandscape (6), no
  `testOnly` flag; debug variant is debuggable. Microsoft external browser is a
  separate application and follows that browser's orientation behavior.
- No broad external-storage / all-files permissions; APK installer permissions
  were removed. No startup upstream APK updater or prelaunch promotional dialog.
- Real Distribution catalog and signed revision metadata were inspected over
  verified HTTPS. Wachiland Elite requests Minecraft 1.21.1 / NeoForge 21.1.248.
- Microsoft device-code authorization accepted the public desktop client ID
  with HTTP 200. No human account login / entitlement verification was performed.
- No Android phone was attached. Layout, device-specific graphics, touch controls,
  actual Microsoft account completion and full modpack launch are **unverified**.

This build proves packaging and static integration, not runtime compatibility of
every desktop mod with an Android JVM/GPU. Preserve the first phone logs when
evaluating the pack. See README for prototype boundaries and source reproduction.
