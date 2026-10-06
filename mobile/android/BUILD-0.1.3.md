# Android 0.1.3 prelaunch storage fix — 2026-10-06

## Confirmed cause

Three user screenshots show a prelaunch FileSystemException, not a Minecraft
mod crash. The stack is GameAssetDownloadTask.execute:112 →
CacheRepository.tryCacheFile:122 → FileUtils.copyFile:388 → Android
UnixCopyFile.copyFile:258. The existing asset in app-scoped shared storage
(/storage/emulated/0/Android/data/net.wachiland.launcher/files/instances/assets)
cannot be copied into the private SHA-1 cache under /data/user/0/.../cache by
the native Files.copy operation: "Operation not supported on transport endpoint".
The exact failing native syscall is not established by these screenshots.
The failure occurs before mod loading; it does not implicate Sable Android or
the global parent/child manifest.

## Changes and invariants

- Both core FileUtils.copyFile overloads share a staged copy implementation.
  Native copying remains the first attempt. IOException/unsupported operations
  retry using a bounded 64 KiB read/write buffer, including recreation of a
  staging file removed by failed native copying.
- Each staging file resides beside the destination. The destination is published
  by same-directory rename after successful copying and closing both streams.
  Atomic rename falls back to ordinary rename if unsupported; a failed copy
  cannot publish a partial cache entry or overwrite an existing destination.
- Previously truncated/empty cache entries are replaced when their size differs
  from the source. Existing checksum verification on cache reuse remains intact.
- Caching an already installed asset is optional. Cache write IOException is
  logged at FINE and cannot abort launch; missing game assets still require
  successful downloading and normal integrity checks.
- Android FCLCacheRepository.restore retains the verified original. It no longer
  deletes that original before attempting a hard link across shared/private
  storage, which cannot work on this storage layout.
- Package remains net.wachiland.launcher, version code 4, ARM64/API 26+,
  landscape. The delivery certificate is reused for an in-place update.

Engine commit: f704bbf658fcccd874dabe991401e0f66590e24b.
Upstream pin remains 5e76d7485d6ca34fe2adf86b31156f2714f5ccd9.

## Validation scope

- `:FCL:assembleDebug -Darch=arm64`: BUILD SUCCESSFUL in 1m33s,
  86 tasks (21 executed / 65 up to date).
- APK: `Wachiland-Launcher-Android-0.1.3-alpha-arm64.apk`, 182,529,686 bytes.
- SHA-256: `c3d0bd9eb1f8ce9580b5515e905bc92de3f8313372ba865beeb186022c90ca7c`.
- APK signature verifies. Certificate SHA-256 remains
  `dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406`.
- Manifest confirms `net.wachiland.launcher`, code 4, `0.1.3-alpha`,
  min API 26, target API 34, native ABI `arm64-v8a`.

No tests were added or run. No phone is attached. Device confirmation must come
from retrying the existing Android profile after installing the APK over 0.1.2.
This change does not claim that all the modpack's mods run on Android.

[Previous build evidence](BUILD-0.1.2.md).
