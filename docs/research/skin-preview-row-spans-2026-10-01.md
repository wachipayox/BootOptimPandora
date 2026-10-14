# Native skin rasterization / GUI limits — 2026-10-01

Baseline includes frontendopt3, native resolution, decoded textures/alpha map, integer shade lookup, opaque/transparent sample paths and persistent depth scratch. New change only narrows each triangle's per-row pixel loop using conservative bounds; the original barycentric acceptance, UV interpolation, depth, shade, ordering and blending are unchanged.

## Structural cause

Pandora builds a native-size RGBA image on CPU every changed pose. Every triangle formerly visited its full bounding rectangle: even roughly half the rectangle lying outside a triangular face ran edge calculations. The new broad phase intersects each triangle with a two-pixel-high horizontal band, expands horizontally and retains the original rectangle on nonfinite/ambiguous geometry. Margins are for conservative exclusion only; no acceptance epsilon or visual approximation was introduced. This reduces outside-pixel work, not resolution or native sampling.

GUI currently has a deliberate33ms cadence in PlayerModelWidget and worker completion, so frame times below33ms cannot make animation exceed roughly30FPS. No FPS was measured here. Lower CPU times are resource savings; they should not be sold as higher GUI FPS. Changing cadence would need a separate CPU/UI budget and real-runtime measurement on both fast PC and weak laptop.

## Benchmark boundary

Standalone CPU rasterizer on the fast Windows PC, rustcopt-level3, identical native452x768 framebuffer and textures, persistent depth scratch on BOTH sides. Timing surrounds60 renderer calls, excluding decode, GPU upload, GUI composition, thread scheduling and presentation. Three repetitions, medianms/frame:

| Fixture | Before row span | With conservative row span |
|---|---:|---:|
| Transparent overlay |12.125|10.527|
| Opaque overlay |16.026|15.776|
| Partial alpha |26.622|24.367|

A separate randomized correctness run repeated the benchmark:12.111→10.364,15.975→14.279,25.722→24.561ms. Treat opaque-case performance as noisy; do not sum deltas across historical runs or claim laptop/hardware equivalence.

## Exactness gates

-576 matched frames across legacy/modern skins, cape/no cape, Classic/Slim/Other, transparent/opaque/partial alpha, yaw/pitch, zoom0.5/1/4 and clipping.
-1200 deterministic random pose/zoom/yoffset/frame-size samples matched pixel-for-pixel.
-New regression test verifies every pixel accepted by original edge formulas remains within optimized row span for both windings, fractional/near-vertical triangles, huge clipped triangles; nonfinite geometry fails open.
-Previous renderer/cache/alpha/depth/shading regression tests preserved.
-Full launcher compile and actual GUI check remain primary-agent gates.

## GPU finding and residual cost

Current vendored GPUI API exposes paint_image and paint_path on Windows, but no public textured3Dmesh/shared GPU surface. Window::paint_surface and SurfaceSource::Surface are cfg(target_os="macos") CoreVideo APIs. Window::paint_image places CPU bytes into sprite atlas. A GPU renderer therefore needs GPUI/platform integration or offscreen render/readback/upload, and float/raster/blend equivalence would need a new validation argument. It is not a safe small patch in assigned files.

Actual GPU upload/presentation latency, whole-widget layout, slider invalidation and global UI cost remain unmeasured. Advancing beyond CPU work should measure these boundaries; no unsupported claim that this patch solves user-perceived30Hz smoothness.

Files in this directory: baseline.rs, candidate.rs, bench.rs, fuzz.rs and executables. Primary agent should copy this compact evidence into durable launcher docs.
