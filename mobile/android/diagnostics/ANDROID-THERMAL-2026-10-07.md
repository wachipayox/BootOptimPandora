# Android thermal front — 0.1.8 candidate

## Decision and origin

User priority: temperature first, Java RAM attribution later. BootOptim is frozen
pending the user's other agent; this change modifies only the Android engine.
Start from delivered engine feeac2bc (0.1.7), upstream 5e76d748. Keep the candidate
on codex/android-thermal-diagnostics-20261007, stacked on diagnostic PR83. Do not
promote thermal effectiveness based on a successful APK build.

The latest physical run is documented in VIVO-JFR-THERMAL-2026-10-07.md: Vivo
V2041, Android13, MobileGlues2.0.0, Xmx3072m/Xms512m; user-owned FancyMenu now loads
only the selected background. The foreground process was stopped by com.vivo.pem
at 19:15:45.300Z during world preparation. User observed an overheating warning
while charging. Android's USER_REQUESTED record identifies the stopping package,
not a temperature threshold. 0.1.7 had no thermal or charging sensor data.
The missing-MCEF background is not treated as the termination cause.

## Ownership

Android thermal APIs, battery/charging, display votes and sustained-performance
window requests belong in the launcher. There is no universal ordinary-app API
that returns all CPU/GPU die temperatures, locks safe frequencies or attributes
GPU energy to individual mods. Do not disable OEM thermal protection.

If measurements justify adaptive in-game FPS or worker budgeting, implement a
thin Android game bridge/mod with correct game/render-thread ownership and
hysteresis. Changing CPU counts blindly may increase total loading duration.
Cross-platform reductions in mod/NeoForge allocation or redundant computation
remain potential BootOptim/user-owned-mod work, explicitly deferred here.

## Reversible platform candidate

The engine's JVMActivity previously voted for at least120Hz unconditionally
on API31+ (`maxRefreshRate = 120f`). New **Modo térmico** defaults on:

- For game windows, request sustained performance only when PowerManager reports
  support. Apply on the Activity UI thread before JVM startup; release on destroy.
- On API31+, replace the maximum-refresh vote with60Hz, seamless changes only.
  Record the requested vote and sample the actual display refresh separately.
- A checkbox in Diagnóstico switches both requests for the next game Activity.
  Off restores the previous engine policy. JarExecutor windows are unaffected.
- Preserve renderer, resolution, texture quality, game options, Xmx, Xms, CPU
  count and pack contents. A display vote is NOT an in-game FPS limiter. The
  sustained-mode request can reduce peak performance and may do nothing on an
  unsupported device. It is not proof that overheating is resolved.

This is an OS policy candidate, not a new mod or proven CPU-saving algorithm.

## Bounded telemetry

When basic diagnostics are enabled, start one background-priority scheduled
worker independent of the existing expensive memory samples. Fixed delay10s,
maximum3hours, <=1MiB `memory-<origin>-<pid>-thermal.jsonl`. Thermal-headroom polls
are never more frequent than10s. Thermal status callbacks use the same executor;
unregister and shut down on game exit. Retain/share the last two session prefixes
through the existing local diagnostic ZIP. No new permissions, uploads, root,
full GC or stack walks. Optional deep JFR remains off unless explicitly armed.

Each sample records epoch/monotonic/session time, collection duration, process
CPU time delta and matching wall interval, available Android CPU count, thermal
status/name (API29), thermal headroom (API30; unavailable/NaN recorded, never
invented zero), battery temperature/raw tenths-C, charging/plugged/health/voltage,
power-save state, sustained support/request accepted, display refresh and vote.
Read only maxFps/enableVsync/renderDistance/simulationDistance from options.txt at
launch, <=1MiB. No credentials or arbitrary configuration content.

Process CPU / elapsed wall is **core equivalents**, may exceed1, includes Java
and native CPU in the same process, and does not measure GPU work. No CPU ratio
is emitted for a sub-second initial interval. Battery temperature is NOT CPU/GPU
temperature. Display rate is NOT rendered FPS. Request accepted is NOT measured
frequency change. Missing/zero thermal status cannot rule out vendor throttling.
Thermal events are written/closed as they occur; abrupt termination preserves
prior completed samples but may precede the next10s sample. Diagnostic cost is
recorded; do not claim zero observer effect or add inclusive operation times.

Recovery also offers the diagnostic dialog for a matching com.vivo.pem stop;
it describes the power-manager termination without labeling it a proven thermal
kill. Other USER_REQUESTED exits keep their prior handling.

## Physical gate

Install in place, same pack/FancyMenu/renderer/3072MiB and ordinary device setup.
Leave **Registrar memoria, temperatura y CPU** and **Modo térmico** on; leave deep
Java attribution off. Start cool, attempt the same world setup, share ZIP before
another launch. Do not intentionally reproduce a dangerous-temperature warning.
Record whether charging; interpret the captured charging state and heat slope.

Check support, real refresh, headroom validity, battery trend, CPU intervals,
sample cost, menu/world outcome and matched exit history. A completed run is a
functionality gate, not controlled evidence of thermal savings. A comparable
mode-off run, if safe/necessary, is the later policy-effect gate. Charging,
ambient temperature, starting heat, system activity and run stages must match
before claiming improvement. No physical thermal result exists for0.1.8 yet.

## Primary references

- https://developer.android.com/games/optimize/adpf/thermal
- https://source.android.com/docs/core/power/performance
- https://developer.android.com/reference/android/os/PowerManager#isSustainedPerformanceModeSupported()
- https://developer.android.com/reference/android/view/Window#setSustainedPerformanceMode(boolean)
- https://developer.android.com/reference/android/view/Surface#setFrameRate(float,int,int)
- https://developer.android.com/reference/android/os/Process#getElapsedCpuTime()
- https://developer.android.com/reference/android/os/BatteryManager

Disposition: separate Android diagnostic/policy candidate; no BootOptim/mod,
server or pack changes; compile/static gates only, thermal effectiveness pending.

Physical follow-up: see VIVO-WORLDEDIT-THERMAL-2026-10-07.md. Temperature/CPU capture
works; sustained policy unsupported on this Vivo; world entry failed at WorldEdit
Java heap exhaustion. The later exit is SIGNALED35, cause unresolved. This is not
a successful world gate or demonstrated heat reduction. Keep this PR diagnostic.
