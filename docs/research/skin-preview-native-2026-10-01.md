# Skin renderer follow-up — native resolution, 2026-10-01

Scope: skin_renderer.rs and component/player_model.rs only. Baseline already includes decoded-texture cache, conservative transparent-face culling, frontend opt-level=3 and worker-side BGRA conversion. No change to native resolution, animation cadence, geometry, shading values, texture coordinates, blending order or interpolation.

## Retained changes

1. Exact integer RGB shading lookup: one read-only 65,536-byte table shared by all previews. Formula unchanged: `(channel * shade) / 255` in u16, cast to u8.
2. Transparent samples exit before shading; nontransparent body pixels retain forced alpha255. Samples with alpha255 write directly, equivalent to image::Rgba::blend's existing fast path. Partial alpha continues through the unchanged library blend.
3. One reusable depth Vec per preview, owned by its single renderer task while rendering. Resize and fill(f64::MIN) happen before every valid frame. Buffer returns to state even when decode/frame rendering fails; dropping preview/task releases it. At452x768 it holds2,777,088bytes (~2.65MiB), versus allocating/freeing those bytes each frame. Frame color storage is not reused because GPUI owns displayed frames.

## Measurement origin and limits

Standalone CPU renderer on this fast Windows PC, rustc opt-level=3 linked to the launcher's existing image/schema dependency rlibs. Framebuffer452x768, identical inputs/angles/sway; timing starts immediately before loops of render_skin_textures and ends after them. Includes image/depth construction and rasterization; excludes GPUI composition, texture upload, UI notifications and PNG decode (both sides cache textures). Times are wall duration of CPU renderer work, not actual launcher FPS or end-to-end measurements. No laptop claim.

Synthetic fixtures: legacy/moderna64x32/64x64, solid/transparent/partially transparent texels, cape/no cape, Classic/Slim/Other, yaw/pitch, zoom0.5/1/4 with clipping. 576 candidate-versus-baseline frames pixel-identical.

## CPU evidence (milliseconds per frame)

Shader+sample paths, 3 repetitions x60 frames, median:

| Fixture | Baseline after face culling | Shader+sample paths |
|---|---:|---:|
| Transparent overlay |13.854|12.604|
| Opaque overlay |18.844|17.256|
| Partial alpha |29.923|26.474|

Depth reuse measured separately against shader+sample paths already enabled,5 repetitions x60 frames, median:

| Fixture | Allocate depth every frame | Reuse depth |
|---|---:|---:|
| Transparent overlay |12.289|11.925|
| Opaque overlay |17.175|16.043|
| Partial alpha |26.274|26.018|

The opaque depth test has one noisy/regressing repetition. Gains are modest; do not add both tables' deltas or claim launcher FPS gains. Separate microbenchmark1000 iterations at same size: allocation+initialization0.746ms, reuse+initialization0.111ms. This measures only the depth storage operation.

A raw-byte replacement for image pixel access was explored but not retained: timings were mixed/noisy and offered no clear additional gain versus simpler shading/sample changes. Reordering opaque faces or changing barycentric floating arithmetic was not done because it could change tie handling/blending/boundary pixels.

## Regression gates

- Exhaustive65,536 shade/channel values equal original integer formula.
- Reused depth matches fresh depth across repeated poses, grow/shrink, invalid zero-size frame and recovery.
- Existing cached texture equality, malformed dimensions, framebuffer bounds and transparent-face culling tests retained.
- Final full frontend compilation and visual/runtime check are owned by primary agent; no full launcher compile or UI launch performed by subagent.

Artifacts in this directory: baseline.rs, candidate.rs, baseline-shader.rs, raw-pixels.rs, shading-only.rs, bench.rs, bench-scratch.rs, memory.rs and executables. Primary agent should copy this evidence into durable launcher docs before committing the work.
