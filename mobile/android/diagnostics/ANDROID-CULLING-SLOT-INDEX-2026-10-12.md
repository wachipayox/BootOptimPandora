# Culling slot-index trial, 2026-10-12

User request: implement directly, run physical device with retained frustum
optimization, compare to that control. Repeat only when uncertain; retain obvious
benefit. No changes to BootOptim or user-owned Sable source.

## Implementation 0.2.16

Source: actual NeoForge 21.1.248 ClientChunkCache.Storage.getIndex is pure
floorMod(z, immutable viewRange)*viewRange + floorMod(x, viewRange).
CullThread JFR previously attributed 1758/4193 Java leaf samples to getChunk.
That does not prove all these samples are integer division; this physical trial
is required. Retained frustum candidate control loops 6.6446 and 6.6270/s,
p95 258.492 and 264.538 ms; p99 375.702 and 399.640 ms.

A gated Storage mixin replaces ONLY that arithmetic method. Last X remainder
and Z row are independently memoized, per storage, for one designated worker
thread. Thread identity published volatile; one-time synchronized ownership.
Other threads use exact original expression. No callback objects per lookup.
Storage viewRange is final: no data/event invalidation needed for this pure
function. Storage replacement automatically gets independent empty memo fields.
No caching LevelChunk, BlockState, opacity or world query results. getChunk still
runs every call, checks current range, atomically reads current slot, validates
coordinates and observes stock empty/null behavior. Modded world/ship queries,
block changes, unloads and packet updates keep their paths. No OpenGL or GPU
vendor assumptions. No new sleeping or simulation skipping.

Hooks off unless Android, enabled config, experimental.cullingSlotIndex=true,
and audited EntityCulling 1.10.4 present. Frustum remains true for both arms.
Build success; physical install is separate experiment branch based on retained
frustum commit c4f6b5f75. TOML now matches packaged version.

## Measurement

Same Vivo Y72, same safe server glass-box view, APK 0.1.13, 2816 MiB,
retained G1 profile, graphics/mods. Existing 90-second first-world fallback plus
settling, 120-second per-frame capture. No JFR/deep memory/basic sampling.
GC info log and existing 2-second GLOBAL vmstat sidecar, filtered Storage export.
Capture_start to capture_complete define origin/endpoint. Inclusive work overlaps;
completed loops are not presented GPU frames. Server weather/time/thermal/paging
may differ; inspect these before asserting causality.

Decision: compare entire frame distribution and slow-frame wall fraction with
retained frustum controls. A tiny delta within prior spread is not worth an
extra hook. Clear coherent gain can accumulate per user request; conflicting
metrics or material pressure variation require one immediate flag-off control.
Do not accumulate merely because code compiles or removes arithmetic operations.

## Candidate result

Session memory-1791813541551-15554; capture_start 1791813895370 to existing
capture_complete, 120.396 s counter interval. 797 loops / 6.6198 per second.
Frame p50 136.076 ms, p95 266.136 ms, p99 351.400 ms, maximum 451.835 ms.
Above-250-ms wall fraction 14.8220% (59 frames), ticks 20.0173/s.
GC pauses 1459.830 ms total, 94.762 ms maximum; intersecting mark cycles
7.4-7.9 s. Mark mean frame 172.875 ms, outside 128.663 ms.
Global MemAvailable 479924/515332/575664 KiB; global pswpin/out/major faults
outside 348.81/8.0/355.61/s, inside 285.37/6.89/293.59/s.

FPS and p95 essentially unchanged against retained frustum control 6.6270,
264.538 ms; >250 ms wall worse than 13.8620%. Candidate p99/max improve, but
previous control had a 281.749-ms GC pause vs candidate 94.762 ms. No coherent
new performance win. One immediate same-JAR flag-off control armed to resolve
this tail discrepancy, not an open-ended repeat series.

Filtered class export in candidate inherited the earlier library-only selector;
Storage export was not captured. Attempted pull failed and produced shell error
text, not valid bytecode; do not cite that as successful hook verification.
Correct selector used for immediate control; source/config/version gate match
previous working frustum gate, startup successful, but candidate transformation
was not independently observed. Negative result can still reject an unproven
benefit; do not report lower arithmetic cost as measured CPU savings.



## Immediate control and rejection

Same JAR, cullingSlotIndex=false, frustum=true. Session
memory-1791814386870-22821, existing capture_start/capture_complete origin,
120.012 s counter interval. 834 loops / 6.9493/s; frame p50 128.613 ms,
p95 253.060 ms, p99 434.513 ms, maximum 561.770 ms. >250-ms frame wall
12.0488% (43 frames), ticks 19.9897/s. Mark mean frame 168.452 ms versus
121.210 outside. GC pause 1063.894 ms total, 75.881 ms maximum.
GLOBAL MemAvailable 339904/359310/591192 KiB, inside mark paging
234.14/84.48/241.27 per second, outside 254.67/248.77/292.43.
Valid exported Storage control class confirms original two floorMod calls,
no Wachiland owner/memo fields; earlier invalid export is not used.

Disposition: reject/remove. Candidate has no FPS/p95/slow-wall benefit over
both retained-frustum reference and immediate control. Improved p99/max alone
conflicts with the rest and varies across control history; insufficient reason
to accumulate this overwrite. Do not claim all 4.7% difference is caused by it
because physical system pressure/time vary. No more repetitions of this micro
candidate. Restore previous 0.2.15 support on phone, retain frustum=true, remove
0.2.16 active JAR and source hook rather than leaving a disabled production
option. Keep experiment in Git history and durable research entry.

Conclusion limited to this implementation: arithmetic memoization is not an
established solution to repeated chunk-query cost. JFR getChunk attribution is
inclusive of inlined/modded paths; not proof floorMod itself is the bottleneck.
Future work must remove a larger verified part of query dispatch or redundant
queries with world/mod callback equivalence, rather than retry this memo.
