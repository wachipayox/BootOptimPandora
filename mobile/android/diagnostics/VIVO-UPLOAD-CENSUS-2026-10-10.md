# Vivo per-sprite upload census

Diagnostic 0.1.3-upload-census, SHA-256
ce4c2d3bb8d8df07e9d8d6676a999e480d5cc179d64a9934e7555d3278fbb7b1.
Same APK 0.1.11/code12, MobileGlues, thermal mode, 3 GiB heap, existing
stationary world, unchanged graphics/packs and USB charging. Batching and the
launcher FPS/deep-heap agents were disabled. Sodium visibility remained enabled;
AsyncParticles runtime tick_async=false. No render-ahead marker; original limit 3.

Origin: first eligible world frame epoch 1791651921256; 60 seconds warm-up.
Capture starts at 1791651981348 and completes at 1791652101643. Completed 606
frames; inclusive frame wall 120.269126229 s. This is not a startup measurement.

| Sprite | GL calls | GL wall s | Pixels | Maximum single call ms |
| --- | ---: | ---: | ---: | ---: |
| minecraft:block/water_flow | 1803 | 68.742811 | 807744 | 451.149 |
| minecraft:block/lava_still | 933 | 0.022234 | 104496 | 2.498 |
| create:block/factory_panel_connections_animated | 1803 | 0.191517 | 201936 | 83.991 |
| create:block/saw_reversed | 1803 | 0.042580 | 201936 | 2.578 |
| Outside SpriteContents upload scope | 30 | 0.769382 | 7680 | 51.710 |

All four named sprites belong to minecraft:textures/atlas/blocks.png. Each named
sprite uploads mip levels 0, 1 and 2 equally often. Water accounts for about 98.5%
of the measured native upload call wall time. GL durations overlap frame wall;
they are not GPU execution time or CPU self-time. The total water pixel count
is only about 3.08 MiB at four bytes/pixel across two minutes, so raw upload
bandwidth alone is not a persuasive explanation. Synchronization, driver work,
memory pressure and scheduling remain hypotheses. Do not label the water sprite
broken or disable its animation based on these timers.

Startup reports a blocks atlas of 8192x8192x2. RGBA8 levels 0, 1 and 2 imply a
theoretical 336 MiB allocation, not an observed GPU allocation or process RSS.
Changing immutable storage/texture identity across reloads requires a separate
correctness argument; no such production change is made here.

## System trace limitations

A 20-second sched/freq/idle/gfx/thermal Perfetto request used a 16 MiB buffer.
Only 5.960670327 seconds remained in the ring. Existing process metadata is
incomplete and the Minecraft render thread cannot be reliably identified by
the process-name filter. Do not treat Android HWUI RenderThread as Minecraft's
Render thread. kswapd0 and G1 concurrent threads are active in the retained
interval, but their activity is insufficient to assign the upload stalls to
memory pressure. No valid per-Minecraft-render-thread CPU/wait result yet.

Next diagnostic 0.1.4 records the Linux owner TID once from /proc/thread-self/stat,
per-mipmap wall time and calls/wall of the first managed upload in each frame.
This observes order without changing ticker order, texture contents, mipmaps or
animation. A larger system trace buffer must retain process metadata and the
full selected interval. No performance improvement is claimed by instrumentation.

World saved and game exited normally after this run. Raw logs, preferences,
trace and battery/thermal snapshots stay outside Git under
android-device-upload-census-20261010. No BootOptim source was modified.
