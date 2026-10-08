# Vivo Android: client Spark profile, 2026-10-08

## Evidence and scope

User supplied Spark https://spark.lucko.me/7xc6Xteq12 and diagnostic (9).
No code, APK, modpack or BootOptim changes made for this investigation.
Profile binary was downloaded from the viewer's configured public bytebin host,
and decoded using the public Spark protobuf schema. Raw data and analysis scripts
are local in C:/BootOptimBench; user identifiers and full JVM arguments are not
copied into this durable record.

Profile interval: 2026-10-07 22:00:19.485Z to 22:01:21.413Z (61.928 seconds).
Client NeoForge 21.1.248 / Minecraft 1.21.1, built-in Java sampler, nominal 10 ms,
all threads grouped by pool. User intended thermal mode on. The sampler reports
1066 integrated-server ticks; these are not rendered frames or client FPS.
JEI finished at 21:58:24Z, before this capture. Discovered mod filenames differ
from diagnostic (8) only by addition of Spark 1.10.124; byte/config equivalence
is not established.

## Confirmed diagnostic mismatch

Log and Android exit report match game PID 28685, launch 21:50:52.396Z.
Normal shutdown log and Android EXIT_SELF/status 0 at 22:02:43.312Z; no new LMK.
ZIP contains telemetry for PID 28185 (brief aborted earlier session) and PID 5823
(previous reload LMK). It contains no memory/thermal series for PID 28685.
Do not correlate old temperature/CPU/RSS samples with this Spark capture.
The reason the launcher failed to retain these new samples remains uninvestigated.

## Sampled render-thread attribution

Render root weight: 26710 ms equivalent / 2671 nominal samples. This is less
than the 61928 ms recording boundary. Never treat these weights as exact elapsed
CPU, GPU or complete frame time. Sampling delays, safepoints and profiler cost
are not attributed by this data; all-thread Java sampling can disturb the run.

- TextureManager.tick 19.84%, TextureAtlas.cycleAnimationFrames 19.66%.
- Animated sprite tick/upload 17.90%; GL11C.nglTexSubImage2D leaf 17.78%.
- Non-interpolated animation upload branch 15.35%, interpolation branch 2.55%.
  Kerria wrapper is on the upload path; presence does not establish causation.
- AsyncParticles post-tick work 12.92%, mostly deferred animateTick/doAnimateTick
  for ambient block/fluid/biome work (~11.7%). Wrapper attribution is not proof
  that removing AsyncParticles removes the underlying vanilla work.
- Create/Ponder placement helper tick ~6.0%, chiefly matchesItem/checkHelpers;
  this is not an active Ponder scene or its old shader issue.
- GameRenderer.render 21.38%, terrain multi-draw native leaf 3.56%.
- Client entity ticks 12.24%, much of that local-player ticking.
- FancyMenu is not a dominant render-thread sampled stack. Initial preloader
  again records one panorama, six suppliers, zero slideshows.

Inclusive percentages overlap along stacks; do not sum them. Native upload
samples can represent conversion, driver work or synchronization in MobileGlues;
Java sampling cannot separate these mechanisms or GPU execution time.

## Other threads and metadata

Chunk meshing executor is parked ~97.5% of its samples; no evidence of a meshing
worker being continuously saturated during this steady-world capture.
Entity culling thread sleeps ~93.8% of its samples.
Integrated server reports ~17.5-17.7 TPS, median MSPT ~13.8, mean ~28.2,
max ~884 ms. Its entity tick stacks dominate its own samples, but cannot explain
client 1 FPS by themselves. World statistics report ~20-22 entities / 77 chunks.
These TPS values are not client FPS. Server native Sable physics leaf ~4.2%.

Profile-end Java heap ~2895.4 MiB / 3072 MiB, nonheap ~501.5 MiB.
System swap used ~3891.7 MiB is whole-system, not per-game memory.
GC metadata is lifetime cumulative, not a profile-interval delta; it cannot prove
or disprove a GC storm in the captured minute. No matching telemetry exists.
Async-profiler native loading fails with Android namespace restriction, then Spark
uses built-in Java successfully; this warning is not a game crash.

## Next decision

Strongest concrete graphics candidate: per-tick animated atlas uploads and their
MobileGlues/native cost. Isolate with a reversible animated-texture A/B or narrow
upload instrumentation before choosing renderer changes or a permanent patch.
Also inspect client ambient tick and repeated placement helper work if this first
premise is insufficient. Preserve animation/visual semantics for production;
turning animations off is an experiment, not a proposed quality reduction.
No single root cause or promised FPS multiplier has been established.

## Subsequent user-authorized diagnostic

The user later requested a consolidated APK diagnostic while their Spark trial
continues. See [frame diagnostic design](ANDROID-FRAME-DIAGNOSTICS-2026-10-08.md).
This adds experimental launcher instrumentation, not a pack/mod/BootOptim change
or a measured performance fix. Existing trial results remain independently useful.
