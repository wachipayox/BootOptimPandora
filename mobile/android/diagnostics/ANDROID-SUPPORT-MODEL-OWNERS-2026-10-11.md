# Android model owners and guarded heap request — 2026-10-11

Diagnostic branch: codex/android-native-image-census-20261011, based on 50c40d7eb. BootOptim untouched. No production RAM optimization promoted.

## Physical 0.2.7 result

Raw evidence: C:/BootOptimBench/android-device-model-owners-20261011; successful session memory-1791712646015-6266. Same Vivo Y72/Mali/MobileGlues, 3072 MiB Java heap, original 30 FPS cap, distances 3/5, thermal mode. Existing snowy world reached and normal save/quit passed. Screenshot after census: 29 FPS. This is not an FPS benchmark.

Map entries: resources 43993, unbaked 42030, top-level unbaked 326752, baked cache 75764, top-level baked 326897. Maps overlap; counts cannot be summed as unique objects.

Bounded stored-field census: 65536 unique baked models examined, 400000 unique vertex arrays, 51200000 payload bytes. Truncated=true; approximately 1.4 seconds traversal. This is a LOWER BOUND excluding headers, wrappers, map keys, many custom graphs and unbaked storage. No getQuads calls, GC or cache mutation. Increasing sample coverage does not establish total model RAM.

Decocraft pinned JAR SHA256 b0589eb7d03b13bbf3b9c45df7f50db556a721882e9ad2bb6be0ff23e5a64526: 3546 static map entries share 654 unique parsed BBModels. Identity-deduplicated texture source strings total 25444810 characters; zero stored texture image entries. Elements 42921 references, animations 60, animators 264, keyframes 15819. No truncation, approximately 69 ms. Characters and references are not retained heap bytes. Animations are consumed in world; clearing them would break behavior. Public texture source fields cannot be cleared solely because this JAR has no internal reads; external consumers remain a compatibility risk. The likely modest saving does not explain GiB-scale heap pressure.

Latest pre-exit sample: Android ActivityManager available about 782 MiB, kernel MemAvailable about 647 MiB; these are distinct estimates, not additive. Java heap occupied about 2876 MiB, old generation about 2754 MiB. Occupancy includes garbage. The 1 GiB target has NOT been achieved.

## Intermittent startup failure

First 0.2.7 attempt failed rendering the loading overlay: Missing layer definitions [xercapaint:easel#main, xercapaint:easel#canvas]. Exact unchanged retry reached menu/world. Census commands had not run before failure; no new mixin in this diagnostic build. Init ordering/Connector is a hypothesis, not confirmed cause. Raw startup-crash.txt and first-attempt-game.log retained. No missing-layer workaround or mod removal applied.

## 0.2.8 request premise

Existing APK 0.1.11 already contains HeapAttribution with shallow occupied/live histograms. Reuse that helper on demand rather than add another JVM profiler. /wachilandandroid memoryheap attempts once per process. Basic sampler must be active, resource reload finished, frame profiler absent. Fresh authentic Android row must match current PID, be at most 15 seconds old, report >=768 MiB available and lowMemory=false. Publish its measured timestamp to the original guard; never fabricate freshness or lower the threshold. Recheck before each occupied/live phase. Failure skips diagnostic without mutating caches.

Live histogram requests one full GC and may pause the game. It changes workload and cannot serve as a timed performance run. Shallow class sizes cannot assign array bytes to per-mod retained owners. No heap dump, JFR start or root traversal. Reflection only bridges the launcher's existing optional helper; unavailable helper fails open. Build passed. Runtime result pending.

### 0.2.8 physical guard result

Raw: C:/BootOptimBench/android-device-guarded-heap-20261011, session memory-1791713611083-14625. Menu and existing world reached. Requested at epoch 1791714092326. Actual row age 3772 ms, matching PID, available 595365888 bytes, lowMemory=false. Original guard failed correctly; no occupied/live histogram, no full GC, no JFR. Normal save/quit passed. This validates skip behavior, not histogram success. During the session available headroom fluctuated, briefly crossing 768 MiB; Java object attribution remains unresolved.

### 0.2.9 bounded armed request

Same threshold and helper, now one armed request waits at most 120 seconds, checking the existing sampler every ten seconds; no allocations scan while waiting. It invokes occupied once if an authentic row qualifies and immediately rechecks before live. No forced retry after a histogram attempt. Time-window expiry skips both. This prevents an arbitrary command timing from missing a temporary valid margin without lowering the safety threshold. Build passed; physical test pending. Threshold is a conservative diagnostic policy, not a guarantee against Android/OEM killing the process.

### 0.2.9 guard expiry

Physical session memory-1791714342789-23355 reached menu and snowy world (29 FPS screenshot at 30 cap). Armed request epoch 1791714866664; all 12 authentic samples failed the unchanged 768 MiB threshold. Available during wait 436617216 to 640139264 bytes. Expired after 120 seconds; no histogram/full GC. Normal save/quit confirmed. Raw in android-device-guarded-heap-20261011. No attribution success or RAM saving claimed.

Next controlled premise: try Java maximum heap 2560 MiB instead of 3072, with all visual/mod settings unchanged. Smaller occupied heap may reduce Android pressure, but may instead cause GC thrash/OOM. Preserve original fclversion.cfg and restore on failure or before finalizing unpromoted trial. Raw C:/BootOptimBench/android-device-heap2560-20261011. This is a different heap condition and must not be silently mixed with the 3 GiB baseline.

### 2560 MiB rejected

Session memory-1791715186692-31605, JVM max verified 2684354560 bytes. Menu reached but existing world stalled preparing resources. From session 370.393 to 403.332 seconds, GC time increased 78350 to 107606 ms (29.256 s in 32.939 s); old generation near heap ceiling with zero young occupancy. At 467.485 s, occupied heap 2675112272/2684354560 and cumulative GC 170914 ms. This is GC thrash, not an acceptable RAM optimization. No successful world entry. Escape did not cancel loading. Recorded raw telemetry/log/screenshot, stopped this failed loading attempt, restored original 3072 MiB config. Last successful world had already been saved normally before trial. Do not present this as a saving or silently leave smaller heap enabled.

Next: original launcher one-session deep attribution armed, 3072 MiB restored, performance profiler disabled. Existing bounded JFR allocation sampling may attribute arrays without a full heap census. Original fresh >=768 MiB guard continues controlling any occupied/live census; no guard bypass. Diagnostic runs are excluded from benchmark timings.

## Existing JFR attribution — successful collection

Original APK deep attribution armed via its one-launch preference, 3072 MiB restored. Session memory-1791715845638-3673, raw C:/BootOptimBench/android-device-jfr-memory-20261011. JFR active (20 allocation samples/s target, 16 MiB rolling target, old-object cutoff zero/no roots); snapshots succeed. Menu marker ModernFix 230.979 s is diagnostic workload, NOT comparable performance timing. Original >=768 MiB guard skipped menu class census. No histogram or full GC claimed.

Use adb exec-out run-as ... cat for binary JFR exports. The first shell -T export failed JFR metadata validation and is excluded. exec-out snapshot parses correctly with JDK21 jfr; early snapshot four chunks/120 s, 2177 allocation samples, 770 old-object sample events. Deduplicate old samples by (address, allocationTime) because snapshots repeat samples; sampled object sizes are not a total retained graph and have no retaining roots.

Early cumulative estimated allocation weights show LongOpenHashSet construction under Palladium FastBitSet around 1946.66 MiB and reflection Field.copy around 2997.51 MiB. These are ALLOCATION ESTIMATES, not GiB of live RAM. Full stacks attribute reflection to Mixin Bytecode.getOpcodeName, and FastBitSet to voxel-shape joins including Create/Catnip shape rotations. Do not modify BootOptim/ASM without refreshing its integration ledger; this turn leaves it untouched.

Pinned phone Palladium 1.1.8 decompilation: FastBitSet extends BitSet, calls super(nbits), and also new LongOpenHashSet(nbits). Capacity is requested in number of bits rather than dense words, with an additional hash representation while retaining superclass bits. Existing fastBitSets option controls the shape redirect and falls back to original BitSet. Candidate is disable only this option, leaving all other Palladium features, heap and visual settings unchanged; must verify actual startup/world behavior before promoting.

Menu snapshot 470 unique sampled old objects, summed sampled shallow size 379.55 MiB; approximately 248.18 MiB from ZipFS newByteChannel readAllBytes plus 49.64 MiB UnionFileSystem Files.readAllBytes and 27.05 MiB ZipFS central directory. This strengthens ZIP buffer attribution but does not prove all are still retained, explain all heap, or promote the earlier spill experiment. Font/model samples present; no blind cache clearing.

Palladium source caveat: fastBitSets=false gates BitSetDiscreteVoxelShape's constructor redirect and retrogen branches, but MultipartBakedModelMixin.getSelectors falls through to its FastBitSet creation even after setReturnValue when false. Therefore this trial is specifically stock voxel-shape storage, not a claim that all Palladium BitSet replacements disappear. No third-party source changed; all other configuration entries retained. The superclass dense BitSet plus LongOpenHashSet storage is source-confirmed; large allocation stacks under Create rotations are measured, retained-memory benefit remains unproven.

## Palladium trial disposition

Basic session memory-1791716375782-11123, 3072 MiB, unchanged graphics/mods apart from fastBitSets=false. Menu ModernFix marker 217.935 s; existing world reached, screenshot 28 FPS at 30 cap, normal save/quit passed. Last six Android samples 635.8–662.5 MiB available (kernel about 533–558 MiB); Java occupied 2768–2876 MiB, cumulative GC around 14.1 s with only 279 ms added during sampled 50 s. This does not achieve 1 GiB headroom or prove an end-to-end/FPS win. Source indicates substantial transient hash allocation, but no large measured steady RAM saving here. Restore original Palladium option before the next isolated trial; no auxiliary compatibility patch promoted.

## New ZIP spill premise: missed constructor depth

Inspect the LATEST old-object batch (2026-10-11 12:55:51), not a union of every earlier event. Last five seconds contain 184 unique sampled objects, summed shallow sizes 239.44 MiB. About 191.93 MiB are ZipFS newByteChannel arrays; full stacks split 129.09 MiB through JarJar PathFileSystem with ZipFileSystem constructor at JFR frame index 17, versus 62.84 MiB through a shorter union path with constructor index 14. Addresses/allocations are sampled, not a retaining-root graph or a complete class histogram.

Prior spill eligibility examined only the first 16 StackWalker frames. The JarJar wrapper path has a deeper constructor and was missed. This is a new, concrete premise, not repetition of the unchanged previous experiment. Change bounded search to 32, keep >=256 KiB/read-only/nested-constructor/pinned JDK guards and all original fallback behavior. Log guardDepth=32. Compiled isolated module patch successfully. Physical trial uses only temporary JVM args; 3072 MiB and original Palladium restored, no JRE overwrite. Raw C:/BootOptimBench/android-device-zipfs-depth32-20261011. Target remains unachieved; test pending.

## Depth32 isolated first result

Basic session memory-1791717554707-18655. GuardDepth=32 emitted 64 channels /145314142 cumulative snapshot bytes, versus prior first experiment's 32/72657071 logged threshold. Counter does not count actual live saving. Menu ModernFix marker 219.924 s, snowy world screenshot 28 FPS at 30 cap. World was already entered by the next observed screen; entry input/timestamp not controlled in this run. Last six Android samples 583.6–614.3 MiB available, kernel roughly 469–518 MiB, swap roughly 1.26–1.30 GiB. Java occupied oscillated 2706–2883 MiB, max 3072, cumulative GC 9.7–10.5 s. No 1 GiB headroom and no reliable end-to-end saving claim. More coverage alone is not sufficient for production promotion.

Next candidate explicitly combines depth32 with Palladium fastBitSets=false and 2816 MiB max heap. This changes two controls relative to the depth32 trial, so any result is a combined profile result, not per-mechanism attribution. Existing 2560 MiB-only rejection remains valid. Restore before finalizing unpromoted experiments.
