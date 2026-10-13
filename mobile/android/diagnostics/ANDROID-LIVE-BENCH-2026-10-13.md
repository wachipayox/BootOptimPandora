# In-session Android culling comparison, 2026-10-13

User restored the safe glass scene and stopped rain; previous Sable candidate
was confounded by rainfall/camera changes. Do not promote that candidate.
Restore exact original phone Sable (2892ae4658e892ad80e0ecbe28772b381a2e7426e73e14ba22e5e64e0874a960).
BootOptim and the user's source main branches remain untouched.

Support 0.2.17 is an isolated diagnostic build, not a production optimization.
It consumes the EXISTING bootstrap PerformanceCounters/ASM agent, with explicit
manual property suppressing the previous automatic worker. No new transforms,
dynamic bootstrap append, heap walk, Spark, thread stack sampler or GC request.
The same APK 0.1.13, MobileGlues, 2816 MiB, optional retained G1 profile and
retained support flags are kept. No graphics/simulation/cap/player changes.

External one-shot request in the instance game directory:
`wachiland-live-bench-request.txt`: `label seconds frustum culling`.
Seconds bounded 30..120; booleans select the retained frustum shortcut and the
existing EntityCulling global enabled flag. Reflection fails closed if unavailable.
Only use with audited phone EntityCulling 1.10.4; no hard dependency is bundled.
When disabled, its official tick/ray paths stop and culled flags return visible;
this may increase drawing. This control tests net workload, not a shipping choice.

Queue waits until unobstructed world, then five seconds quiet and ten seconds
after toggles. Start/stop are render-thread client tick boundaries. Frame method
that straddles capture start is excluded using nano origin, and balancing nested
depth happens before rejection. Cumulative counters are NOT reset while worker
calls may be in flight. Frame trace arrays are reset only while inactive. Captured
arrays are copied after stopping, then bounded CSV exports run outside timed work.
Capture capacity 16384; overflow recorded and invalidates completeness claims.

Metadata records epoch/nano origin, elapsed interval, pose/yaw/pitch/rain and
flags. Any world/UI/position (>5cm)/camera (>0.2deg)/rain (>0.02) change marks
phase invalid. These guards do not detect all external base/world changes.
Original flags restore after each phase and interruption. No retained level/player
reference after capture; only bounded primitive arrays are exported.

Planned same-JVM sequence: retained baseline, frustum off (original ray path),
EntityCulling entirely off, retained return. Compare individual frame tails,
one-second cadence and GC marks, plus outside Android global pressure/temperature
sampling. Completed Minecraft runTick loops are not GPU presentation FPS. Inclusive
metric times overlap. Same process limits restart/page-cache confounding but thermal,
server activity and culling-worker state transition remain gates.

Build: mod Gradle assemble passed. Engine Gradle failed before task execution
(Windows lock-handler BindException); exact existing Java sources compiled with
JDK21 source/target17 and the same ASM exports, packaged as separate manual probe
and bootstrap counter JARs. Initial adb stdin copy was truncated; rejected by
byte comparison before launch. adb push + run-as cp succeeded with exact byte checks.
Native bootstrap ABI/path is unchanged. First startup failed in Create RegisterEvent
with unbound create:chocolate_bucket; later missing Xercapaint model layers were
consequences of the broken mod state. No live bench capture had begun. Archived
live-kit-startup-crash.txt/log; exclude this attempt from performance measurements.
The launcher only installs MemoryMain when basic/performance collection is armed.
With both false, premain bootstrap loaded but PerformanceAgent did not initialize.
For retry enable EXISTING basic memory collector (10s JVM/Android, 30s meminfo) and
keep deep/performance-next false; all compared phases have identical collector
cost. Preserve original three Cacio bootstrap entries alongside counters in the
final bootclasspath argument. Physical startup and measured phases pending.

Raw files/scripts: C:/BootOptimBench/android-device-server-fps-20261011,
prepare-live-bench.py, live-bench-phase.py. Restore retained support0.2.15 and
visible-clean.cfg after diagnostic; optional nonshipping instrumentation stays isolated.

## First physical same-JVM results

All six captures valid: same exact position (2837.5755,67,-536.5748), yaw9.6436,
pitch-1.5691, rain0; UI closed, overflow0, ~19.94..19.99 client ticks/s.
User confirms fixed fan aimed at phone, retained thermal mode. Battery temperature
after phases decreased33.7→32.4→31.0→30.7C, so first phase is not a thermally
matched benefit control. Same JVM also keeps warming JIT/caches; neither cause
is individually attributed from the observed drift.

| Phase | seconds | loops/s | p95ms | p99ms | >250ms frame wall |
| --- | ---: | ---: | ---: | ---: | ---: |
| retained baseline |120.003|5.9165|325.724|463.047|22.425%|
| frustum off |90.021|7.6427|208.236|260.455|3.167%|
| EntityCulling off |90.062|8.0722|194.551|257.457|3.494%|
| retained return |120.044|9.3466|154.462|226.436|2.493%|
| EntityCulling off repeat |90.000|9.6555|150.097|188.249|0.406%|
| retained final |90.035|10.2627|138.801|180.336|1.332%|

Decision: no net FPS win proven by turning EntityCulling off. Immediate bracketing
retained9.3466/10.2627 versus off9.6555 is within drift. First off vs early5.9165
is misleading because later retained is faster than both. Do not remove the mod
or ship its disable as an optimization. Retained frustum's first off comparison
also cannot prove new benefit/harm under this drift; existing provisional decision
is unchanged. No Sable direct query variant present during this sequence.

GC tail: retained final mean102.339ms in mark vs93.188 outside, max pause265.59ms;
off repeat106.612 vs100.703,maxpause282.34. Initial baseline206.683 vs136.174,
maxpause99.53. Concurrent-phase interference shrinks with session age despite
larger isolated mixed-evacuation pauses. These are observations, not proof of a
JIT/thermal/paging root cause or of hardware limit. Spikes remain perceptible;
30FPS objective unmet. No user graphics or simulation state was changed.

Normal world disconnect/main-menu quit/EGL termination passed. All diagnostic
toggle flags restored to retained baseline. Next startup test isolates explicit
early window argument; first180s existing JFR ends before any world FPS window.
