# Vivo 0.1.8 world entry / poor FPS result — diagnostic7

Session memory-1791406062762-11245, PID11245, start2026-10-07T20:47:42.758Z.
Launcher0.1.8/code9, Android13/VivoV2041, MobileGlues2.0.0, Mali-G57MC2,
3072MiB heap,512MiB minimum,8reported processors, BootOptim058ac544.
Basic recording on, deep attribution false. Previous session13798 remains in
the ZIP and must not be mistaken for the new run. User reached the world after
long loading, reported ~1FPS, then left; another trial with visual mods disabled
was already underway when this ZIP was supplied. Do not apply that second
trial's changes retroactively to this diagnostic.

## Pack isolation and exit

Compared with diagnostic6, the discovered mod filenames removed are WorldEdit
7.3.8 and SableEdit0.0.4; no new discovered filenames. The log explicitly reports
worldedit->MISSING. This compares discovery names, not JAR byte identities or
every effective config/resource/shader setting. No source/pack was changed by
Codex during this analysis. No Java OOM is present in the new game log.

Player joined at21:00:18Z. Server shutdown starts21:13:56Z; all dimensions are
reported saved by21:14:16Z; Minecraft Stopping at21:14:24Z. Android records a
session-matched EXIT_SELF/status0 at21:14:30.007Z. JVM-return callback is false,
but self-exit0 plus the normal game/save sequence supports the user's normal
exit report; false alone is not evidence of a crash. No new thermal/LMK stop.
World entry and normal-save evidence pass; playability (~1FPS) does not.

## Loading phases (UTC; game log clock UTC+2)

- ModernFix's own menu timer ends20:54:56Z with432.863s (7m13s). Preserve its
  measurement origin; do not present as directly comparable launcher-to-menu.
- Integrated server starts20:58:09Z. Spawn progress ends20:59:53Z with30260ms;
  this timer is spawn preparation only, not the entire join/load.
- Player joins21:00:18Z; JEI starts21:00:39Z on Render thread and finishes
 21:05:13Z. JEI reports4.569min total (~274s).
- JEI ingredient registration22.98s; categories24.34s; recipes1.085min;
  ingredient filter2.466min (~148s), runtime2.629min. These nested/inclusive
  timings must NOT be summed with total JEI time. They identify expensive first
  join recipe/index work, not pure CPU or all post-menu loading.
- Server Can't keep up warnings during play; some large warnings follow explicit
  Saving and pausing game markers, so they do not prove uninterrupted tick stalls.

This removes the prior WorldEdit initialization OOM front but exposes other
substantial work. JEI initialization blocks Render thread during first join;
its indexed data may contribute memory pressure, but retained ownership has not
been measured. No instruction to remove JEI/addons or patch BootOptim yet.

## Thermal / CPU / memory observations

159thermal rows, last21:14:24.392Z;158JVM rows, last21:14:20.722Z. Start battery
36.9C, peak41.4C, final40.4C; AC plugged/charging throughout. Thermal headroom
0.842–1.0895,119/159samples>=1; status NONE throughout. Android documents this
inconsistent status and that headroom~1 is severe-throttling territory. This is
a signal of thermal limitation, not a measurement of exact clock loss/SoC
temperature or proof it alone explains1FPS.

Sustained mode unsupported, surface vote60Hz and actual display60Hz. Options at
launch vsync=true,maxFps120,renderDistance8,simulationDistance5; no rendered FPS
or GPU occupancy/frametimes collected. Display60Hz is not game60FPS.

Whole-process CPU equivalents: startup3.25,menu/config2.52,server/spawn/login3.22,
world initial2.82,world later incl. pauses2.46. These are weighted CPU-time/wall
intervals, not CPU frequency/power, a specific mod's CPU, or proof GPU idle.

World-initial JVM occupancy2727.8–3057.0MiB, later2868.4–3062.7MiB of3072MiB.
Initial-world GC counter +43.791s over430.354s between samples; later incl.pauses
+26.318s over353.041s. Collector counters can overlap; not exact pause percentage.
Pressure persists but is unlike diagnostic6's near-continuous GC/OOM sequence.

Process RSS peaks4389.5MiB, per-process VmSwap peaks2382.8MiB. These are resident
and swapped subsets, not to be added to logical heap/PSS. VmSwap can include
compressed swap; it does NOT prove slow disk activity. Minimum system available
356.6MiB; last available452.0MiB, Bionic native1102.7MiB. Post-exit available is
not the in-game headroom. This demonstrates tight system memory even without OOM.

## Graphics evidence and bounds

Veil4.1.4 reports incomplete framebuffer attachment for veil:light at20:54:39Z,
before menu. Kerria reports fast uploads/animated caches unsupported by current
GL capabilities. These are compatibility/performance fronts, not proof they
cause the measured/user-reported1FPS. The game recovers far enough to enter/save.
Iris explicitly reports shaders disabled at startup (enableShaders=false).
No GPU profiler, frame counter or render-time ownership is present.
Do not promise renderer changes or heat/FPS fixes from these logs alone.

Next: compare the user's current visual-mod trial with this same scene/settings
and recorded starting heat/charging. Capture its enabled-mod changes from next
log, loading phases, pressure/temperature and user FPS. If possible stay still
in an existing world for a short interval; creating a new world confounds FPS
with world generation. No prolonged overheating loop required.

## Disposition

First entered-and-saved world on this phone, performance gate still failed.
WorldEdit removal isolation supported; JEI first-join work, thermal limitation,
memory pressure and graphics compatibility remain. No proven exclusive FPS
cause, temperature optimization win, or end-to-end speed comparison. Keep PR84
as diagnostic/candidate; BootOptim remains frozen by user instruction.

Primary interpretation reference:
https://developer.android.com/games/optimize/adpf/thermal
