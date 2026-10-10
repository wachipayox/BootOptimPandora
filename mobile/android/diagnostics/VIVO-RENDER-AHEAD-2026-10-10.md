# Vivo same-session render-ahead probe

Marker-opted Android-only diagnostic 0.1.2, SHA-256
e253a10283fbc07c8852d8ae3f6e4fdbe5f1487215cbdb9d577e8d408b987f39.
Same APK 0.1.11, existing world and stationary scene, MobileGlues, 3 GiB heap,
thermal mode, graphics and packs unchanged, USB charging. Batching was disabled.
Launcher FPS sampling/deep attribution were disabled for the entire run; own
frame and managed upload timers had identical overhead across phases.

Source authority: exact phone Sodium inner JAR. MinecraftMixin injects GPU-fence
waiting at runTick HEAD and inserts a fence at RETURN. It waits while its fence
queue exceeds cpuRenderAheadLimit. The probe wraps the full runTick, including
that wait, so shifting waits cannot masquerade as an end-to-end win.

Measurement origin: first eligible world frame epoch 1791650857755. Initial
60-second warm-up excluded. Each subsequent phase measures its own elapsed
monotonic window and completed frame count, without launcher/startup time.
The original limit 3 is changed only in memory, never written to config.

| Phase | Limit | Seconds | Frames | Frames/s | GL calls | GL wall s | Pixels |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Warm-up (excluded) | 3 | 60.128 | 216 | 3.592 | 2287 | 28.475 | 466240 |
| Control A | 3 | 59.959 | 385 | 6.421 | 3915 | 35.225 | 812592 |
| Candidate B | 1 | 59.954 | 389 | 6.488 | 4038 | 37.566 | 841488 |
| Control A again | 3 | 60.013 | 382 | 6.365 | 3942 | 37.564 | 817632 |

All phase results completed. Normal completion restored original=3 at epoch
1791651097810. Battery snapshot 36.2 C shortly after world entry and 36.5 C after
the probe; battery temperature is not die temperature or proof of no throttling.
No new crash observed during the probe.

Candidate versus average of the two controls is only about +1.5%. This single
physical sequence does not establish a meaningful performance win; world time,
thermal scheduling and phase order remain sources of variation. Keep original
limit 3. Do not promote or infer a benefit for other GPU vendors.

GL wall durations are inclusive and overlap full-frame time, not GPU execution
time. The small pixel volume does not identify the source of the long native
call wall time. Next premise: bounded per-sprite/atlas/mipmap upload attribution
to distinguish actual data/conversion cost from waits affecting a specific
atlas or its first update. No blind animation disabling or quality reduction.

Raw log, battery/thermal snapshots and per-phase JSON remain outside Git under
android-device-render-ahead-20261010. Remove the local probe marker before the
next diagnostic or ordinary use.
