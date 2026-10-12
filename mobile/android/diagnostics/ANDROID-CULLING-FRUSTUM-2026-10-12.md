# Frustum-first culling query experiment, 2026-10-12

Continuation of ANDROID-ENTITYCULLING-ALLOCATION-2026-10-12.md. Failed collection
and particle candidates removed/restored; no claim from allocation counts alone.
User requested continuing priority frame spikes; Spark remains parked.

## Measured premise

Bounded execution JFR in the actual safe glass base scene: CullThread mean CPU
machine fraction0.09285 versus Render0.1110;4193 Cull Java samples in frame window.
ClientChunkCache.getChunk1758 (~41.9%), isAABBVisible896,isVisible605. Culling
queries rather than tiny iterator allocation are a stronger CPU/memory-traffic
front. These samples are not exact per-method critical-path durations.

## Candidate, separate Android support0.2.15

experimental.frustumCullingQueries=false by default. Early hooks only for Android
and audited EntityCulling1.10.4. No hardware-vendor assumptions or GL calls.

- On main-thread LevelRenderer.prepareCullFrustum completion, copy the Frustum
  and capture immutable camera position/current-level identity/time.
- Frustum copy owns independent intersection and matrix; shared viewVector in
  vanilla copy is never mutated/used by this helper. Worker uses isVisible only.
- On CullThread isAABBVisible, finite ordered SAME corrected min/max bounds and
  camera must match snapshot within0.001, level must match, UI closed, age<=500ms.
- If copied frustum says entirely outside, return VISIBLE to skip costly rays.
  This shortcut never introduces a new hidden object. Inside/unknown/stale/invalid
  cases execute original ray method. Existing Nowheel custom bounds already apply
  before this library call; do not substitute block-position-only bounds.
- Clear snapshot at world transition so old ClientLevel is not retained. No pooled
  positions/world read cache or callback reuse. Diagnostic scalar counters write
  only at world transition after measured interval; no query/frame file writes.

Potential semantic/visual gate: omitted offscreen ray queries modify timing and
internal occlusion cache population, may admit extra offscreen draws; unknown
special render passes/custom bounds require validation. Stationary physical scene
cannot prove all camera rotations/ship motion/reloads/shader contexts. No production
promotion just from build or skip counts. Unknown versions remain stock.

Compiled build succeeded15s. Source Frustum/LevelRenderer audited from actual
NeoForge21.1.248 sources; public copy constructor and prepareCullFrustum signature
verified. Vec3d fields x/y/z verified in exact phone1.10.4 JAR.

Physical trial visible-frustum-query: same APK0.1.13,2816MiB, G1 profile, original
Sable, same server/camera/mods/graphics; support0.2.12 preserved disabled for rollback.
Basic/deep/JFR off, existing120s per-frame capture on; GC info log and2s GLOBAL
vmstat sidecar. Filtered library export verifies injection/accessor presence.
After world disconnect read wachiland-culling-frustum.txt for real query/skip
counts. Clean baseline recently6.2356 completed loops/s,p95 278.64,p99 357.12ms,
mark-window191.78ms versus139.25ms outside; conditions still vary. Immediate same-build flag-off control pending.


## First physical candidate result (not a promotion)

Origin: existing capture_start marker 1791810608674, session
memory-1791810238666-27357; endpoint existing capture_complete, 120.399 s
counter interval. Completed Minecraft loops, not GPU/display presentation FPS.
800 loops / 6.6446 per second; frame p50 136.802 ms, p95 258.492 ms,
p99 375.702 ms, maximum 551.607 ms. Frames above 250 ms occupied 11.5038%
of captured frame wall time (42 frames). Tick rate 19.9919/s.
GC pauses total 1146.108 ms, maximum 66.236 ms; eight concurrent mark cycles
8.5-9.0 s. Mean frame during mark 171.947 ms versus 128.564 ms outside.
Global MemAvailable min/median/max 389348/408224/466780 KiB; low paging
relative to the rejected collection experiment. Global counters are not
attribution of unique game memory or game-only faults.

After disconnect: 5,702,887 queries and 804,866 skipped (14.113%). These are
whole-session counters, not the exact timed interval and not CPU time saved.
Filtered export confirms library injection/accessors; nonzero skip count proves
snapshot path was exercised. No observed crash in this stationary scene; broader
camera, moving ship, reload and special-pass correctness remain unvalidated.
Compared with earlier clean control: small loop-rate/p95 improvement, p99 worse.
Not enough evidence of benefit or disappearance of spikes. Immediate flag-off
control uses the same APK, support JAR, heap, G1 profile and capture settings.

Maintenance note: candidate currently creates a short-lived AABB per eligible
query. Millions of queries make that an allocation concern, though escape
analysis may remove some. Do not infer retained heap or CPU cost from count;
finish control before adding an independently measurable refinement.


## Immediate flag-off control

Same 0.2.15 JAR, flag false; origin session memory-1791811613038-2184
capture_start and existing capture_complete, 120.380 s counter interval.
524 completed loops / 4.3529 per second; frame p50 191.670 ms, p95 518.937 ms,
p99 597.121 ms, maximum 699.907 ms. Above-250-ms frames occupied 47.5394%
of frame wall (149 frames). Client ticks 19.7209/s. No registry failure; same
stationary camera, verified safe box/server by screenshot.
GC pauses total 889.481 ms, maximum 75.207 ms; seven intersecting concurrent
mark cycles. Mean frame inside mark 305.517 ms versus 191.909 ms outside.
Global MemAvailable 685676/762132/796636 KiB, substantially MORE free memory
than candidate; however global paging/fault rates were greater:
inside mark pswpin 401.04/s, pswpout 561.19/s, pgmajfault 479.74/s;
outside 240.69/302.71/264.58. No per-process attribution from these counters.
Candidate has promising tail reduction against this immediate control, but
control differs from previous clean baseline and paging is a confounder.
Do not report +52.6% as an established repeatable optimization win.
Same-candidate repeat armed with no source changes to close A/B/A.


## Same-candidate repeat and disposition

Origin capture_start 1791812694680, session memory-1791812316339-9247;
existing capture_complete, 120.417 s counter interval. Same source/config as
first candidate, no intermediate optimization changes. 798 completed loops /
6.6270 per second; frame p50 135.487 ms, p95 264.538 ms, p99 399.640 ms,
maximum 654.738 ms. Above-250-ms frame wall fraction 13.8620% (51 frames).
Ticks 19.9723/s. Mark-window mean frame 175.663 ms versus 127.034 ms outside.
GC pauses total 1489.041 ms, maximum 281.749 ms. Global MemAvailable
392564/416982/464156 KiB. Paging remains a confounder: inside-mark global
pswpin/out/pgmajfault 329.47/573.55/359.09 per second; outside
273.96/229.96/287.86. Same camera verified at end; server day/night changed.

A/B/A result: 6.6446 / 4.3529 / 6.6270 completed loops per second, p95
258.492 / 518.937 / 264.538 ms; above-250-ms wall 11.504 / 47.539 / 13.862%.
Candidate result reproduced closely on both sides of the immediate control;
credible promising mechanism, not a solved-spike or 30-FPS result. Historical
clean control 6.2356 shows baseline variance, so no general +52% gain claim.
Keep isolated on diagnostic branch; no production/default-on promotion yet.
Next gates: eliminate per-query allocation with exact finite-bound equivalence
if worthwhile; moving-camera/ship/reload validation, then matched repeated
controls and CPU attribution. Original support remains available for rollback.
Frame source/ticks are inclusive wall and cannot be summed as disjoint work.


User disposition after A/B/A: explicitly retain this improvement to accumulate
with later optimizations. This supersedes a reject merely because the remaining
spikes are still perceptible. Retention is provisional: broader visual gates
remain outstanding; do not misread the decision as proof of all mod/render-pass
contracts or solved spikes. Phone keeps 0.2.15 with filter enabled, original JAR
available disabled for rollback; restore user 2816-MiB config without temporary
GC/export/JFR args, turn off armed capture flags after normal disconnect/quit.
Repeated whole-session query count 4,257,205, skipped 707,133 (16.610%).
Next measured front: repeated chunk/state lookups in CullThread. Prefer bounded
per-query/per-pass reuse with explicit invalidation and custom-world fallback;
do not bypass modded world semantics just for a lower lookup count.
