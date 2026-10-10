# Terrain drawing follow-up (diagnostic, not promoted)

## New premise after staging

The previous reference world was capped at 30 FPS in options.txt. That was a
configuration ceiling, not a hardware limit. A bounded 60-FPS trial on the same
Vivo, renderer, pack, camera and quality settings measured 6292 complete frames
over 119.801551436 s: 52.519 FPS. Origin/end are the existing runTick wrapper's
post-warm-up capture boundaries (60 s warm-up, 120 s capture). JFR/basic memory
recording was off. Existing bounded sprite attribution was on. This is a new
candidate attribution run, not an A/B against the earlier JFR-enabled screenshot.
Battery stayed approximately 34.3–34.9 C on USB. A snapshot temperature does not
prove sustainable thermals or GPU frequency stability.

Two later simpleperf recordings are separate intervals, not part of that FPS
measurement. First: 14.9338 s, 134540 samples, 19196 lost (7798 userspace); the
loss prevents precise extrapolation. Follow-up uses 49 Hz, 8192-byte dwarf
stacks, 32 MiB user buffer and 15 s request: 14.9125 s, 151539 samples, 2462 lost
(2361 userspace), 2528 truncated stacks. Owner TID 25644. Whole-app counts include
off-CPU samples; they are not render frames or CPU-only samples.

Follow-up owner on-CPU: 454 samples, estimated 9.2653 CPU s; substantial unknown
JIT and proprietary-driver symbols remain. Owner off-CPU: 13059 samples,
5.1062 sampled s. Inclusive mg_glMultiDrawElementsBaseVertex_indirect covers
2.916 sampled s, Mali osup_sync_object_wait 3.190 s. These overlap and must not
be added. Consistency across captures motivates a draw-path comparison; neither
wait nor unknown JIT symbols establish a final hardware limit.

## Version-pinned alternative

Exact phone Sodium inner JAR 0.8.12-beta.1+mc1.21.1 calls
GL32C.nglMultiDrawElementsBaseVertex(IJIJIJ)V from
GLRenderDevice$ImmediateDrawCommandList.multiDrawElementsBaseVertex. Its six
stock vertex/shader include sources do not contain DrawID or BaseInstance.
MobileGlues f56af31 source has the named indirect backend and an indirect command
buffer upload path. Installed native source provenance is not proved by a
matching symbol or version label; do not claim exact line attribution from it.

The separate opt-in experimental.directTerrainDraws defaults false. An optional
Pseudo mixin wraps only that Sodium call. Before any submission it requires
Android render thread, exact Sodium version, MobileGlues context, available
base-vertex entry point, loaded unobstructed world, no Iris/Oculus, triangle/
unsigned-int batch and bounded/nonnegative native counts/offsets. Contexts
exposing core 4.6 or ARB_shader_draw_parameters keep stock: per-subdraw DrawID
would differ in a scalar loop. Each accepted nonempty draw keeps the original
count, byte offset, base vertex, order, program, VAO, buffers and GL state. No
index rewrite, geometry reduction, texture change or worker GL. Zero-count draws
are skipped only in a context without shader draw parameters. Unknown versions,
batch shapes, owners or capabilities keep stock before any draw is submitted.

There is deliberately no exception/error fallback replay after a partially
submitted batch: that would duplicate translucent geometry. This prototype has
not demonstrated all native behavior or third-party callback equivalence. A
readiness/build gate is not a visual test. General defaults remain false.

## Probe and decision gate

Reuse the existing full-frame recorder with only wachiland-terrain-probe.txt and
diagnostics=true: 60 s warm-up, 60 s stock, 60 s direct, 60 s stock. Staging stays
on in all phases; Sodium render-ahead and visual settings are unchanged. Direct
flags are restored on completion, configuration disable or world/screen exit.
Legacy result limit numbers are compatibility placeholders in mode=terrain,
not a render-ahead change. Counters must show eligible/actual submissions before
the candidate can be interpreted. Do not combine this with other probe markers.

Tested artifact 0.2.2-terrain-probe SHA-256
c871a5fe1e100c390fe73e0aff0e24ba30d9ec82cd71ad0a834e0fffe92bd92c.
Build passes after correcting the LWJGL pointer-size constant to Pointer.
Physical world A/B/A completed with staging on, maxFps60, diagnostics on and
JFR/basic recording off. Full-frame warm-up/capture boundaries, not launcher time:
stock1 2901 frames / 59.994696619 s = 48.354274 FPS;
direct 3282 / 60.003657542 = 54.696666 FPS;
stock2 1850 / 60.013284619 = 30.826508 FPS.
The accepted route executed 39384 batches / 298662 draws. No obvious geometry
loss in the snowy creative scene, but translucent/custom shader/callback coverage
remains incomplete. World time moved from night into dawn: second-control drift
prevents assigning a precise effect size or general GPU benefit. Battery snapshots
34.3 to 35.1 C on USB do not prove long-term thermal sustainability.

Later explicit direct=true, diagnostics=false interval reached a 59-FPS screenshot
in daytime. Separate native capture: 14.9398 s, 113103 samples, 1430 userspace lost,
2494 truncated stacks; render owner TID 2434. On-CPU 604 samples / 12.3265 CPU s;
off-CPU 8520 / 2.0655 sampled s. Indirect draw wait no longer dominates that
interval, but this is not a controlled subtraction. Inclusive os::javaTimeNanos
2.551 CPU s and clock_gettime 2.429 CPU s overlap. Large unknown JIT/driver
components remain. This led to source verification of an empty Android GLFW wait,
not a conclusion that the GPU has reached its final limit.

Raw evidence stays outside Git in android-device-support-headroom-20261010 and
android-device-terrain-draw-20261011 under C:/BootOptimBench. BootOptim unchanged.
