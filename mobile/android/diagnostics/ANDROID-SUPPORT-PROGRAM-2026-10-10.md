# Android support mod program

## User objective, 2026-10-10

Create a separate auxiliary Android mod that improves pack performance and
compatibility across GPU families. Default behavior preserves final visual and
game quality. Quality/performance tradeoffs may be added only as explicit,
reversible configuration options. Audit optimization and general mods for
Android regressions; remove optimization-only mods only when a measured
comparison supports that decision, or isolate a compatible workaround here.
RAM takes priority when it demonstrably interferes with runtime FPS. BootOptim
source remains untouched; portable memory optimizations belong in that project
only after its ongoing external work and required integration/history checks.

## Current foundation

Branch codex/android-support-foundation-20261010 starts from 4189d0db7 of the
diagnostic branch, retaining its research rather than duplicating a profiler.
New mod ID wachiland_android_support; standalone packaged JAR, no APK rebuild.
Plain properties file, conservative defaults, runtime status/config reload and
one-time GL vendor/renderer/version/capability report. On non-Android clients
these mixins are skipped. Diagnostics default off; old queue prototype remains
off. Small staging uploads now have a physical FPS candidate result; they are
not promoted because reload and broader correctness/GPU gates remain open.
See ANDROID-STAGED-UPLOAD-2026-10-10.md. Compile and physical FPS evidence do not
establish a production release.

## Evidence-driven order

1. Animated blocks-atlas update synchronization: native off-CPU stacks confirm
   glTexSubImage2D waiting in the Mali driver. Test small staging textures and
   GPU copies without duplicating the full atlas or changing image content.
   Preserve callback, texture/framebuffer binding, PBO and pixel-store contracts.
   Capability check plus pixel-equivalence/runtime/reload gates before promotion.
2. Memory pressure concurrently affects this workload: ~3.0 GiB process RSS and
   ~1.7 GiB swapped, heavy reclaim/fault traffic, only ~570 MiB available system
   memory in the measured interval. Attribute retained Java/native/model/image
   ownership before dropping caches. Increasing Xmx is not a default remedy.
   No arbitrary forced GC every frame or automatic cache deletion.
3. Controlled optimization-mod comparisons in the same loaded world and camera,
   after warm-up. Separate server CPU, JIT, driver CPU/waits and full-frame FPS;
   record thermal/charging state, memory and renderer. A removed visual effect
   must be an explicit opt-in tradeoff, not advertised as equivalent output.

See VIVO-UPLOAD-ORDER-AND-NATIVE-WAIT-2026-10-10.md for intervals, sample loss,
overlapping wall times and the limits of the Vivo observations. No thermal
improvement has been validated. Phone tests on one Mali GPU are not cross-GPU
validation.

## Mod audit queue (actual enabled pack snapshot)

| Mod | Evidence now | Decision / next gate |
| --- | --- | --- |
| Kerria 1.3.1 | Exact runtime rejects fast upload and GPU animation cache capabilities. The Kerria-off Spark capture still spends time in native uploads; wrapper inclusive time is not Kerria self time. | No evidence that its unavailable PBO/cache causes this stall. Keep disabled in current reference tests; do not repeat that rejected explanation or generalize removal across GPUs. |
| AcceleratedRendering 1.0.10.1-wedit | User-maintained fork reports unsupported; its feature gate rejects MobileGlues and desktop capabilities are missing. | No active fast path or demonstrated drastic overhead. Direct changes in that fork are possible if attribution identifies a specific fallback cost. |
| Sodium 0.8.12 beta1 | Exact JAR source shows visible-only animation skip; verified enabled. Render-ahead 3/1/3 did not meaningfully improve FPS. | Retain; avoid repeating the rejected limit tweak or disabling visibility optimization. |
| AsyncParticles 21.1.0b-beta.3 | Runtime tick_async=false, deferred=true; source requires async ticking for its queued texture path. | Do not blame deferred flag alone. Separate particle workload comparison if profiling warrants it. |
| EntityCulling 1.10.4 / MoreCulling 1.0.8 | Kerria-off Spark shows CullThread sleeping 94.47%; chunk meshing workers parked 98.67%. These are not evidence of continuous worker saturation. | Inspect source only when attribution warrants it; compare cost versus saved render work before removal. |
| ModernFix 5.27.14 / FerriteCore 7.0.3 | No demonstrated regression; memory pressure makes discarding memory optimizations especially unjustified. | Keep unless a specific path/regression is measured. |
| Chloride/Embeddium/Sodium integration 1.7.9 | Present; no causal evidence yet. | Check renderer callbacks/settings against exact version if trace points here. |
| C2ME NeoForge 0.3.0+alpha.0.93 | Present in the complete enabled manifest and reload log, including native math and scheduling components. The earlier abbreviated filename filter omitted it. Parked meshing workers in one Spark sample do not establish world-generation cost. | Attribute world-generation worker CPU, concurrency and memory before changing its policy; do not infer a steady FPS regression from presence alone. |

This is a queue of hypotheses, not a blacklist. Mod filenames and Java thread
names alone do not identify ownership or prove regression. General gameplay mods
are not silently removed to manufacture a faster result.

Reuse ANDROID-OPTIMIZATION-MOD-AUDIT-2026-10-08.md and
VIVO-SPARK-KERRIA-OFF-2026-10-09.md. Their Spark captures have different worlds,
entity counts and conditions and are not comparable FPS A/B measurements.
The later physical census also rejects repeated-upload batching (zero repeated
keys); it supersedes that earlier hypothesis without erasing its history.

## GPU/renderer policy

Distinguish physical GPU, vendor driver, GL translation renderer and optional
Vulkan/Mesa driver. Market share of a GPU is not a correctness gate. MobileGlues
provides a desktop GL interface on host GLES 3.x; its published compatibility
matrix includes both Mali and Adreno with device-specific outcomes. Keep it as
the current baseline, not a universal winner. A configurable implementation may
be selected by capabilities and verified behavior, with stock fallback and
individual kill switches. Unknown GPU/renderer starts conservatively. PowerVR
and Xclipse cannot be declared validated without evidence.

Primary references:
- https://github.com/MobileGL-Dev/MobileGlues
- https://github.com/MobileGL-Dev/MobileGlues-release/blob/main/ShaderSupportMatrix.md
- https://github.com/FCL-Team/FoldCraftLauncher/blob/main/README_EN.md

## Promotion gate

Compile; apply optional hooks without new crash; verify pixels/animation and
resource reload plus representative world behavior; comparable physical A/B
including process/native memory, thermal state and full-frame time. Unknown
callbacks keep stock behavior. No GL from workers, no retained native pointers,
bounded staging allocations, explicit context/lifetime invalidation. A moved
stall is not an FPS win. Shipping mechanisms need durable safety/configuration
entries; diagnostic code remains opt-in or isolated.
