# Android memory: native resource census and ZIP backing trial

## Objective and scope

User target: at least 1 GiB available system memory without reducing final quality.
Physical Vivo Y72, Android 13, Mali G57 MC2, MobileGlues 2.0.0, Java 21.0.1,
3 GiB max heap, original 30 FPS cap, distance 3 / simulation 5, thermal mode.
8 GiB physical RAM does not establish viability on arbitrary smaller phones.
No BootOptim source changed. Integration reviewed: 058ac544aef11c0c10dc32e0aeacba2e39176d40.
No histogram/full-GC guard bypass, heap dump, blind cache release or quality change.

## Native resource census

Support 0.2.5-memory-census command `/wachilandandroid memoryresources`.
Local raw: `C:/BootOptimBench/android-device-memory-census-20261011`.
Explicit render-thread stored-field reads, bounded address deduplication, no
pixel reads, GL calls, resource mutation or persistent strong references.
400.22 MiB / 419,662,256 bytes in 64,536 registered live NativeImage addresses;
64,587 examined entries, no truncation, 285.36 ms traversal.

Owner payloads:

- Block atlas mip0: 216,727,236 bytes.
- Block atlas higher mips: 67,727,036 bytes.
- ElytraTrims features mip0: 56,165,376 bytes; one 128 x 104960 animated
  sheet accounts for 53,739,520 bytes.
- Dynamic Minecraft textures: 53,591,932 bytes. Six FancyMenu 1440 x 1440
  faces account for 49,766,400 bytes, plus two 2033 x 223 logos.

This is a lower bound for registered CPU image buffers, not total Bionic,
GPU, process RSS or a retaining-root graph. No multi-background FancyMenu leak
is established by these figures. Unknown/native owners remain unattributed.
Discarding mip images without auditing custom pixel reads and reuploads is
not approved. No optimization saving is claimed from this diagnostic.

## Generic ZIP backing candidate

Isolated `runtime-patches/zipfs-spill`, pinned to embedded OpenJDK source
060c4f7589e7 (21.0.1). Stock source verified against the official source commit.
Only readonly nested ZipFileSystem construction snapshots >=256 KiB become
private immediately-unlinked temporary files. Fresh filesystem/readers and
loader callbacks stay unchanged; writable/other channels remain stock.
This changes the storage premise, unlike rejected FML live-object reuse in
BootOptim PR247. No deduplication/callback bypass/forced close of live owners.
Read the runtime patch README for error and lifetime limitations.

Local raw: `C:/BootOptimBench/android-device-zipfs-spill-20261011`.
Session memory-1791675854806-20732, origin existing JVM memory wrapper.
ModernFix reports menu in 223.681 s (basic control 218.008 s); these are single
runs, not a statistically validated startup comparison. World entry 01:51:39,
63.88056 s from menu-to-world marker. Staging real-pixel validation passed.
The user later manually closed the game. Do not classify this as a crash;
prior in-world samples remain usable, controlled shutdown/reload gate missing.

Indicative medians from six samples each, 60-120 seconds after each recorded
world entry; same saved world and settings, not identical simulated actions:

| Metric | Basic control | ZIP candidate |
| --- | ---: | ---: |
| Heap occupied MiB | 2900.3 | 2786.4 |
| G1 old occupied MiB | 2771.9 | 2752.5 |
| Kernel MemAvailable MiB | 393.4 | 503.7 |
| Android available estimate MiB | 570.6 | 616.0 |
| Process RSS MiB | 4492.9 | 4340.2 |
| Process swap MiB | 1165.8 | 1308.6 |

Occupied heap is not live retained bytes. RSS decrease accompanied by higher
swap is not proof of freeing the same amount. Snapshot counters are cumulative,
not live saving: first 32 spills logged 72,657,071 bytes. The initial full
array still exists during spill, so initial decompression peak is not avoided.

Disposition: not promoted, target not achieved. Retain source/evidence as an
experiment; remove patch JVM arguments before next attribution run. Runtime
files were never overwritten. Startup/world passes do not establish reload,
all-mod semantics or disk-failure equivalence. No universal GPU/phone claim.

## Next bounded attribution

Support 0.2.6-model-census adds `/wachilandandroid memorymodels`: direct model
map sizes and unique stored SimpleBakedModel vertex-array payload, with class
counts. It complements existing JVM sampling; no new JVM profiler. No custom
getQuads/model evaluation, cache mutation or GC. Limits 50000 models, 100000
arrays, 200000 entries/quad references and 500 ms traversal. Unknown/wrapped
models and unbaked object graphs are explicitly excluded. A truncated lower
bound must not be treated as total model memory. Built successfully; physical startup, existing-world entry and normal save/quit passed. Raw run: `C:/BootOptimBench/android-device-model-census-20261011`.


### Physical model census result

Session memory-1791711054329-28418 (verify origin in raw session file before
using for timing). In-world command epoch 1791711478624. Screenshot 29 FPS at
original 30 FPS cap; no visual quality changes. Normal save/quit confirmed.

Map entries: modelResources 43993, unbakedCache 42030, topLevelModels 326752,
bakedCache 75764, bakedTopLevelModels 326897. Counts are not byte totals and
maps overlap; do not sum as unique models. 16125 unique baked values examined,
8667 SimpleBakedModels, 155195 quad references. Vertex-array cap reached at
100000 unique arrays / 12800000 payload bytes. Truncated=true; 236.39 ms scan.
This lower bound does not measure all model memory and excludes wrappers,
custom models, object headers, maps/keys, unbaked graphs and other owners.
Observed sampled classes include Decocraft BlockbenchModel (1263), Create
CTModel (721), DisplayDelight PlateHidingBakedModel (1285), BNB
DoubleOrientedBlockModel (1030), plus BootOptim's retained derived models.
These class counts are incomplete sample counts, not per-mod retained RAM.

The successful diagnostic does not yet identify the owner of most occupied
Java memory or achieve the 1 GiB target. Next useful work: separate map/class
coverage from the vertex-array budget, then attribute custom/unbaked storage
using version-pinned stored-field traversal. Do not remove live ModelBakery
caches merely because map sizes are large: NeoForge exposes its bakery after
reload and custom models may depend on it. BootOptim remains untouched.
