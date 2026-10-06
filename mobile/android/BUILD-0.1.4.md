# Android 0.1.4 process termination diagnostics — 2026-10-07

## Current incident and evidence

User log: https://mclo.gs/zkoB1nk (raw retrieved through mclo.gs API).
Vivo V2041 / Android 13 / Mali-G57 MC2, MobileGlues 2.0.0, Java 21,
`-Xmx4096m -Xms4096m`. It reached the title screen and stopped shortly
afterwards without a JVM exit code, Java OOM, Minecraft crash report or native
signal in the game log. Veil shader/framebuffer failures and EMF repeated-model
warnings precede continued resource/menu activity; neither establishes causation.

The user cannot connect the phone to this PC and requested automatic collection.
This is a diagnostic build, not a confirmed fix for that unknown termination.

## Mechanism and bounds

- MainActivity asynchronously queries ActivityManager exit history once on
  creation, plus explicitly when the user opens Diagnóstico. No game-time polling.
- Android 11+ history includes up to eight recent process records: reason,
  status/signal, timestamps, process/PID, importance and sampled RSS/PSS. Physical
  memory at recovery is separately labelled, never presented as memory at death.
- Before native Minecraft launch, persist a PID/start/renderer session marker.
  A normal JVM exit callback records completion before stock process termination.
  Matching requires main process, matching PID and exit timestamp after start.
  Older pre-feature runs are explicitly uncorrelated.
- Collect at most two retained ANR/native traces per query (4 MiB each), keep
  at most eight files. Native Android 12+ tombstones are binary protobuf `.pb`;
  ANR traces are text. Missing/vendor-unavailable records remain inconclusive.
- Preserve the available game log locally before another game launch; cap its
  tail at 8 MiB and mask access tokens. Never rewrite the actual game log.
- Diagnóstico → Compartir diagnóstico exports a local ZIP via existing
  FileProvider and Android chooser, including text report, log and saved traces.
  No automatic uploads, signing changes, runtime/renderer/heap changes or
  permission prompts. Report failures cannot prevent launcher startup.
- Explicit LOW_MEMORY differs from SIGKILL: the latter is not itself proof of
  memory pressure (stock FCL also intentionally kills itself on JVM exit).

## Source / build evidence

Engine commit: `7c2199703239c324472b75b89f41a3231ab6bc3f`.
Pinned FCL base remains `5e76d7485d6ca34fe2adf86b31156f2714f5ccd9`.

- `:FCL:assembleDebug -Darch=arm64`: BUILD SUCCESSFUL in 1m56s;
  86 tasks, 24 executed / 62 up to date.
- APK: `Wachiland-Launcher-Android-0.1.4-alpha-arm64.apk`, 182,544,622 bytes.
- SHA-256: `d0da4e83637c4fcda7a7de25c6a77ec7c57807c3b0cbfc9b3e028d5dc67b4c83`.
- APK signature verifies; delivery certificate SHA-256 remains
  `dc1a10e59f3fd7d74a09cc8eeaaed41b080da89eaa31c86f4ab32c7937020406`.
- Manifest confirms `net.wachiland.launcher`, code 5, `0.1.4-alpha`,
  min API 26, target API 34, native ABI `arm64-v8a`.
- Existing instances/accounts/worlds are preserved by an in-place update.

No tests were added or run, and no physical phone was connected. Compilation
and artifact inspection do not validate vendor exit reporting or the share
chooser on Vivo. First export may contain the earlier incident if Android still
retains it; otherwise reproduce and reopen before the next launch, then export.

API design:
https://developer.android.com/reference/android/app/ApplicationExitInfo
https://developer.android.com/topic/performance/issues/lmk

[Previous build evidence](BUILD-0.1.3.md).

## Subsequent phone evidence

The user exported the diagnostic ZIP on Vivo / Android 13. Recovery and sharing
worked; the repeated game restart is explicitly attributed to Android
LOW_MEMORY, with a matching game session PID and timestamp. The app remains
subject to that memory problem; this build is not a memory optimization.
See [incident and next gate](diagnostics/LOW-MEMORY-2026-10-07.md).
