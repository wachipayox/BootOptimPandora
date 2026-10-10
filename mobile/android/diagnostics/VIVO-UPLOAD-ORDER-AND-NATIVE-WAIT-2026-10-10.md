# Vivo upload order, system pressure and native waits

Follow-up to [the sprite census](VIVO-UPLOAD-CENSUS-2026-10-10.md).
Diagnostic 0.1.4-upload-order SHA-256:
9f4601f479e8db645d7d1191f6e1882a5a3f74b42b31afe1dd519d004731ffd8.
Build passed. All seven hooks applied; menu, existing-world entry and bounded
capture completed without a new crash. Same APK 0.1.11/code12, MobileGlues,
thermal mode, 3 GiB heap, unchanged graphics/packs and USB charging. No quality
or animation changes. Batching off, render-ahead limit 3, no probe marker.

## Managed upload census

Origin first eligible world frame 1791653010984; Linux TID 16071, Java name
Render thread. Capture begins 1791653071013 after 60 seconds warm-up and ends
1791653191086. 779 completed frames, 120.045124013 s inclusive frame wall
(about 6.489 completed frames/s). This is an observation, not a comparable A/B
performance gain over previous runs.

| Sprite | Upload calls | GL wall s | Level 0 wall s | First-in-frame calls |
| --- | ---: | ---: | ---: | ---: |
| water_flow | 2265 | 70.624294 | 70.545564 | 755 |
| lava_still | 1092 | 0.026707 | 0.017547 | 0 |
| factory_panel_connections_animated | 2265 | 0.147778 | 0.042582 | 0 |
| saw_reversed | 2265 | 0.053554 | 0.033828 | 0 |
| Outside sprite scope | 31 | 1.120375 | 1.120375 | 0 |

Water's 755 first-in-frame calls are exactly its 755 level-0 uploads. These
accumulate 70.545563862 s, about 99.89% of that sprite's native upload wall.
Later mipmaps and other sprites are inexpensive. This proves the observed
order/cost concentration, not that water's pixels themselves are defective.
No animation, mipmap or ticker was skipped/reordered by the diagnostic.

## System trace: separate interval

20-second Perfetto sched/freq/idle/gfx/thermal request, 64 MiB buffer.
Retained bounds 161831987320087 to 161852078757499 (20.091437412 s), no reported
data-loss counters. The explicit owner TID maps to Linux Thread-10, not HWUI
RenderThread. Perfetto assigns that TID to two internal thread records; join by
the known TID for this stable run rather than discarding samples with missing
or inconsistent process ownership metadata.

Owner thread-state aggregates: Running 11.783695061 s, interruptible sleep
7.791243464 s, runnable 0.148955175 s, uninterruptible non-IO wait 0.050361535 s;
some short state records have no classified state. These cannot individually
be assigned to a GL call without corresponding call markers. App C2 compiler
and server thread were also busy (10.948 and 9.185 scheduled CPU seconds in
the trace); summed thread CPU can exceed elapsed time on multiple cores.

Before/after system snapshots cover roughly 24.1 seconds, slightly more than
the trace. Global deltas: pswpin 81681 pages, pswpout 100583 pages, pgmajfault
82890, workingset_refault 40446, allocstall_normal 52, allocstall_movable 119.
kswapd0 consumed about 3.487 CPU seconds in the trace. Page-byte conversions
must use the device's page size; these are system-wide counters, not solely
Minecraft events. App smaps rollup before: RSS 3179376 KiB, Swap 1721404 KiB;
after: RSS 3123612 KiB, Swap 1787536 KiB. System MemAvailable ~568 to 574 MiB.
This is concrete memory pressure/reclaim evidence. RSS plus swapped bytes is
not a precise total GPU allocation and must not be equated with Java heap use.

Battery after trace 38.5 C on USB. Thermal service reports status 0 and repeated
CPU/GPU/NPU proxy temperatures; this does not prove absence of GPU throttling.
Temperature reduction is not demonstrated by this experiment.

## Native sampling after the census

The installed Android NDK simpleperf tool profiled the debuggable app without
root, restarting it or recompiling ART code. CPU-only sample: task-clock:u,
99 Hz, ~14.899 s, 3013 whole-app samples, none lost. Filtering owner TID gives
297 samples (~3.0 sampled CPU seconds); 66.3% are unresolved JIT/native mappings.
This cannot establish a native CPU-copy bottleneck.

A second ~14.936 s recording adds sched_switch/off-CPU call stacks, same 99 Hz
CPU rate. Whole app: 89562 samples, 215 kernel-space samples lost, no user-space
sample loss. This interval is later than the census and Perfetto; do not combine
their totals as one timeline or use its overhead for an FPS benchmark.

Owner off-CPU attribution totals about 7.333 sampled seconds. Inclusive stacks:

| Call path | Off-CPU sampled s | Share of owner off-CPU sample time |
| --- | ---: | ---: |
| MobileGlues / Mali glTexSubImage2D | 5.262964079 | 71.77% |
| Mali osup_sync_object_wait | 5.572794688 | 75.99% |
| JVM SafepointSynchronize::block | 0.449774466 | 6.13% |
| MobileGlues multidraw indirect | 0.624645227 | 8.52% |

Rows overlap: never sum them. The driver sync wait, condition wait and futex
are nested in the upload stacks. This confirms a major driver synchronization
wait in texture sub-upload; it is not just an inference from Java stack tops.
Sample loss, short duration, unknown JIT mappings and proprietary driver frames
limit exact attribution. It does not prove that changing renderer is sufficient
or that memory pressure is unrelated.

## Next implementation premise

Stop reviving duplicate upload coalescing (zero repeats) or claiming render-ahead
1 is faster (A/B/A gave only ~1.5%). Do not disable water or lower texture quality.
The bounded candidate worth testing is a small staging texture plus GPU-side
copy into the atlas, so CPU client-memory upload does not directly update an
atlas still consumed by queued draws. Keep atlas identity, dimensions, mipmaps,
pixel data and tick order. Never duplicate the full 336 MiB atlas under current
memory pressure. No such optimization has been implemented/promoted yet.

Compatibility gate: MobileGlues release source f56af31 forwards color
glCopyTexSubImage2D to GLES and contains FSR read-framebuffer redirection. Native
phone binary build ID is 87df65c883a2fc85229763afc46a4d7714eaa866; source identity
is not yet proved. A prototype must verify exact read/draw framebuffer binding,
active texture/binding, pack/unpack/PBO state, RGBA8 eligibility, no sRGB/format
conversion, no sampling feedback loop, context lifetime and a stock fallback.
glCopyImageSubData support cannot be assumed. Small staging resource recycling
could itself reintroduce the wait; measure full-frame behavior and memory, not
only a shifted timer. Visual and resource-reload checks are mandatory before
promotion, as are tests on other GPU vendors.

All raw recordings remain local under android-device-upload-order-20261010.
Diagnostic-only source and documentation remain on draft PR #88; no BootOptim
or production launcher change and no performance win claimed.
