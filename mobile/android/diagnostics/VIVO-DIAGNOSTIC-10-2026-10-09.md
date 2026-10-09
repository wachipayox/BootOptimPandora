# Vivo diagnostic (10), 2026-10-09

## Evidence and session boundaries

User supplied diagnostic (10), APK 0.1.9-alpha/code10, Vivo V2041/Android13,
MobileGlues. They confirmed they had not run the new Spark comparison yet and
could not recover the first attempt's crash log. They will collect Spark next.
No Spark URL/profile is present in this ZIP. Private filenames/identity details
are intentionally omitted here. Local extraction: C:/BootOptimBench/android-diagnostic-10-20261009.

Two new attempts must be separated:

1. PID4254, telemetry origin 2026-10-09T19:35:37.891Z. One initial Android and
   thermal sample, launcher_prepared status at19:35:39.098Z, and
   jvm_probe_arguments_prepared event. Android reports SIGNALED/status9 at
   19:35:42.889Z, followed by crash-page process PID4858/status9 at19:36:06.383Z.
   No embedded JVM CSV/info, agent_ready, hooks or capture output is present.
   SIGKILL alone does not establish LMK or temperature. Crash-page existence is
   suggestive but does not identify the exception. The first game's log was lost
   when the later launch overwrote latest_game.log. Initial thermal sample: battery
   35.4C, not plugged, OS status NONE, headroom~0.808; not a series or CPU/GPU temp.
2. PID5056, launch19:36:20.639Z, normal EXIT_SELF/status0 at19:52:50.171Z.
   Log reaches the world and saves/exits normally. Current preferences report
   basic=false and FPS=false, no telemetry prefix. Therefore this run intentionally
   had neither basic recording nor the new profiler requested; do not correlate
   its log with PID4254 or older thermal/memory files. No new mod crash is proven.

World-entry markers in local log: ModernFix reports startup271.079s, player join
   19:44:31Z, JEI completion19:45:29Z (46.52s reported). These are distinct origins
and endpoints, not additive sequential totals or an A/B performance win. Kerria
is absent from this log's discovery/listing; exact JAR/config equality with earlier
pack is not established. Repeated server can't-keep-up warnings do not measure FPS.

## Confirmed exporter bug

Launcher prepares performance-settings.json, but the export allowlist only
included csv/jsonl/txt/jfr. Thus absence of the launch-settings JSON cannot prove
snapshot failure. Fix only the bounded, allowlisted performance-settings JSON;
do not export arbitrary configuration JSON. This is separate from missing JVM
initialization/capture for the early attempt.

## Follow-up diagnostic hardening, 0.1.10

User explicitly asked to continue useful work while collecting Spark. Isolated
branch codex/android-diagnostic-export-recovery-20261009 starts from0.1.9 engine
8eaa8bbebd92e4562612b1e1eefc9c8043ac62cf and reproduction334eaf10e.

- Minimal premain retains only Instrumentation and writes bootstrap_ready; no
  profiler/ASM dependency references. Heavy agent resolution and activation move
  into ordinary MemoryMain after basic telemetry starts and before mio.Wrapper.
  Loading/initialization errors there are caught, bounded stacks recorded and
  normal game launch continues. This hardens a potential startup failure path;
  it does NOT establish that the earlier SIGKILL was caused by that path, nor
  protect against arbitrary native runtime failure or OS kills.
- Preserve launcher/bootstrap/profiler stage markers rather than overwriting them.
- Include launch-settings JSON in the ZIP.
- Before the next bridge writes/redirects the game log, save the prior attempt's
  masked tail (up to8MiB), retain three separate attempt logs, declare timestamp/PID
  and whether the telemetry-origin property matches. Basic-disabled runs can also
  have archived logs; missing exact origin is explicitly reported.
- Crash-page process saves its existing bounded report for future diagnostic ZIPs;
  no added work in the failing uncaught handler. Retain three reports.

BootOptim/modpack/server are unchanged. No physical startup, profiler coverage or
FPS claim is implied. Finish the existing Spark trial on0.1.9 with FPS capture
off and basic collection on; use0.1.10 for a separate agent trial afterward.
