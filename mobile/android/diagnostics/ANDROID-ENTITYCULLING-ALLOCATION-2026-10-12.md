# EntityCulling allocation experiment, 2026-10-12

User priority: reduce GC-correlated frame troughs, then normal frame cost. Spark
parked. BootOptim untouched; separate Android Support candidate0.2.14.

## Premise and scope

Phone EntityCulling1.10.4 JAR pulled and javap inspected. Upstream matching tag:
https://github.com/tr7zw/EntityCulling/tree/d15eed84cc5eb74b037ea39c6fca40ee83efd3bd

Existing post-world JFR, excluded from FPS benchmarking, estimates147.77MiB
allocation weight in HashSet iterators from isDynamicWhitelisted and84.61MiB in
HashMap.resize from clientTick putAll. These are sampled traffic, not retained
memory, exact allocated bytes, critical-path CPU or measured FPS benefit.

Candidate experimental.entityCullingAllocation=true, false default, restart to
install hooks; early plugin limits to Android and exact EntityCulling1.10.4.

- Empty whitelist: return shared empty iterator only when the original Set is
  currently empty. Nonempty callbacks use stock iterator and stock order.
- Snapshot capacity: continue allocating a fresh HashMap for each capture.
  Predict capacity from preceding published map size, keep stock minimum16,
  bound prediction to65536 entries. Store only an integer, never a map/entity/key.
- Preserve queried chunks, membership, duplicate-key semantics, callback work,
  capture frequency, publication and thread ownership. No shared mutable map.
- On runtime config disable, stock iterator/construction return. Unknown versions
  skip hooks entirely. Optional targets/injections require0, no hard dependency.

Residual risks: pre-sizing can choose a different hash-table size if population
changes, so Map traversal order may differ although HashMap never guarantees it.
No visual/semantic equivalence is established merely by compilation; physical
visibility, transformed-call-site verification and frame measurement required.
Snapshot live nodes remain; this does not eliminate all snapshot allocations.
Empty whitelist optimization may have no effect if all dynamic sets are nonempty.
Repeated immutable BlockPos remains stock: shared mutable positions could escape
through modded block-state queries. Do not introduce that reuse without contracts.

## Runtime gate

Build succeeded22s. Candidate installed via authorized ADB, original0.2.12 JAR
disabled intact and support properties backed up.2816MiB, same optional G1
profile, same graphics/simulation/official server/camera; basic/deep/JFR off,
existing bounded per-frame capture on. Temporary GC info log and filtered mixin
class export for call-site verification, no new Spark/profile mechanism.

Candidate label visible-entityculling; raw C:/BootOptimBench/android-device-server-fps-20261011.
Comparison recent profile-only capture visible-affinity2-skipped:6.2972 loops/s,
p50/p95/p99 148.76/247.41/317.35ms,max459.33ms,8.86% frame wall>250ms,
mark-window187.07ms versus132.72 outside,19.991ticks/s. Actual display FPS not
measured. Weather/thermal/order vary; one candidate run cannot establish causality.
Results below. No candidate production promotion.

First connection rejected by the known intermittent registry bug:
create:mechanical_press unknown server item. No world capture from that attempt
is valid performance evidence. Saved entityculling-invalid-registry-game.log and
normal shutdown/rearmed collector for visible-entityculling-retry. Filtered
export/javap verifies freshSnapshot and recordSize redirects, and emptyWhitelist
in the BlockEntity overload. The Entity overload remains stock; do not claim
both overloads were changed. No new registry or server patch included.

## Separate Sable premise, not installed

Local user-edited Sable source contains the JFR-attributed unconditional
ObjectOpenHashSet in ParticleMixin.moveWithSubLevels. Existing worktree main has
unrelated user changes; untouched. Created separate git worktree
C:/BootOptimBench/sable-particle-lazy-20261012 from0c4cb7680839e4aaf44fc83d8d236f87bcc21c06,
branch codex/android-particle-lazy-allocation-20261012.

Draft lazily creates the stock set at first insertion, preserving capacity,
deduplication and insertion/traversal behavior when nonempty. With no candidates,
use shared empty set and omit five temporary Vector3d objects used only in the
loop. Keep all intersection queries, inherited movement, original.move,
onGround and tracking cleanup in original order. No pooling, shared scratch
state or cached ship membership. General source-level change, not Android-only.
Compile pending; NOT installed/promoted or claimed as performance win. Exact
phone fork/source alignment and semantic particle/ship validation required
before substituting the pack JAR. This is separate from EntityCulling trial.

## EntityCulling result: removed

Candidate session memory-1791802408799-6575, origin1791802761020,120.441s.
375 completed loops,3.1136/s,p50/p95/p99 252.34/745.61/970.98ms,max1182.78ms;
74.13% frame wall in >250ms frames; ticks18.025/s. Mark-window391.53ms versus
254.52ms outside. GC max196.689ms,total1253.089ms; cycles8.5–16.8s.

Return stock support0.2.12: session memory-1791803153673-19840,
origin1791803527562,120.438s.751 loops,6.2356/s,p50/p95/p99
141.80/278.64/357.12ms,max542.78ms;16.25% wall>250ms; ticks19.985/s.
Mark-window191.78ms versus139.25ms outside; GC max151.635ms,total1167.371ms.
Mark cycles7.6–7.9s. These match earlier profile-only ~6.3/s far more closely.

Candidate had heavy GLOBAL paging (mark pswpin5269pages/s,majorfault6053/s;
outside2586/3392) versus return control130/161 and220/408. MemAvailable
candidate315256/594264/1036968KiB; control470692/544856/607180KiB.
Global rates do not identify which process owns those pages. Battery temperatures
31.7 versus31.8C earlier control do not measure CPU/GPU junction temperature.
Day/night and memory state vary; cannot assign all regression to the patch or
assert a root cause from this one return comparison. No benefit demonstrated.

Disposition: candidate REMOVED from deliverable source and phone active mod set.
Experiment code retained in commitf2d76812a only for research. Restore original
support config and0.2.12; do not keep a disabled experimental switch in production.
Remaining GC spikes still not solved, not hardware-limit evidence.

Sable follow-up compile succeeded3m25s (:common:compileJava), commit4ed5589.
Pulled exact phone Sable2.0.3; javap comparison of method instructions excluding
constant-pool indices shows ONLY moveWithSubLevels changed. Packaged a diagnostic
copy of that exact JAR replacing only ParticleMixin.class. Verified all other ZIP
entries byte-identical and no signature entries invalidated. Source patch and
jar-contract hashes saved in raw folder. This avoids replacing user/native/other
compatibility changes with a fresh whole-mod build. Physical Sable trial pending.


## Sable physical result and attribution continuation

Session memory-1791803987162-28241,120.410s,739 completed loops,6.1374/s,
p50/p95/p99 144.92/298.65/386.43ms,max488.52ms;18.38% frame wall>250ms.
Ticks20.023/s. Mark-window199.23ms versus131.03ms outside. GC max99.184ms,
total1229.447ms; cycles7.6–8.6s,roughly14s apart. Control6.2356/s,p95 278.64,
p99 357.12: no coherent loop-rate or spike benefit. Global MemAvailable
372264/392488/413952KiB; low paging, unlike culling trial. No new particle/mixin
failure, but stationary view does not prove all ship collision behavior.

Disposition NOT promoted; exact original Sable restored on phone. Source draft
remains on isolated research branch, not production or an installed disabled
feature. No claim that allocated objects were dominant simply because sampled
traffic was high. No universal hardware limit established.

Existing JFR g1-attribution.jfr has ZERO ExecutionSample/NativeMethodSample events.
Sampled allocation weights did not select two frame improvements. Reuse existing
JFR mechanism with delayed330s, bounded180s/8MiB CPU/native samples at20ms,
CPU/thread CPU load1s and GC summaries; no allocation sampling, old-object walk,
heap dump or new Spark. Original mods restored. Existing frame capture and GC
info timestamps allow inside/outside-mark and >250ms-frame attribution. This run
is for source attribution, EXCLUDED from ordinary FPS performance comparisons.
Physical label visible-frame-cause, recording visible-frame-cause.jfr. Pending.
