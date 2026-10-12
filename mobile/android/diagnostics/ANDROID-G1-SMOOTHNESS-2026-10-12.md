# Android G1 frame smoothness â€” 2026-10-12

Continuation of ANDROID-FRAME-CYCLES-2026-10-11.md. User prioritizes eliminating
concurrent-mark-aligned troughs, preserving quality/simulation. BootOptim untouched.
Raw evidence: C:/BootOptimBench/android-device-server-fps-20261011.

## Single concurrent worker, then return control

Same Vivo V2041/Mali/MobileGlues, support0.2.12, APK0.1.12,2816MiB heap,
thermal mode, original mods/graphics/distances and stationary official-server
player. Palladium fastBitSets=false already present; no new saving claimed.
Memory collectors off; existing bounded per-frame profiler on. Candidate changes
only -XX:ConcGCThreads=1. Candidate and return use gc* info logging; initial
control used simpler gc/safepoint info. No startup timing comparison.

| Completed-runTick metric | Original2 workers | Candidate1 worker | Return2 workers |
| --- | ---: | ---: | ---: |
| frames/s |10.4814|13.1090|8.4083|
| p50 ms |63.05|66.27|74.06|
| p95 ms |308.88|149.65|426.81|
| p99 ms |459.44|200.68|549.62|
| max ms |696.91|337.21|634.00|
| fraction frame wall in >250ms frames |28.06%|1.00%|40.87%|
| >250ms frames |90|4|118|
| 1s windows below5 frames/s,118 complete bins |34|0|48|
| minimum complete1s window |1|5|1|
| GC stop-the-world pause sum ms |2174.09|1721.19|2090.12|
| max GC pause ms |208.23|225.72|214.03|
| client ticks/s |19.884|20.000|19.783|

Origins/endpoints: initial session24324 capture_start1791754621011; candidate
memory-1791755256376-31444 capture_start1791755618613,120.452s counter interval;
return memory-1791755891535-32737 capture_start1791756277897,120.357s interval.
All use same90s first-world fallback/20s settle/120s capture. Frames.csv overflow0;
completed game-loop durations, not actual display/GPU completion. Counter boundary
may have a partial in-flight call. Quantiles are measured individual frame times.

Candidate mark cycles lengthen to~9.1â€“9.9s but mark-window mean frame drops to
89.95ms versus263.95/334.18ms controls. Outside-mark means61.16 versus66.88/77.20.
Only13 frames have0 ticks; normal simulation preserved. Candidate captures have
fewer deep stalls and controls both reproduce them; this supports reducing
concurrent interference, not hardware-limit attribution. Scene/weather/network/
thermal order are not identical; one A/B/A is not a universal device guarantee.
Still perceptible: candidate p99~201ms and minimum1s bin5FPS. Goal not complete.

Disposition: retain one-worker argument for this tested local instance, with
original arguments backup and removal as kill switch. No universal default,
RAM increase, resolution change, skipped ticks or new mod mixin. Repeat/endurance/
resource-reload validation on other hardware remains outstanding.

## Attribution run

Same one-worker candidate; frame profiler off, basic memory on. Bounded existing
JFR mechanism via StartFlightRecording: allocation20/s, rootless old-object
samples, GC summaries/phases,520s duration,16MiB rolling target. No forced GC,
heap dump, live histogram, path-to-root traversal or duplicated profiler.
Detailed gc+phases=debug isolates residual remark-pause work. This diagnostic
run is excluded from FPS comparisons. Latest saved recording was a later user-started attempt: session memory-1791757923228-15724, JFR starts2026-10-11T22:32:05Z,360s duration, normal close00:38:05CEST. It is not the initially armed00:10 attempt; preserve exact origin rather than merge attempts. World marker00:36:42CEST. Rootless220 unique old samples sum52.76MiB shallow only, not a complete retained graph. Post-world JFR reports five Class Unloading phases totalling1017.01ms,max262.58ms, versus Reference Processing0.71ms. This is the leading residual pause premise. Estimated allocation traffic includes EntityCulling BlockPos280.22MiB, Sable particle collision hash arrays207.44MiB and Create raycast BlockPos190.56MiB; not live bytes or ownership of the entire heap.

External taskset even via run-as fails Permission denied; no affinity changed.
Do not treat returned /proc thread IDs as verified namespace-correct attribution
without resolving the discrepancy between stat IDs and directory IDs.

## New visible base scene

User moved player from underground tunnel into a reinforced-glass safe enclosure
looking at Create/Aeronautics base. Previous scene numbers are historical only.
New first control: session memory-1791791841347-12543, capture origin
1791792203982,120.102s counter interval; same2816MiB, original mods/graphics,
explicit ConcGCThreads=2, memory collectors off, deep/JFR off. Original user
settings retained; only owned temporary attribution args removed. Normal connect,
visible scene screenshot and normal EGL shutdown passed. No player/world changes.

111 completed frames,0.9242FPS,p50 1029.02ms,p95 1627.10,p99 2078.91,
max2399.66. Every frame contains10 ticks; ticks9.342/s,76.208s inclusive;
world render16.467s inclusive. Thus simulation is saturated in this scene;
this is not evidence of a pure GPU limit. Outside mark mean frame904.23ms,
57.21ms/tick; inside1275.80ms,81.08ms/tick. Concurrent-mark trough still present,
ratio1.411. GC pause sum3243.92ms,max623.19. Six mark cycles~8.9–11.5s.

External2s global vmstat sidecar, same for next candidate; PSI unavailable to
shell. In capture26 outside/28 inside-mark intervals: pswpin428.55 vs3647.16
pages/s,pswpout811.06 vs3173.07,majorfault1423.93 vs4519.54. Global counters
cannot assign all traffic to game or convert it into unique retained memory.
MemAvailable738272/890706/1053712KiB min/median/max. This strengthens paging
correlation while system headroom exceeds many older runs; raw pressure unchanged.

Next cumulative profile candidate: ConcGCThreads=1 plus
-ClassUnloadingWithConcurrentMark; both exact arguments recorded. One-worker
benefit was isolated in old scene, class phase attributed separately; the new
combined trial cannot assign its FPS delta independently to each option.
Keep ClassUnloading enabled globally: G1 full GC still can unload classes/code.
Reclamation frequency/retained metaspace and code-cache pressure require continued
monitoring; short test does not prove long-session/reload safety. No blind IHOP
increase, GC disabling, heap increase, simulation skipping or GL changes.

Primary source for remark conditional:
https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/gc/g1/g1ConcurrentMark.cpp
Full-GC ClassUnloading conditional:
https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/gc/g1/g1FullCollector.cpp

User subsequently requested leaving Spark parked; no new Spark profiler launched.
Priority remains temporal spikes. Existing sparse stack samples are source hints,
not a replacement for fine CPU attribution when that front resumes.

## Visible-scene trials, completed loop durations

All captures use the same 90s first-world fallback, 20s settling and 120s window,
with original graphics/simulation and no JFR/deep or basic memory collector.
These are individual completed runTick calls, not displayed GPU frames.

| Trial | frames/s | p50/p95/p99 ms | max ms | wall fraction in frames >250ms | max GC pause ms |
| --- | ---: | --- | ---: | ---: | ---: |
| Original2, 2816MiB |0.9242|1029.02/1627.10/2078.91|2399.66|100%|623.19|
| One worker + no concurrent class unloading,2816MiB |5.7310|152.18/311.46/471.87|589.80|21.69%|69.52|
| Same profile,3072MiB |6.7323|135.48/258.71/397.78|542.57|13.37%|128.69|

Combined-profile session memory-1791792811955-13827, origin1791793185547,
120.397s. Client ticks19.967/s. Mark-window mean215.63ms versus144.15ms outside;
residual troughs remain perceptible. Global MemAvailable755952/785204/826924KiB
min/median/max; global paging traffic much lower than original2. Two changed GC
options prevent assigning this delta separately to either option.

Heap trial session memory-1791793988164-21680, origin1791794351579,120.464s.
Ticks19.989/s. Mark-window199.77ms versus129.28ms outside. MemAvailable fell to
364088/418398/474580KiB. REJECT the extra heap despite modest frame benefit:
it leaves too little Android/native margin and does not remove the trough.
Phone restored to2816MiB. SwapTotal was not captured, so absolute SwapFree
differences cannot establish additional unique game memory or comparable pressure.
Weather/network/thermal/order are not fully controlled; no universal FPS claim.

## Opt-in launcher profile and bounded affinity experiment

APK0.1.13 provides an independent opt-in GC profile, disabled by default on other
installations: ConcGCThreads=1 and -ClassUnloadingWithConcurrentMark for Java17+.
It respects explicit collector/worker/class-unloading arguments. ClassUnloading
itself remains enabled; full GC may reclaim classes. No heap/graphics changes.
Short physical testing does not establish long-session or reload safety.

Separate experimental switch places only G1 Conc# workers on lower-capacity cores
after a world-ready marker. Detects cpu_capacity, intersects original affinity,
verifies syscall result, saves/restores mask with kernel thread-start identity.
No render or STW workers are changed. Unknown topology/marker/API fails open.
Native helper is optional and does not require root. Permission success unproven
until an actual pin is recorded; external taskset permission failure is not proof.

First attempt session memory-1791796317968-4006: world_ready then trim level10
caused withdrawal before any pin. This is SKIPPED, not a performance improvement.
Capture376 frames,3.12565/s; variable scene costs preclude causal comparison.
Its raw trace is visible-affinity-skipped-trace.txt. A later prototype uses fresh
ActivityManager.MemoryInfo after30s settling, requires768MiB and !lowMemory,
and always restores on critical trim/onLowMemory. A low trim hint reads actual
headroom instead of assuming it. No safety threshold is lowered for the test.

Guarded trial visible-affinity2 also SKIPPED: at world_ready+30s, measured
available_bytes631476224 (~602MiB), lowMemory=false. No native pin occurred.
Session memory-1791798194167-19423, capture_start1791798572532,120.054s,
756 completed frames,6.2972/s,p50 148.76,p95 247.41,p99 317.35,max459.33ms.
35 frames>250ms,8.86% frame wall; ticks19.991/s. GC max84.15ms,total1176.42ms;
mark-window frame187.07ms versus132.72ms outside. These validate the existing
GC argument path again, NOT affinity. Scene/weather/order still vary.

Disposition: REMOVED the affinity prototype from deliverable source, UI and APK;
no disabled failed feature retained. Raw rejected sources and traces preserved in
benchmark folder for reopening with safe real headroom and demonstrated benefit.
GC profile alone retained as opt-in. Engine commit6ea2720; rebuilt APK success
1m10s, same signing key, installed physically. Normal EGL shutdown passed before
install. Restore original2816MiB user cfg (no temporary Xlog/JFR args); frame,
deep and basic memory collectors off for ordinary gaming. GC profile enabled on
this phone, disabled by default elsewhere. BootOptim and support0.2.12 unchanged.
Goal NOT complete: residual correlated spikes and ~6FPS visible base remain.

## Next allocation premise (source audit, no patch yet)

Actual phone EntityCulling1.10.4; upstream tag1.10.4 SHA
d15eed84cc5eb74b037ea39c6fca40ee83efd3bd inspected. Provider.isOpaqueFullCube
allocates one immutable BlockPos per query. clientTick periodically snapshots
block entities across17x17 chunks into a new HashMap. isDynamicWhitelisted loops
HashSets through new iterators. These match existing JFR allocation stacks;
allocation estimates do not prove individual frame/GC savings. Must preserve
async snapshot ownership, dynamic whitelist changes and position escape contracts
before reuse; blindly sharing mutable maps/BlockPos is not equivalent.
Source: https://github.com/tr7zw/EntityCulling/tree/d15eed84cc5eb74b037ea39c6fca40ee83efd3bd
Spark remains parked by user instruction; no new profiler. No mod candidate promoted.

