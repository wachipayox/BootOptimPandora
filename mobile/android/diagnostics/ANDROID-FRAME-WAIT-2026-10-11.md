# Android frame deadline wait (experimental)

## Confirmed source mechanism

Minecraft 1.21.1 RenderSystem.limitDisplayFPS computes lastDrawTime + 1/cap,
then loops over glfwGetTime and glfwWaitEventsTimeout(remaining). The Android
engine LWJGL 3.3.3 and 3.4.1 implementations of glfwWaitEventsTimeout are empty;
the historical Thread.sleep body is commented out. A native capture of the
post-terrain-candidate world attributes 2.551 inclusive CPU seconds out of
14.9398 sampled wall seconds to os::javaTimeNanos. clock_gettime overlaps and
must not be added. This confirms a plausible unnecessary busy wait, not the
ownership of every unknown JIT frame or a promise of increased capped FPS.

## Isolated implementation

0.2.3-frame-wait-probe adds experimental.frameWait=false by default. Optional
RenderSystem mixin wraps only the existing GLFW timeout call. Before enabling
sleep it requires Android, enabled flag, render thread, finite positive timeout,
and bytecode proof from the runtime GLFW class resource that the static (D)V
method contains exactly one RETURN instruction and no other executable opcode.
A real wait, unknown resource or failed verification retains stock behavior.
This preserves vanilla's deadline, lastDrawTime, clock and call ordering. Parks
are bounded to the smaller of the requested remainder and 2 ms. Spurious wakeups
return to the original clock check; interruption remains untouched. The original
GLFW call still executes. No workers, GL calls, frame cap change, input event
filtering, world modification or quality reduction. Runtime config reload is the
kill switch. Short parks may overshoot deadlines under scheduler pressure; frame
pacing and input responsiveness require a physical gate, not merely compilation.

Artifact SHA-256 7a36daba3a90804457c6484e16ac1254fa25adfa46849ea64d6776088aa9c965.
Compile/package passed. Physical startup and world passed. Runtime status
verified_empty_android_wait, waits=3513 confirms the actual class resource gate.
Staging/direct on throughout; diagnostics/JFR/basic off, maxFps60, same camera.
Native render owner TID16074, separate 49Hz task-clock:u/offcpu 15-second intervals:
stock1 14.9128 s, 128986 samples, 344 userspace lost, 2651 truncated; render CPU
12.285714 s / offCPU 2.300689 s, inclusive javaTimeNanos 1.327 s.
candidate1 14.9178 s, 130525 samples, 4932 lost (4900 userspace), 2299 truncated;
render CPU 9.857143 s / offCPU 4.369954 s. Sample loss limits precision.
stock2 14.9398 s, 109867 samples, zero lost, 79 truncated; render CPU 13.040816 s /
offCPU 1.334302 s, inclusive javaTimeNanos 2.653 s.
candidate2 14.9708 s, 135617 samples, zero lost/truncated, larger 64MiB/512-page
buffers; render CPU 9.081633 s / offCPU 5.522518 s, inclusive Unsafe_Park 2.761 s.
Clocks leave the top CPU list in both candidate captures. These inclusive entries
overlap. Intervals include scene/time/GC drift, so do not assign a precise FPS
speedup or thermal decrease from CPU samples. Candidate screenshot 55 FPS;
control screenshot 50 FPS is contextual, not a comparable timed frame census.
No obvious input/menu breakage; deadline distribution and wider Android coverage
remain open. Default remains false pending those gates.

0.2.4-jit-attribution retains the same mechanisms and adds explicitly invoked
/wachilandandroid jitmap to export DiagnosticCommand compilerCodelist, and
/wachilandandroid testfpscap <10..120> to change only the session frame cap.
Neither runs automatically. The compiled-code export runs outside timed captures,
does not request GC and has a failure message on unsupported JVMs. The cap test
must restore the user's original cap afterward. This fresh attribution premise
addresses unknown JIT cost after the resolved clock waste, not another startup
profiler or an inferred GPU hardware limit. Artifact SHA-256
2be106c9d9234a97442058740c654f2f22fd09a5b09009a5ae0700aeb32a798f;
compile passed; physical startup/world and both explicit commands passed.
The export contained 27811 compiled-method ranges (~4.95 MB). Its invocation
is excluded from timings and its elapsed cost was not measured; do not treat
this medium-impact JVM operation as harmless during gameplay or benchmarks.

## Remaining cost / display ceiling

Vivo display reports only 1080x2408 at 60 Hz. Explicit session cap120 showed a
67-FPS screenshot; this is a ceiling probe, not a timed average or visible
67-Hz output. No quality setting was changed. Corresponding separate native
capture 14.9006 s, 153579 samples, 2442 truncated, zero lost; render TID24524,
12.0 sampled CPU s and 2.682775 sampled offCPU s. Compiled-code addresses from
the immediately preceding same-JVM export resolve part of the formerly unknown
Java. Code-cache turnover/inline attribution remains a limitation.
Exclusive leaf CPU categories (non-overlapping, sum=12.0 s): mapped Java 5.591837,
Mali driver 3.183673, MobileGlues 0.510204, other native 1.0, JVM 0.265306,
unresolved 1.448980. Driver inclusive durations are larger and overlap.
Mapped Java leaf groups: net.minecraft.client 1.857143,
net.minecraft.world 0.897959, com.mojang.blaze3d 0.183673,
net.neoforged.bus 0.183673, org.lwjgl.opengl 0.183673,
com.simibubi.create 0.163265, net.caffeinemc.mods 0.122449.
No single resolved Java method or optimization mod dominates this capture;
inline work and unresolved addresses prevent declaring every mod exonerated.
Do not subtract this uncapped/different-world-time capture from capped controls.

Disposition: retain isolated opt-in candidates, no forced GPU-wide activation,
no production promotion while resource reload/translucency/cross-GPU gates are
open. This reference scene is near the visible display ceiling after the large
software bottlenecks; there is no evidence of an absolute hardware limit in
all scenes. Prefer stable <=60 FPS with lower CPU waste over running extra
unpresented frames. Final same-JVM cap60 census, diagnostics enabled only for the existing bounded
full-frame recorder: 60 s warm-up, 120 s capture, 6818 complete frames /
119739531824 ns = 56.940259 FPS. Origin/end are runTick wrapper boundaries,
not launcher/world-loading time. No JFR/basic memory recorder; same camera,
quality and world, later world time. Do not claim precise improvement against
noncontemporaneous 52.519-FPS baseline. Battery snapshot 34.7 C on USB at end,
not sustained thermal proof. Diagnostic instrumentation disabled after capture;
original cap30 restored and world saved normally. Local reference phone keeps
explicit stagedUploads/directTerrainDraws/frameWait=true; general defaults stay
false. 0.2.4 is installed, not a new launcher APK. Remaining native/JIT uncertainty,
resource reload and cross-GPU equivalence are reopening gates, not waived checks. This is a separate
Android-support candidate; BootOptim source and integration are untouched.
Raw evidence is local android-device-frame-wait-20261011 under C:/BootOptimBench.
