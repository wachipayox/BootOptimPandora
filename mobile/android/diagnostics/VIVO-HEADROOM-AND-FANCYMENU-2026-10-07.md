# Vivo 0.1.6: world-creation LOW_MEMORY and old FancyMenu pack binary

## Physical evidence / origin

User supplied `wachiland-android-diagnostic (3).zip`, reported main menu success
then termination when entering world creation. Android 13 vivo V2041, APK 0.1.6
(code 7), MobileGlues 2.0.0, `-Xms512m -Xmx3072m` confirmed in effective arguments.
No attached USB device. These are within-run diagnostic observations, not a
controlled startup-performance A/B; stages differ from the previous failed run.

Session PID 11199, process marker 2026-10-06T23:51:18.926Z, probe origin .931Z.
Matched foreground (importance 100) LOW_MEMORY at **23:57:25.782Z**, no JVM
completion callback or Java OOM/native fatal. Latest Android sample
23:57:20.967Z is 4.815 seconds before kill; JVM sample 23:57:21.486Z is 4.296
seconds before kill. ExitInfo RSS/PSS are older system samples, not exact peaks.
ModernFix startup marker is 23:56:25; select_world_screen registered 23:56:41;
data/tag/recipe/loot preparation follows. These boundaries do not establish
successful world creation or gameplay.

## Memory gate outcome

- First JVM sample committed 512 MiB: launcher candidate took effect.
- Heap grew to 3072 MiB. Final occupied 2640.2 MiB, G1 old occupied 2581.9 MiB,
  young 54.9 MiB, nonheap 425.2 MiB, direct buffers 13.3 MiB. Occupancy is not a
  retained/live-object census or a physical-memory partition.
- Final proc RSS **4481.8 MiB**, proc VmSwap **1909.2 MiB**, available system RAM
  **268.0 MiB**. Max sampled RSS 4788.9 MiB; proc high-water mark 5056.3 MiB.
- VmSwap reached **3508.8 MiB** when RSS was ~2902.2 MiB. A resident-only figure
  under 4 GiB omits many logical pages already swapped out. Do not add swap
  uncompressed bytes as physical RAM consumed: compressed/storage backing has
  different costs. No exact per-process zRAM physical allocation is exposed.
- Bionic native allocated grew from ~896 MiB at t=244 s to ~1875 MiB at t=307 s;
  final **1857.4 MiB**. ART occupied final **20.5 MiB**. Graphics at last detailed
  snapshot (t=339 s) **782.3 MiB**, not simultaneous with the final samples.
- Android vendor PSS still disagrees with proc RSS: overlapping/asynchronous
  counters cannot be summed into an exact memory decomposition.
- 46 memory callbacks recorded. Basic snapshots summed 9837 ms background wall
  (max 2621 ms); one initial mapping scan 2103 ms. Reduced repeated scans worked,
  but no claim that all instrumentation is cheap or that this is a timed run.

The physical gate remains failed: smaller minimum helps avoid initially forcing
3 GiB commitment but is not sufficient once actual allocation pressure grows.
Do not promote PR #82 as a proven complete memory fix or increase Xmx by default.
Android LMK reacts to whole-system pressure, not a universal 4 GiB application
ceiling. Available RAM was genuinely low; user laptop success does not prove
equivalent OS/runtime/driver/memory budget on this phone.

## Concrete resource front, no speculative mod removal

Native growth substantially precedes entering world creation and coincides with
FancyMenu preload (23:55:45 through resource completion 23:56:13). Current server
child revision `profile_wachiland-elite-android` / `rev_b7ef73bb1136f9546f6a7a3e4599cbdf`
inherits root `rev_0d8a9ba50f96f192a3b9b7b862b3dda1`. Config/image audit used that
immutable manifest and normal Windows TLS validation. Configuration payloads
were SHA-256 checked; image dimensions came from PNG headers, not full decoded
image allocations. Python's older SSL chain failed validation; used normal
Windows HttpClient rather than disabling certificate checks.

Pack has 21 panorama directories (126 PNGs), potential decoded RGBA total
931.76 MiB. All FancyMenu PNG assets together: 153 files, 1026.48 MiB potential
RGBA. `preload_resources` explicitly requests 20 panoramas, two slideshows and
two ordinary title images. Mapping the property names to directories resolves
120 configured panorama PNGs / **925.76 MiB** potential RGBA. Ordinary resources
and other mods still need memory.
Not every shipped image necessarily loads; these totals are source-size
calculations, not measured ownership. They identify a plausible ~1 GiB avoidable
native allocation front, not proof that all native growth belongs to FancyMenu.

Exact published FancyMenu JAR (parent mod entry, not replaced in child):

`mods/(svfr) (dep 109) FancyMenu -117- {v3.9.0-wedit} [1.21.1] [MAINLOC].jar`

SHA-256 **8e1c68f2c91aed02057209252bbe221bf3b019c4e82fb20fe35809bac2c08db8**.
Downloaded audit copy hash verified. javap proves only the old `preLoadAll(long)`
API and no `preLoadOnlyInitialLayoutResources` option. Its loop visits the full
configured list. `PngTexture` retains a NativeImage / possible DynamicTexture
until explicit close; the old pipeline has no selected-background filter.

Current local user-owned FancyMenu-welitemodpack source (14f9dba5) already has
the selected-initial-layout overload, safe fallback and default-true option
`preload_only_initial_layout_resources`. Parent options file has no such key,
so new-source default applies unless the user's phone configuration has since
set it false. This is an already implemented fix missing from the published
pack, not a reason to create another BootOptim bypass or lower image quality.

## History and user decision

BootOptim integration refreshed: 058ac544aef11c0c10dc32e0aeacba2e39176d40.
Read initial-layout-preload research and panorama-overlap catalog, plus merged
PR #116 body. Historical generic BootOptim runtime defer remains rejected;
the direct user-owned FancyMenu selected-layout optimization is retained.
Old/closed experiment status must not override that later retention decision.

User explicitly proposed using their latest FancyMenu version, which contains
this fix. Next simplest gate: replace the old JAR in the Android pack/instance
with the latest controlled build, ensure exactly one FancyMenu JAR and
`B:preload_only_initial_layout_resources = 'true';`, keep full serialized
preload source list, MobileGlues, Xmx3072m and current launcher fixed. Verify
`Initial-layout preload filter kept ... skipped ...` in the next log. Test
world creation, then export the diagnostic. Android compatibility and total
memory reduction are still unproven; no new APK, mod change or server publication
was performed during this analysis.

## Additional savings audit requested by user

Keep renderer, quality and behavior invariant when evaluating these fronts.
No further diagnostic APK or optimization was added in this analysis.

| Priority / front | Evidence | Savings boundary / required proof |
| --- | --- | --- |
| Existing FancyMenu selected-background fix | Published exact binary lacks the feature; full configured panorama pixels ~925.76 MiB and ~1 GiB native growth during preload | Highest-value first phone gate. Potential decoded-byte reduction is not yet a measured resident-memory saving. |
| FancyMenu CPU image lifetime after GPU upload | Current `PngTexture` retains NativeImage plus DynamicTexture; pixels close only at `close()` | A typical selected six-face 1440x1440 panorama alone has 47.46 MiB CPU pixels. `open()` requires NativeImage to re-encode an image, so blindly closing it changes API behavior. Need lazy reread/reconstruction or a proven no-reader owner path, correct upload/context/reload behavior, and visual validation. |
| Later client resource reloads | Current MixinMinecraft selected-background filter is guarded by `isBeforeFinishInitialMinecraftReload()`; subsequent reloads use full list | Could reintroduce the large preload after resource-pack/F3+T reload. World-creation server data reload is not proof of a client texture reload. `ResourceHandlers.reloadAll` releases old handlers, so no general generation-leak claim. A retained-background policy needs existing random selection/reload/error contracts; generic earlier defer remains rejected. |
| Java retained model/data owners | Final G1 old occupancy 2581.9 MiB, nonheap 425.2 MiB; heap reaches maximum despite smaller minimum | Old occupancy includes collectable objects. Need retained-owner or post-collection evidence before targeting caches/models, not a blanket forced-GC tweak. Existing BootOptim PR #336 is allocation/CPU diagnostics, not a live heap census or Android evidence; no duplicate profiler started. |
| Atlas / renderer allocations | 8192x8192x2 block atlas, last detailed graphics category ~782 MiB | If RGBA8, a single base+two-mip copy is 336 MiB (256+64+16), not measured total. Better packing or obsolete-buffer release could preserve pixels; lowering mip levels/resolution would change quality and is not chosen. Historical Decocraft sprite deletion #79 remains rejected; no claim this phone atlas can be halved. |

Renderer source audit (public main 97558a6, not proven binary-equivalent to
installed V2.0.0) found metadata maps, direct GLES buffer allocation/map calls and
temporary texture conversion, no demonstrated global full-buffer CPU shadow
cache. Shader-translation cache is disabled at `maxGlslCacheSize=0` (source
returns early for <=0); installed log already reports 0. There is no supported
large saving from disabling it again. Do not attribute native/graphics counters
to MobileGlues solely because it is the renderer. Native allocation ownership
requires actual object/resource counters, not filename mapping sizes.

The selected pack menu PNGs have no repeated SHA-256 entries, so a generic
content-hash image cache offers no demonstrated benefit. Launcher ART is only
~20.5 MiB near failure; no growing UI object heap is shown. Launcher framework/
native surfaces still contribute, but a claimed multi-GiB UI leak is unsupported.
Effective JVM flags contain ActiveProcessorCount=8, with no explicit compressed-
oops disable or forced high-Xms/AlwaysPreTouch remaining. Do not invent an absent
flag problem or reduce worker concurrency without peak/performance evidence.

Next measurement should hold launcher 0.1.6 / renderer / Xmx / pack settings
constant while replacing only the old FancyMenu JAR with the latest controlled
build and confirming the filter marker. This isolates the strongest identified
owner before choosing more intrusive native/JVM attribution. User will supply
the next diagnostic; no mobile connection is required for these existing probes.

Primary platform references:

- https://developer.android.com/reference/android/app/ApplicationExitInfo#getRss()
- https://developer.android.com/topic/performance/memory-management
- https://source.android.com/docs/core/perf/lmkd

