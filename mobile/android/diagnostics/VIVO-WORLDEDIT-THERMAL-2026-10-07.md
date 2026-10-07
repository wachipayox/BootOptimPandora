# Vivo 0.1.8 run: thermal telemetry, WorldEdit OOM and later signal exit

Input: user-shared diagnostic (6), Android0.1.8/code9. Current session prefix
memory-1791403396511-13798, PID13798, start2026-10-07T20:03:16.508Z.
Deep attribution false. Prior-session0.1.7 JFR is still in this two-session ZIP;
it is NOT evidence from this run. Basic counters and thermal sampler are valid.
User added BootOptim integration build058ac544; log confirms the artifact name.
This is not a controlled comparison with0.1.7: pack and instrumentation changed.
BootOptim sources are not modified; user requested waiting for another agent.

## Timeline (UTC; game log local clock is UTC+2)

- 20:10:40: ModernFix menu proxy reports443.335s startup. This is the mod's
  own origin/end marker, not a launcher-to-menu performance comparison.
- 20:15:11–20:15:36: spawn preparation completes, reported25319ms.
- 20:15:36 onward: ServerStarted handlers, then WorldEdit state-table building.
- 20:16:20–20:21:50:33WorldEdit warnings, individual population operations
  >5s (5.0–15.1s), including blocks with only8/16possible states. Their214274ms
  sum is incomplete inclusive operation time, NOT CPU or total critical path.
- 20:22:37: WorldEdit PlatformReadyEvent dispatch logs
  `OutOfMemoryError: Java heap space`, Guava ImmutableTable builder / ImmutableSet
  ->BlockState.populate->generateStateMap->Capability.ready.
- Later log also reports OOM in WorldEdit Session Manager's uncaught handler.
- 20:35:54.822: Android records matching foreground SIGNALED/status35, no JVM
  exit callback. User opened notification shade, observed complete freeze and
  launcher restart; no manual force-stop. No trace/tombstone was included.

## Confirmed heap/CPU pressure

109JVM samples; last20:22:07.424Z,30s before logged OOM, then JVM sampler stops.
Last occupied heap3067.95MiB of3072MiB; G1 old3067.93MiB. From20:15:40.429 to
20:22:07.424, reported GC elapsed counter grows370531ms over386995ms wall.
GC MXBean sums may include overlapping collector activity: do not present this
as exact CPU percentage or exclusive pause time. It strongly supports severe
GC pressure; the later exception independently proves Java heap exhaustion.

Thermal worker remains alive until20:35:48.014Z,192samples total. Process CPU
averages4.98core equivalents during the WorldEdit window and4.41 after logged
OOM (CPU delta / matching wall; all Java/native process threads). This attributes
whole-process CPU, NOT all CPU to WorldEdit/GC. No detailed CPU stacks captured.
WorldEdit is the failing allocation path; this does not establish that its
objects alone occupy the whole heap or that it is the only retaining owner.

173Android memory rows; last20:35:43.324Z. Last system available545.34MiB,
minimum306.57MiB; last RSS3943708KiB (~3851MiB), swap1676968KiB (~1638MiB),
Bionic native897574544bytes (~856MiB). These overlap other metrics; do not add
them to Java logical occupancy or post-exit available memory.

## Thermal policy capability and measurements

- Thermal mode requested; sustained_supported=false throughout. Therefore the
  sustained-performance API optimization cannot operate on this Vivo/release.
-60Hz surface vote applied; actual display reports60Hz. Launch options maxFps120,
  vsync=true, renderDistance8, simulationDistance5. No rendered FPS measured.
- Device charged on AC throughout; battery75->81%,38.0–39.8C, final39.8C.
  Battery temperature is not CPU/GPU die temperature or an OEM shutdown threshold.
- Thermal status remains NONE despite headroom0.8809–1.0206;10samples>=1.
  Android documents that this status can be stale/unsupported; headroom reaching
  ~1 indicates severe-throttling territory. Do not claim the phone stayed cool.
- Thermal collection median90.5ms, max1227ms; even lightweight platform calls
  slow under pressure. No zero-overhead claim. Sampling interval remains>=10s.

This exit has no matching LOW_MEMORY or com.vivo.pem thermal-stop record.
Android reserves signal35 for debuggerd. That number alone cannot identify the
sender, original fault or prove an ANR/thermal/driver failure. Notification shade
timing is user-reported correlation; causal surface/lifecycle failure unproven.

## Source evidence and next gate

EngineHub WorldEdit commit7d32b45 generates Cartesian state combinations and an
ImmutableTable of neighboring states per BlockState. Its startup capability
initialization calls getAllStates according to the failure stack. This is real
pack work after spawn100%, not proof of a pure world-generation deadlock.

User previously requested being told before changing/removing another mod.
Report WorldEdit as the specific failing mod path and offer a bounded isolation
run without WorldEdit, or separate compact/lazy table research if needed. Do not
silently remove it, change the global profile, patch BootOptim, raise heap or
claim a retained-table optimization without validating its contracts/dependencies.
Reducing this prolonged GC/allocation work is also a thermal front. Platform
window policy alone did not reach a usable world on this device.

Primary sources:

- https://raw.githubusercontent.com/EngineHub/WorldEdit/7d32b45/worldedit-core/src/main/java/com/sk89q/worldedit/world/block/BlockState.java
- https://developer.android.com/games/optimize/adpf/thermal
- https://android.googlesource.com/platform/bionic/+/master/libc/platform/bionic/reserved_signals.h

Disposition: thermal capture gate passed; sustained mode unsupported; usable-world
gate failed. Java OOM at WorldEdit initialization confirmed, final signal cause
unresolved. No source optimization, APK rebuild, server/pack or BootOptim changes.
