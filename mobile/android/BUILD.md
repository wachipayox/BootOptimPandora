# Android 0.1.1 build and public API evidence — 2026-10-06

- Mobile engine commit: `5baae2e5e4811be530c6d427ab56b4eac6588ce5`.
- Pinned upstream remains FCL `5e76d7485d6ca34fe2adf86b31156f2714f5ccd9`.
- Build: `:FCL:assembleDebug -Darch=arm64`, successful in 1m13s,
  86 tasks (21 executed, 65 up to date). No unit/instrumentation/game tests run.
- APK: `Wachiland-Launcher-Android-0.1.1-alpha-arm64.apk`, 182,528,562 bytes.
- SHA-256: `1962b6ea96649914492e845e4019bb7f6d9676169501a9c0ba63726886a45752`.
- APK signature verifies. Certificate fingerprint is unchanged from 0.1.0:
  `dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406`.
- Package `net.wachiland.launcher`, version code 2, min SDK 26, target SDK 34,
  ARM64. Existing installation can update in place without deleting app data.
- Default Distribution origin is now `https://welite.ddns.net` (public port 443).
  Non-JSON responses produce a clear API deployment error.
- Operator executed pinned installer `988baa0288670d6d051572b8a74d8b5cf520d4ea`.
  Nginx syntax passed before and after; reload succeeded. Original site backup:
  `/var/backups/wachiland-nginx/20261006T183838Z-178249/pterodactyl.conf`.
- Curl checks forced public IP `79.116.38.74`, bypassing the developer PC hosts
  entry. Verified HTTPS: catalog schema 1 / three profiles, two signing keys,
  Wachiland Elite immutable revision, and its 117,313-byte PNG with matching hash.
- Public `/admin/` returns 403; `/v1/admin/session` returns the existing website's
  HTML, not Distribution JSON. Only read API routes were proxied. Private 8444
  deployment and its administrator session/CIDR checks are unchanged.
- Actual Microsoft account completion and modpack startup on Android remain
  unverified. The first phone run of 0.1.0 established launcher startup/layout,
  but failed before catalog access because the initial URL was LAN-only.

See [initial build evidence](BUILD-0.1.0.md) for toolchain and prototype scope.
