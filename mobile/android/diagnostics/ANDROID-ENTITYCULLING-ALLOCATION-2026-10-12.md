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
Results and disposition pending. No candidate production promotion yet.

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
