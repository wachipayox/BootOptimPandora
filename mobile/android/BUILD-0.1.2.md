# Android 0.1.2 crash fix — 2026-10-06

## Confirmed cause

The user supplied an Android crash report from Vivo V2041 / Android 13 (SDK 33),
version 0.1.1-alpha. The NPE originates at `WachilandDashboard.java:107`, in the
posted global-card completion callback reading `profile.description` without
checking whether it exists. The signed manifest schema explicitly makes this
field optional (`internal/revision/types.go`, `docs/PROFILE_PROTOCOL.md` in
BootOptimDistribution). Public JSON inspection confirmed absent descriptions
in the E2E root and child manifests; the E2E root also has no presentation object.
The background worker's catch cannot intercept an exception thrown later by
an Android UI callback.

## Change

`ProfilePresentation` handles absent/null/wrong-type optional strings and objects.
Title falls back to catalog title, profile ID and then generic text. Description
uses current presentation, optional signed text, then Minecraft version or a
generic label. Explicitly empty current descriptions do not restore stale text.
Resolved description is computed before posting to the UI; the callback only
sets a prepared string. The creation dialog reuses the displayed description.
This fixes the specific crash rather than catching arbitrary UI exceptions.

## Build evidence

- Engine commit: `933f7a3a11a071919d03f146ceaaf00b237ddaf0`.
- `:FCL:assembleDebug -Darch=arm64`: successful in 1m11s; 86 tasks,
  22 executed / 64 up to date. No tests added or run.
- APK: `Wachiland-Launcher-Android-0.1.2-alpha-arm64.apk`, 182,528,922 bytes.
- SHA-256: `f9ed66a766d19eb01b6325be5431b4b7e86e0066305b4398f9cabc263802cbb3`.
- Signature verifies; unchanged certificate SHA-256 fingerprint:
  `dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406`.
- Same package `net.wachiland.launcher`, version code 3, Android API 26+, ARM64.
- Public HTTPS origin remains port 443. Nginx/server changes are unnecessary.
- No phone is attached to the agent host. The fix is traced to the supplied stack,
  compiled and inspected; on-device confirmation is pending the user's upgrade.

[Previous build and public API deployment](BUILD-0.1.1.md).
