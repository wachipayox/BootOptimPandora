# Staging uploads in the crowded Android server, 2026-10-13

New premise: the earlier staging benefit was measured in a different scene.
The current JFR contains staging's native GPU copy calls; those samples alone
cannot establish that our own workaround regresses the crowded server.

Same JVM, APK0.1.16/support0.2.24, MobileGlues,2816MiB, retained G1/thermal mode,
fixed fan, dry safe glass scene. Existing manual one-shot frame agent, sampler
off. No Spark, heap walk or additional profiler. All other support flags and
EntityCulling/frustum retained. Each phase has15s request/transition settling,
then60s at stationary pose2837.57550251587,67,-536.5747864714097,
yaw9.643555/pitch-1.5691304. Metadata valid, no trace overflow, rain0 throughout.

| Phase | loops/s | p95 ms | p99 ms | max ms | >250ms frame wall |
| --- | ---: | ---: | ---: | ---: | ---: |
| staging ON first |7.086|241.832|321.324|420.592|9.697%|
| staging OFF |3.146|483.890|700.962|797.143|82.794%|
| staging ON return |9.648|153.157|204.490|261.990|0.868%|

Completed runTick loops are not GPU presentation FPS. Initial ON is still
warming relative to return ON; do not claim a precise3x universal speedup.
Both surrounding ON phases are substantially faster than the middle OFF.
This is clear net benefit evidence for this phone/server scene; no additional
repetition requested. Keep the existing staging option locally enabled.
It remains a capability/pixel-guarded opt-in for broader GPUs; these measurements
do not close its historical resource-reload or other-GPU gates.

Inclusive upload wall:0.700s/19606calls,18.982s/9999calls,0.851s/27257calls.
Client-tick wall29.991/43.514/30.607s; world-render wall19.759/9.098/19.696s.
These scopes overlap: never sum them as independent elapsed time. Different
completed-frame counts also change the number of upload/draw calls.
Return ON ~19.963client ticks/s,1200ticks in60.010s: ~25.506ms inclusive per tick;
world render~33.958ms per invocation. These averages identify substantial work
remaining, not a hardware-limit proof or a main-thread-only CPU duration.
No30FPS success claimed. Individual frames and per-second bins remain recorded.

All flags restore automatically after each phase. No player/camera/world state
changed by the benchmark. Normal disconnect/quit and restoration of ordinary
launch arguments are the final cleanup gate recorded below.

Cleanup passed: capture flags restored, normal disconnect/main-menu Quit Game
and EGLBridge termination observed. Complete game/JFR/GC files archived outside
Git. Instance javaArgs restored to the original ZipFS-only baseline; no manual
FPS/JFR/startup overrides remain. APK0.1.16 and support0.2.24 stay installed;
launcher reopened. Next ordinary launch uses automatic audited startup defaults.

Raw files in C:/BootOptimBench/android-device-server-fps-20261011:
live-staging-on-first-*, live-staging-off-*, live-staging-on-return-*.
