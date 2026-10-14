# Android empty Create behaviour ticks, 2026-10-13

Experimental branch from distribution4d8c7b956, engine6f021ee retained. BootOptim
read-only integration refreshed4813ab19aec2a508034cd7c5e36410ee3723411a.
No new runtime sampler/profiler. Existing deep JFR revealed324 leaf samples in
Create's behaviour-tick lambda; this is attribution, not proof of recoverable
time. EntityCulling whole-mod disable already failed to prove a net FPS gain.

Candidate changes only the literal `BlockEntityBehaviour::tick` implementation
handle in SmartBlockEntity.tick, preserving the original collection traversal,
callback ordering, virtual forEachBehaviour/getAllBehaviours, initialization,
lazy ticks and every custom tick override. The final ModLauncher-produced base
class is observed by the existing instrumentation delivery: only a method with
one RETURN, no exception handlers/synchronization/native/abstract/static status
qualifies. Unknown/transformed nonempty base methods stay original. A base class
redefinition disables the shortcut; classloader ownership is matched through
weak references and ClassValue. No behaviour/world instances are cached.

JVM property `wachiland.android.emptyTicks=true` installs the experiment only;
normal launch never requests it. Existing manual capture accepts a sixth optional
boolean `[staging [emptyTicks]]` for a same-JVM control/candidate/control. Scalar
skip/delegate counts are opt-in and recorded only with the test property; they
are not performance evidence by themselves. Single-client-thread counter origin
is declared; no atomics or per-callback timings are introduced.

Host fixture gate: stock, transformed disabled and transformed enabled retain
1500 visits/500 overridden ticks each across500 calls. Candidate skips1000 base
noops; nonempty bytecode rejected. Physical startup, scope guard, in-world visual
and repeated frame gates pending. Build is necessary, not a promotion.

Parallel memory audit: exact installed Sodium0.8.12-beta.1 mipmap/alpha-scan
methods already read native pixels directly. The suspected full Java pixel-array
copy in these methods does not exist; no speculative replacement implemented.
Existing ZIP spill and lower-heap trials remain distinct and unpromoted. Two
hidden startup LOW_MEMORY failures are unresolved; foreground service/battery
exemption cannot reserve physical RAM. Do not drop live texture/model caches.

Cleanup correction: the previous checkpoint restored an extra `<version>.cfg`
file instead of FCL's effective `fclversion.cfg`. Its ordinary-settings claim
was incorrect: the effective config still had the manual measurement arguments.
Readback caught this before this next run. Preserve/restore the actual filename
and restart the launcher after writing settings; otherwise its cached settings
can overwrite disk state. Frame recordings from the previous run remain valid
at their explicitly recorded real background2 probe origin.

## Runtime rejection

Physical run30960 installed the SmartBlockEntity reference hook but the base
tick guard reported empty=false. Exact installed Create6.0.10 bytecode confirms
private lazyTickCounter decrement/reset and virtual lazyTick dispatch. The
no-op premise is false; no ticks were skipped. Removed the experiment's runtime
classes and host harness from the shipping tree, rather than leaving dead code
disabled. Synthetic host fixtures were insufficient to establish the premise.

Menu192.665s/world260.742s from ModernFix's process-defined markers; startup
smoke only, not a speedup. Valid60.017s glass-scene capture5.9483loops/s,
p99457.344ms, >250ms wall26.548%,19.828ticks/s, rain0. 10,035,636 delegated
behaviour calls, zero skipped. This is a diagnostic run with an inactive
experimental dispatch wrapper, not an untouched-control FPS result. No A/B
repetition is warranted because the guard proves non-activation.

## Next isolated candidate: empty client chute output checks

Exact Create6.0.10 ChuteBlockEntity.tick still invokes output/capability/world
queries when its client-side item is EMPTY. The renderer omits that item, and
incoming authoritative item state resets the item-position interpolator.
The experiment wraps only tick's handleDownwardOutput/handleUpwardOutput calls:
exact ChuteBlockEntity, normal client, not virtual Ponder, EMPTY item and
simulation=true. Return false only there; all other calls delegate unchanged.
Keep SmartBlockEntity behaviours, lazy counters, air/particles, all tick/post-
tick callbacks and network/server ticking. No cache or GPU-specific selection.

State tradeoff must be explicit: an empty item's private interpolation position
can clamp instead of advance; arbitrary external consumers of that empty visual
position or skipped output-query side effects are NOT proven equivalent.
Do not call this universally compatible or promote from a static screenshot.
Unknown Create versions/custom chute subclasses are excluded. Opt-in JVM switch
wachiland.android.emptyChute.trial; same-JVM sixth capture boolean controls the
runtime arm. Optional scalar skip/delegate count is not a profiler or saving.
Physical guard/counters/frame comparison and semantic scope gates pending.

Support0.2.28 failed its runtime hook gate: inherited `isVirtual` was incorrectly
shadowed on ChuteBlockEntity. Optional Mixin failed open, so both90s phases had
zero delegate/skip counts. Their1.977/6.652loops/s difference is NOT a candidate
gain. First capture also began before ModernFix's final world-entry marker and
had heavy shader/initial settling; retained as startup-overlap evidence only.
Corrected0.2.29 declares the invoker on actual owner SmartBlockEntity. The
existing capture now rejects resource overlays; external runner additionally
waits for the world marker and60s settling before control/candidate/control.

Existing bounded native-resource census collected after timing:421,304,516bytes,
64,680 unique live NativeImage addresses, no truncation,333.8ms scan. Blocks
atlas base/higher mips216,762,052/67,737,916bytes. ElytraTrims animation sheet
128x104960 alone53,739,520bytes; its atlas56,165,376bytes. FancyMenu dynamic
images about55,188,496bytes under the minecraft namespace; six1440x1440 faces
are consistent with ONE panorama, not evidence of six loaded backgrounds.
This census is a lower bound, excludes GPU/other native owners and does not
authorize freeing live images. No GC/pixel reads/GL state changes requested.

Foreground pressure in run30960: max RSS5,196,304KiB, min global MemAvailable
262,584KiB, max own VmSwap1,474,684KiB. Distinct residency/swap/global estimates,
not additive. In run9473 world, young GC repeatedly leaves2396..2398MiB of2816;
remark2411..2442MiB. That does not justify blindly retrying a smaller heap:
already-live occupancy leaves little room for young/transient allocation.
Initial background startup and1GiB headroom are unresolved.

## Final physical chute gate: rejected and removed (2026-10-14)

Support29's corrected hooks activated on PID19267. Same dry glass pose,
90-second phases, completed runTick loops rather than display FPS, trace
overflow zero and approximately20 logical ticks/s:

| Phase | loops/s | p95 ms | p99 ms | max ms | >250ms wall % | skipped/delegated |
|---|---:|---:|---:|---:|---:|---|
| Control | 6.1615 | 310.645 | 429.380 | 536.661 | 23.7499 | 0/861270 |
| Candidate | 7.9860 | 197.264 | 313.896 | 389.701 | 5.3289 | 241884/455371 |
| Control return | 8.7104 | 178.103 | 222.099 | 342.913 | 0.74596 | 0/863110 |
| Candidate repeat | 9.7886 | 144.430 | 180.852 | 255.059 | 0.28396 | 241749/455223 |
| Control final | 9.9168 | 143.095 | 182.210 | 252.399 | 0.28060 | 0/863697 |

The first apparent improvement was warming/settling, not a proven optimization.
The uncertainty repeat shows no net frame/tail gain despite real reduced work.
No additional repetition justified. Removed all Chute mixins/runtime controls
and six-field benchmark protocol from shipping source; do not leave disabled.
Preserved diagnostic overlay rejection/census request and added window dimensions
for the next resolution discriminator. Support30 is diagnostic maintenance,
not a performance feature. Menu192.912s/world271.066s from ModernFix markers
are startup smoke evidence only. Raw five-phase metadata, frames and summaries:
`C:\BootOptimBench\android-background-memory-fps-20261013\retry29-*`.
