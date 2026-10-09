# Vivo Spark without Kerria, 2026-10-09

## Evidence and comparability

User supplied https://spark.lucko.me/YAYb4OBfRU while preparing an independent
0.1.10 diagnostic. Binary downloaded from the viewer's public bytebin host using
curl, decoded with the existing Spark protobuf reader/schema. Local files:
C:/BootOptimBench/YAYb4OBfRU.sparkprofile and spark-kerria-off-decoded.json.
No new profiler or mod/APK change made for this analysis; private metadata omitted.

New recording:2026-10-09T20:14:55.917Z to20:15:58.566Z,62.649s.
Baseline:2026-10-07T22:00:19.485Z to22:01:21.413Z,61.928s.
Both NeoForge21.1.248/Minecraft1.21.1, built-in Java sampler, interval10000us,
all threads by pool, execution mode. Render nominal sample weight26230ms versus
26710ms baseline is much less than either recording duration; it is NOT complete
CPU/frame/wall time. Sampling delays/safepoints and profiler overhead are unknown.
Neither profile supplies client FPS or per-frame/tick/upload call counts.

Declared mod version maps differ only by removal of Kerria1.3.1+1.21.1-neoforge
(238 entries baseline,237 now). Kerria hooks are absent from new render stacks.
This is stronger evidence than toggling a live enabled option, but does not prove
byte identity, equal config/camera/temperature/world state. Profile windows have
6-8 entities versus20-22 baseline,77 chunks in both. This is not a controlled
performance A/B. Await current-session0.1.10 telemetry; do not reuse older samples.

## Render sample attribution

Inclusive shares of each recording's own render root; overlaps MUST NOT be summed.
Named NativeImage.upload appears in nested overloads: summing them double counts
the same upload. Prefer the single native leaf rather than its misleading42.24% sum.

| Route | Baseline | Without Kerria |
|---|---:|---:|
| Minecraft.tick |58.70%|55.05%|
| TextureManager.tick |19.84%|25.20%|
| TextureAtlas.cycleAnimationFrames |19.66%|25.09%|
| GL11C.nglTexSubImage2D native leaf |17.78%|21.12%|
| GameRenderer.render |21.38%|26.57%|
| ClientLevel.doAnimateTick |11.27%|13.27%|
| AsyncParticles.postTick |12.92%|13.76%|
| Create PlacementClient.checkHelpers |5.95%|4.54%|
| GL32C.nglMultiDrawElementsBaseVertex |3.56%|5.41%|

New render self leaves: native texture upload21.12%, terrain native draw5.41%,
ClientChunkCache.getChunk4.69%, atlas animation loop3.43%, ambient block tick2.25%.
Texture uploads remain prominent without Kerria. This rejects treating the old
Kerria wrapper's inclusive time as its own overhead or blaming Kerria alone for
the remaining rendering cost. It does NOT show that removing Kerria slowed FPS;
relative sample shares change when other work changes and no FPS is present.

New chunk meshing worker parked98.67%, entity-culling thread sleeping94.47%.
No evidence these workers continuously saturate during the sampled interval.
Server Sable native Rapier step is9.91% of its own root (baseline~4.2%); changed
world state makes that a separate candidate, not a causal client-FPS verdict.

## Memory/server metadata

Heap at profile end2936.63/3072MiB (95.59%); baseline2895.44MiB (94.25%).
Nonheap508.33MiB, baseline501.46MiB. GC counters are lifetime totals, not capture
deltas; one historical full GC averaging5.391s is not proof it occurred in this
minute. System memory/swap is whole-system, not attributable to Minecraft alone.
Matched Android/JVM/thermal time series are needed for pressure/GC/heat attribution.

Server tick count1063, baseline1066; last-minute TPS17.55 versus~17.5-17.7.
New mean MSPT27.31, median10.10, p9596.97, max1043.64ms. These are server metrics,
not rendered client FPS. Spikes cannot be assigned to a mod using this alone.

## Concrete next premise: repeated texture updates before presentation

Read-only local NeoForge/Minecraft1.21.1 decompilation in the prior resource-reload
audit shows Minecraft.runTick calling advanceTime, then up to min(10,i) client
ticks before rendering. Minecraft.tick calls TextureManager.tick when the level
runs normally. New Spark confirms TextureManager.tick's parent chain is precisely
Minecraft.tick -> runTick -> run. SpriteContents.Ticker advances frame/subFrame
and uploads changed frames or interpolation each tick.

Hypothesis: low FPS batches several logical animation ticks before one rendered
frame, uploading intermediate atlas states never presented. Reducing GPU upload
frequency independently of animation-clock progression may remove mobile driver
work without reducing visible animation quality. This mechanism is not measured
yet:0.1.10 counters already capture texture ticks/uploads/pixels per frame plus
wall durations, selected CPU, GC and thermal state. No extra transfer requested.

Correctness traps before any production implementation:
- Do not skip animation-clock advancement or slow animation speed.
- Skipping all but the last tick's upload is insufficient: a frame transition can
  happen on an earlier tick and no upload occurs on the last tick. Flush the final
  dirty state for that animation instead.
- Queuing raw NativeImage pointers risks mutated interpolation data, close/reload
  lifetimes and freed memory. Require generation ownership/invalidation.
- GL work remains on the render thread, with the same atlas binding/mipmap state.
- Mod callbacks may render offscreen or consume atlas contents during ticks;
  preserve those observations or fail open for unknown paths.
- Validate animated/interpolated sprites, pauses, reloads and representative world
  rendering; source inspection and build success cannot prove visual equivalence.

No BootOptim/mod/server code changed and no FPS gain claimed. Wait for0.1.10
coverage before implementing a batch/coalescing experiment or changing renderer.
