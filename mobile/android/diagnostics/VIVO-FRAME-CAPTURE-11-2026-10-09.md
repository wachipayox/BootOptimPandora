# Vivo diagnostic 11: first complete Android frame capture

## Origin, coverage and disposition

User supplied diagnostic (11) after installing APK 0.1.11/code12 and confirms the
early native crash no longer occurs. They did not perform the intended unarmed
0.1.10 retry. Do not treat archived previous attempts as new 0.1.11 evidence.
Local extraction: C:/BootOptimBench/android-diagnostic-11-20261009.

Latest session PID8913, telemetry origin 2026-10-09T20:41:58.445Z; exact origin
appears in its game log. Bootstrap, bootstrap-owned counters and agent readiness
all succeed. World join20:48:20Z, JEI completion20:48:51Z (21.83s logged).
Capture origin20:49:16.744Z, final counter row20:51:17.562Z, 120.818s elapsed;
capture_complete20:51:17.563Z. This is a post-world-entry window, not startup time.
Counters activate after setup, so rates are approximate window averages of
completed calls. Thread CPU interval separately spans119.946s.
World saves/exits normally; Android EXIT_SELF/status0 at20:53:12.181Z.

This is a physical acceptance result for avoiding the previous native bootstrap
append failure and obtaining a complete capture. It does not validate every
probe or promote any performance change.

## Actual settings

Vivo V2041, Android13, Java21.0.1-internal, MobileGlues, Xmx3072MiB, Xms512MiB,
thermal request enabled, sustained performance unsupported,60Hz surface vote.
Launch options: maxFPS30, VSync false, renderDistance3, simulationDistance5,
graphicsMode0, mipmapLevels2. These are launch snapshots, not live field probes.
Kerria is absent from the loaded-class probe; its leftover JSON says enabled=false.
No Kerria overhead inference is needed for this capture.
AsyncParticles snapshot: deferredTextureTick=true, animationTickMode=INTERRUPTIBLE,
particle tick/render modes SYNCHRONOUSLY, gpuAcceleration=true. Runtime stacks
confirm its TextureManager wrapper exists; snapshot is not proof of live changes.
Deep heap/JFR diagnostics are off; the armed FPS request forces basic collection.

## Measured frame work

| Metric | Completed calls | Inclusive wall seconds | Percentage of window |
| --- | ---: | ---: | ---: |
| Minecraft runTick/frame | 412 | 120.298 | 99.57% |
| Client tick | 2329 | 82.331 | 68.14% |
| Java glTexSubImage2D wrapper | 4188 | 57.111 | 47.27% |
| GameRenderer world rendering | 412 | 20.603 | 17.05% |
| AsyncParticles postTick | 2329 | 8.049 | 6.66% |
| Create placement helpers | 2330 | 2.969 | 2.46% |
| GLFW swap | 413 | 0.731 | 0.61% |

These are overlapping, inclusive call-wall durations and MUST NOT be summed.
GL wrappers aggregate instrumented calls across threads; they are not GPU timers.
The matching Render thread stack samples independently establish the dominant
upload path. At approximately3.410 runTick/s and3.418 swap/s, performance is
still poor despite a configured30FPS cap. This is not a controlled comparison
with the older user's subjective1FPS report. Mean completed frame291.985ms;
197/412 frames fall in(250,500]ms and43 in(500,1000]ms. Bucket bounds are not
precise percentiles. Client ticks average19.277/s, or5.65 ticks per frame.

Render thread:234 samples,119.946s elapsed,56.334s CPU (~47% of one core).
Server thread:69.084s CPU (~57.6% of one core) over that same interval. Selected
thread CPU is distinct from inclusive wall and total process CPU; no claim that
all cores are idle, or that the57s upload time is purely GPU execution.

Of234 Render thread stacks,112 (47.86%) have native GL11C.nglTexSubImage2D at
the top;111 come through SpriteContents.AnimatedTexture.uploadFrame and one
through interpolation. These stacks include the atlas/TextureManager/client-tick
chain. This confirms the animated-atlas upload front survives removal of Kerria.
GL32C native terrain draw appears in10 stacks; placement helpers in10; Xaero in3.
Sampling categories overlap; counts are not exclusive CPU fractions.

The GL call histogram has3869 calls<=1ms and308 calls>66ms (including two
in(1000,2000]ms). Mean13.637ms hides the expensive tail. Do not interpret all4188
calls as equally costly. Pixel total1188160 covers only the instrumented pointer
overload; other overloads are counted but their pixels are not. No total upload
bandwidth or texture size conclusion is valid from this partial pixel counter.

## Coverage gaps and observer cost

Hooks include Minecraft, GameRenderer, GL11C, GLFW, AsyncTickBehavior,
PlacementClient and GL32C. TextureManager, NativeImage and ClientLevel have no
hook-ready entry, and their counters are zero despite being visible in stacks.
Thus texture_tick/image_upload/ambient_tick are unavailable, not cost-free.
Terrain_draw is also zero despite native draws in stacks; the wrapper probe does
not cover the observed native entry route. Why these coverage gaps occur in this
ModLauncher/LWJGL loading chain is unproven; do not rely on those zeros.

Sampler234 iterations:2.243s summed sampler wall, mean9.584ms,max217.911ms.
This is observer-thread wall (includes waits/scheduling), not exclusive CPU or
measured extra frame cost. It is1.86% of window elapsed as a task sum and does
not explain the57s upload front by itself. Hook overhead has no uninstrumented
A/B measurement. Comparisons must use the same diagnostic configuration.

## Memory and temperature, matched to the capture

- Heap used2.736–2.823GiB of3GiB; old generation about2.703–2.711GiB.
  Nonheap490–504MiB. Recorded GC deltas14 collections/1.440s between the first
  and last in-window JVM samples; these are sampled boundaries and no full-GC
  attribution is present. Earlier accumulated GC time is not capture GC time.
- System available584–933MiB. Process VmRSS3.484–4.115GiB,
  VmSwap1.429–2.003GiB. Repeated foreground trim callbacks include levels5,10,15;
  these support memory pressure even though system_low_memory staysfalse and
  this session exits normally. Swap occupancy alone does not measure page-in
  latency, compression cost or prove that it caused each long GL call.
- Android graphics accounting reports roughly826–1082MiB in four detailed
  in-window samples. The vendor Debug.MemoryInfo PSS readings exceed proc RSS;
  retain them as reported and do not add them, subtract JVM heap to infer native
  ownership, or present them as an independent physical footprint.
- Battery35.7–36.6C during capture, entire run maximum36.9C, unplugged;
  Android thermal status remainsNONE. Headroom0.867–0.905, where1.0 is the
  SEVERE threshold per [Android's API contract](https://developer.android.com/reference/android/os/PowerManager#getThermalHeadroom(int)).
  Battery temperature is not CPU/GPU die temperature; NONE does not prove the
  OEM has no other frequency policy. No thermal shutdown occurred in this run.

## Next optimization premise, not yet implemented

Animated texture GPU submissions are the highest-value measured front. Low frame
rate leads to several client ticks before one visible frame. A candidate could
advance animation state on every logical tick but submit only the final dirty
atlas state before drawing. This could avoid updates the user never sees without
slowing animation time or reducing texture resolution.

Do not simply skip TextureManager ticks or only allow the last tick's existing
upload: a transition may happen on an earlier tick and not on the last. Preserve
dirty state, interpolate the final state, retain mipmaps and GL/render-thread
ownership; do not defer unsafe NativeImage pointers across close/reload. Unknown
mod draws/callbacks during ticks need stock fallback. AsyncParticles already wraps
TextureManager and can queue calls, so this interaction must be resolved first.
Pinned source audit89317906 shows it queues original::call when configured and
particle conditions permit; that is not by itself final-state upload coalescing.
The exact user JAR/source identity has not been established.

The MobileGlues2.0.0 source audit f56af31d3e490d95808b3adc4ace20234249a37a
wraps pixel conversion and GLES.glTexSubImage2D. Runtime upload format, actual
plugin binary identity and driver stalls remain unknown. Do not invent a
confirmed driver bug or blindly patch conversion from Java samples.

Memory pressure is a concurrent front. Quality-preserving coalescing, driver
attribution and cache residency are candidates, not proven savings. This turn
changes no launcher code, modpack or BootOptim; the user's earlier instruction
to leave BootOptim alone while another agent works there remains respected.
