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
