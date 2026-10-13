# Android early-window and startup investigation, 2026-10-13

User requests using each launch for startup diagnosis and then stationary server
frame captures. Existing resources/final loading bar is acceptable; first red
period is not. Keep retained MobileGlues renderer and thermal mode. Fan is a fixed
physical test condition; temperature sampling verifies comparability, not a new
primary optimization front. No BootOptim source modifications authorized here.

## Authority and prior work

Refreshed origin/agent/integration-current: 4813ab19aec2a508034cd7c5e36410ee3723411a.
Read project AGENTS/README, research/catalog indexes, boot-pipeline program and
Mixin pipeline. Actual current source retains EarlyStartupProbeService,
StartupDiagnostics and scan-cache/discovery mechanisms. Open #145, #275 and #276
bodies/results inspected. Their detailed diagnostics/notes are NOT integrated
merely because a PR documents success. #276 hosted futures all finished before
scan barrier; don't blindly move the barrier or infer Android distribution from
that hosted result. No generic side-load cache/class prewarm rewrite is proposed.

## First same-JVM run

APK0.1.13, support0.2.17 explicit live probe, original Sable, 2816MiB/G1,
MobileGlues, thermal mode, fixed fan, basic collector on (deep/JFR off).
Initial failed attempt: Create RegisterEvent unbound chocolate_bucket, then
Xercapaint layers missing after broken mod-state rollback. Exclude failure from
startup benchmark. Retry reached official server and correct dry glass view.

Existing ModernFix launch-age marker reconstructs process origin to ~11:37:59
CEST on October13 (second-resolution logs; +/-1s, NOT launcher click time).
Coarse observations:

| Boundary | Process age approx |
| --- | ---: |
| ModLauncher main | 5.09s |
| ModernFix Bootstrap reached | 58.09s |
| Resource reload begins | 134.09s |
| Drippy resource overlay registered | 144.09s |
| Quickplay progress transition | 202.09s |
| JEI startup completes | 254.09s |
| In-world ready reported | 304.46s |

No observed title-screen endpoint: direct quickplay bypasses it. Resource-ready
surrogate, JEI preparation and server/chunk/world work remain separate; don't call
304.46s time-to-menu or sum overlapping resource listeners. Main coarse early
gate is ~134s before resource reload. A phase gap does not identify exclusive CPU,
disk, transformation or a particular mod. Basic instrumentation overhead exists.
No startup performance win established.

## Red-window finding

Screenshot live-kit-retry-startup.png confirms uniform red. FCL actual
DefaultLauncher.java adds default `-Dfml.earlyprogresswindow=false` unconditionally,
independent of renderer. User-supplied Java argument overrides this default
(CommandBuilder.addDefault checks prefix). Current phone fml.toml has
earlyWindowControl=true and earlyWindowProvider="drippy_early_window".
Phone custom Drippy early module was pulled and bytecode checked: bootstrap
preserves existing provider, GL window requests3.2 compatibility, render thread
loads textures then draws/swaps until Minecraft handoff. Existing Drippy module
and regular Drippy3.1.2 are present; no missing MCEF error is attributed here.

This explains suppressed early progress provider; it does NOT yet prove the
whole red-surface behavior is caused solely by the argument or driver regression.
FCL's default predates this experiment. Test the existing module with explicit
earlyprogresswindow=true on a future launch, preserving renderer and config with
rollback. Inspect actual early log/screenshot and Minecraft handoff; never promote
to a global Android default just because a screenshot is improved.

The provider loop relies on glfwSwapInterval(1), with no explicit Java frame cap.
Whether that swap blocks on the Android bridge is a separate CPU/thermal gate;
do not assume desktop vsync semantics. If needed, fix the user-owned early module
in its own isolated source worktree with documented semantic/visual gates.

Next launch: reuse existing bounded JFR settings for early-only owner samples,
duration capped before world FPS windows; enable only existing lightweight
startup report. No duplicate full profiler or live heap walks. Keep diagnostic
startup timing separate from clean startup A/B claims.

Raw markers: C:/BootOptimBench/android-device-server-fps-20261011/live-startup-markers.json.
