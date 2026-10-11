# Android server frame cycles, 2026-10-11

## Policy and scope

User explicitly requested removal, rather than an off-by-default switch, for the
staging-storage-reuse candidate. Its config, storage implementation, marker mode
and README entry are removed from current source; build version returns to0.2.12.
The rejected0.2.13 disabled JAR was deleted from the phone and local build output.
Historical research/commit remain as evidence; this overrides the earlier
experimental-off disposition in ANDROID-SERVER-FPS-2026-10-11.md.

BootOptim remains untouched. Work is in the standalone Android support/launcher
repositories. Original AsyncParticles/ParticleCore config and original enabled
mod set remain the measurement baseline. Direct play is retained.

## Existing captures: averages hide regular troughs

Reanalyzed cumulative500ms counters without another phone run. One-second and
five-second rates are interpolated from actual counter timestamps, not physical
display presentation timestamps. Partial boundary frames and interpolation
limit instantaneous precision. Histograms have upper bounds, not exact values.

Original return session memory-1791752058316-15685:

- Mean10.2747 completed runTick/s. Estimated one-second rates: min0.9515,
  median10.3410,90th percentile18.2922,max20.8949.
-15.6831% of completed frame durations exceed125ms. Median is in(33,66]ms,
 95th percentile in(250,500]ms,99th in(500,1000]ms.
- Rate autocorrelation peaks at11s,r0.9142; another original control peaks at
 10s,r0.7873. These include slow warm-up trends; not proof of a timer owner.
- After first30s, sparse selected Render samples in intervals below4FPS show
  mean CPU/wall fraction0.9434 versus0.9459 above16FPS. Inclusive client-tick
  wall fraction0.702 versus0.447; world-render fraction0.142 versus0.324.
  Slow intervals contain Create behaviour/chunk/entity/particle work, not one
  demonstrated single culprit. These metrics overlap and sparse sampling
  cannot establish exclusive per-mod CPU attribution.

CPU-parallel candidate memory-1791751430638-8391 retains a10s pattern,
autocorrelation0.9055; mean12.7455FPS, estimated one-second min2.0269/median13.5735,
95th frame-duration upper bound250ms rather than500ms. It improves an observed
distribution, not only the mean, but cross-run conditions and visual/stability
gates remain. It is not enabled as a production default.

GC totals in original10s windows are typically111â€“259ms, which do not directly
account for repeated multi-second troughs. Correlation with FPS is confounded
by trend/load; do not infer that GC is irrelevant or the unique cause. Android
full PSS samples take212â€“389ms and occur every~30s, light samples6â€“9ms every~10s.
The FPS cycles occur across both. Collector observer effects remain untested.

## Existing diagnostic extended, not replaced

Engine diagnostic branch codex/android-frame-cycles-20261011 extends the existing
PerformanceCounters/PerformanceAgent with a bounded16384-frame in-memory trace:
completed frame end/duration, ticks per frame, inclusive tick and world wall.
One Render writer; no file output per frame. Trace is written after capture;
overflow explicitly reported. Same90s first-world fallback,20s settle and120s
capture endpoint; monotonic timestamps anchored to a recorded epoch. This is
completed game-loop work, not GPU timing or display presentation.

APK0.1.12 also respects memory_enabled=false in a performance-only session:
no ART/PSS/mapping samples or embedded-JVM memory sampler, while preserving
performance initialization/log checkpoints and existing thermal recording.
Default memory-enabled behavior is retained. Deep attribution stays off.
Compile/package passed with existing signing key. APK 0.1.12/code13 installed; physical server capture completed without a new launcher/probe failure.

First run uses original mods/config, memory collectors off, GC/safepoint logging
and a bounded externalADB1s CPU-frequency sidecar. No renderer/quality/RAM change.
GC log uses its own uptime/wall markers; no boot/startup performance claim.
Frequency availability is device-specific; missing GPU frequency is not zero.
Next decision: determine whether repeated troughs persist without collectors,
then align actual frame ticks/GC and available frequency data. Do not alter
client simulation cadence or skip Create callbacks merely to raise FPS.

Raw analyses/plot/logs stay outsideGit at
C:/BootOptimBench/android-device-server-fps-20261011.

## Physical result: concurrent-mark-aligned sawtooth

Origin: PerformanceAgent capture_start epoch1791754621011, after the existing
90s first-world fallback plus20s settle; endpoint capture_complete1791754741647.
No launcher/startup performance comparison. Session memory-1791754253005-24324,
1263 completed runTick records, overflow0, same original mods/config/2816MiB,
MobileGlues/thermal mode/direct-server/stationary player. Memory collectors off;
GC/safepoint logging and external 1s CPU frequency sampling remain observers.

Rate10.4814 completed frames/s,2396 ticks/120.499s. Per-frame duration median
63.05ms,p90 202.91,p95 308.88,p99 459.44.90 frames exceed250ms; these execute
4–10 ticks and spend71.44% inclusive frame time in ticks. Fast frames below66ms
mostly execute one tick (648/692) and spend42.32% in ticks. Tick cost is itself
higher in slow frames (37.81 vs21.35ms per tick), so catch-up count is an
amplifier, not an established initiating cause.

Ten repeated troughs align with all ten G1 concurrent mark cycles in capture:
[3.45,8.73],[16.78,22.25],[30.12,35.11],[43.00,47.92],[55.28,60.36],
[67.61,72.54],[80.17,84.87],[92.98,97.74],[104.93,109.56],[114.75,119.41]s
relative to first captured frame. Completed-frame membership by end timestamp:
inside182 frames mean263.95ms,tick36.28ms/tick; outside1081 mean66.88ms,
tick22.80ms/tick. Boundary membership is approximate for straddling frames.
Stop-the-world GC pauses total2174.09ms over capture, much smaller than the
multi-second troughs. Concurrent mark lasts4.6–5.5s and repeats~10–13s.
This is strong timing correlation, not yet a controlled causal GC-policy A/B.

Heap after young/mixed collections stays~2434–2469MiB out of2816MiB; repeated
marking reclaims little. Thus retained Java data and GC pressure are now directly
relevant to FPS. Do not blindly raise heap on an already constrained Android.
Native thread inventory confirms G1Conc#0/#1 and8 GC worker threads. Launcher
passes ActiveProcessorCount=8; do not assume the device has8 equal-performance
cores. Average big-core frequency during mark~1.654GHz vsoutside~1.703GHz;
little~1.705GHz vsoutside~1.650GHz (43/67 samples). Available CPU frequency data
does not show a large whole-CPU drop as the sole explanation. Vendor/GPU
thermal throttling is not disproven; battery33.7C is not SoC temperature.

Next bounded gates: compare concurrent-worker policy at unchanged heap and
measure retained live set/allocation sources. A GC change can prolong marking
or increase pauses; reject it unless actual frame distribution improves and
memory/thermal stability holds. Avoid discarding client ticks or skipping mod
callbacks merely to improve a counter. Memory collectors remain off for this
performance A/B; re-enable only for attribution sessions.

Original Java arguments restored after normal EGLBridge termination; flags
restored (memory enabled, performance/deep arming off). No candidate GC policy
has been promoted. Engine commit837ab0a; storage trial removal a8a22c336.
Oracle G1 guide for concurrent thread semantics:
https://docs.oracle.com/en/java/javase/17/gctuning/garbage-first-g1-garbage-collector1.html
Raw frame/GC/frequency files and plotted overlays stay in the external raw folder.
