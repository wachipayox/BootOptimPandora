# Vivo atlas attribution follow-up

## Origin and artifact

Physical ADB run on the same Vivo V2041 / Android 13, APK 0.1.11/code12,
MobileGlues, thermal mode, 3 GiB heap, distances 3/5 and mipmaps 2. Existing
New World (1), stationary view, USB charging. No Spark or deep heap attribution.
BootOptim was not edited. No resolution/pack/animation quality change.

Installed diagnostic artifact SHA-256
e7676fc3c8b271b6c8478e089e8e2a201a365af9020f33c16bda1e2bd542d789.
Its file is named 0.1.1-attribution, but it still contained the old 0.1.0 mod
metadata version; the metadata was fixed afterwards. Report artifact hash rather
than treating the metadata label as authoritative. All seven hooks applied.

Own observation window: first eligible in-world frame, bounded to 180 seconds,
aggregated logging every 30 seconds. Launcher FPS capture is a separate
post-world/JEI window: epoch 1791649860655 to 1791649981015, 120.360 seconds
(last counter reports elapsed 120.359). Never conflate these origins or totals.

## Exact-pack evidence

Phone Sodium inner mod SHA-256
912a12199fe120a99b94e225dc08886671d2052386c6e92beb0146c587eab1e4
matches the offline audited 0.8.12-beta.1+mc1.21.1 inner JAR byte for byte.
Stored config and runtime reflection both confirm animateOnlyVisibleTextures=true.
Runtime AsyncParticles reflection confirms tick_async=false, deferred=true.
This resolves the binary/setting uncertainty in the
[offline audit](ATLAS-BATCH-OFFLINE-AUDIT-2026-10-10.md).

## Results and disposition

Completed bounded observation: 1590 frames, 57648 atlas visits, 5068 original
sprite uploads (4345 discrete, 723 interpolated), **zero repeated
sprite/atlas/coordinate keys** within a frame, observation overflow zero.
The observer counts original immediate uploads even when batching is inactive;
it excludes replayed uploads to avoid counting its own queue as an opportunity.

Reasons: interpolation immediate 503, atlas outside batching 8128, upload batch
inactive 1812. No observed sprite/atlas-class, binding/shape, owner lifecycle,
worker invalidation or consume/upload flush reason. Outside-batch counts also
include legitimate single-tick frames; they do not establish a fault.

The independent FPS window contains 1179 frames (~9.796/s) and 65.957 seconds
inclusive GL texture upload wall time. These timings overlap other work and
are not GPU time. The comparison with earlier ~8.09 or ~3.41 FPS runs is not
a paired A/B and is not an optimization claim.

Sodium's visible-animation mechanism supplies a source-level explanation for
the lack of repeated upload keys. The measured scene offers no work for the
discrete batching queue to eliminate. Default batching is therefore disabled
in the next diagnostic artifact; no production promotion. Do not disable the
existing visibility optimization to manufacture opportunities.

Next bounded premise: source-verified Sodium cpuRenderAheadLimit uses a fence
queue and glClientWaitSync. A marker-opted, reversible same-session original/1/
original probe will distinguish a possible queue scheduling effect from the
rejected repeated-upload premise. No prior claim that this change improves FPS.

Raw logs, screenshots, pulled JARs and private preferences remain outside Git in
the local android-device-atlas-attribution-20261010 trial directory. The world
was saved and the game quit normally before replacing the diagnostic artifact.
