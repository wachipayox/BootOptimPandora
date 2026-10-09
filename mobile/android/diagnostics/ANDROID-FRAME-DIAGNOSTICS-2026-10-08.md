# Android frame diagnostic, 0.1.9 alpha

## Decision and scope

The user is collecting a Kerria-off Spark comparison and explicitly requested a
single diagnostic build covering more hypotheses to reduce APK/mod transfers.
This supersedes the earlier audit's temporary instruction to leave the APK alone;
the existing phone trial should still finish before installing this APK.
BootOptim, mods, pack contents, global profiles and graphics settings are untouched.
This is experimental diagnostic instrumentation, not a promoted FPS optimization.

Kerria does affect animated texture upload. The actual 0.1.8 run reported its fast
cache/PBO routes unsupported. Spark's inclusive Kerria wrapper time is underlying
upload work, not proof of Kerria overhead. See the adjacent Spark and mod audit.

## One APK, repeatable captures

In Diagnostico, `Investigar FPS en la proxima partida` is initially enabled for
this diagnostic version. Preparation consumes it once. Rearm it for another run;
no new APK or mod is needed. Thermal policy remains independently selectable.
Basic Android/JVM/thermal recording is enabled for the armed session even if its
ordinary preference was previously off. Deep JFR/full-GC attribution is deferred
without consuming its armed preference. Do not run Spark during frame captures.

Start a world and allow JEI to finish. A fresh log must contain the exact current
JVM origin property, a player-join marker and `Starting JEI took`. Then wait 20 s
to settle and capture 120 s. Fallback: 90 s after the first world render plus the
same 20 s settling interval. Wait about four minutes after world entry and JEI
completion, then exit normally and share the usual diagnostic ZIP. An early kill
preserves partial flushed files. Keep the same scene/camera/settings for A/B.

## Instrumentation

An optional Java 17+ agent stages separate agent and bootstrap-counter JARs.
The selected runtime must have lib/libinstrument.so; otherwise it is skipped.
The supplied ARM64 Java 17/21/25 archives contain that library. The agent uses
the runtime's internal ASM with explicit exports, no downloaded agent dependency.
It transforms a fixed class/method allowlist at initial load, not retransformation.
Methods preserve original arguments, return values and thrown exceptions. Named
modules get a read edge to bootstrap counters. Frame computation uses actual
loader types; unresolved hierarchy or verifier failure skips that class.
Missing hooks/counters remain explicitly unavailable, not zero-cost evidence.
Compatibility with the physical ModLauncher/LWJGL patched load chain is pending.

Counters measure Minecraft runTick/frame, client tick, TextureManager tick,
NativeImage upload, Java glTexSubImage2D wrappers, GLFW swap, world rendering,
ambient animateTick, Create placement helpers, AsyncParticles postTick and terrain
multi-draw wrappers. Native method declarations are never rewritten. Reentrant
calls of the same metric are counted once per thread. Actual runtime coverage is
reported in hooks/status files and nonzero call counts.

Measurements are cumulative call counts, inclusive wall nanoseconds, coarse fixed
histograms and upload pixels for the pointer glTexSubImage2D overload only. Metrics
overlap and MUST NOT be summed. Durations include native work, scheduling, GC and
driver waits; they are not GPU execution time or exclusive CPU. Frame count is
runTick count, not proof of distinct display presents. Histograms give upper-bound
buckets (1,2,4,8,16,33,66,125,250,500,1000,2000 ms and overflow), not exact quantiles.

Render/server thread CPU totals and two stacks of depth 20 are sampled every
500 ms. Metadata discovery (not all-thread stack walks) runs once per second.
Sampler wall cost is recorded separately. Counter hooks are active only during
the two-minute window; inactive calls remain a volatile guard, with first-world
time recorded once. No forced GC, heap dumps or GL calls from sampler threads.
The diagnostic itself can perturb timings; compare identical instrumented builds.

## Correlation, settings and files

Every run persists PID, origin, requested modes and a unique memory-origin-PID
prefix. The report lists current-prefix coverage rather than treating old files
as current. The ZIP keeps two older memory sessions plus the current one.
The missing telemetry in diagnostic (9) remains unproven; preparation/coverage
events and forced basic recording for armed captures now expose that failure.

Launch snapshots include allowlisted game options and Kerria/AsyncParticles
settings, including config/asyncparticles/asyncparticles.json. No full config or
arbitrary setting values are exported. Effective Kerria enabled/cache/fastUpload/
fastLightTextureUpload fields are read once per second from the already-loaded
mod, without calling a config getter or saving changes. Unknown fields stay unknown.
AsyncParticles snapshot is launch-only, not proof of dynamically changed values.

Existing Android/JVM/thermal streams run every 10 s; they retain their original
metric boundaries. Process memory categories overlap. Battery temperature is
not CPU/GPU temperature, unavailable thermal data is not evidence of a cool phone.

New prefix suffixes: performance-status.txt, hooks.txt, settings.json,
effective-settings.csv, request.txt, calls.csv, threads.csv and sampler.csv.
Text output is bounded around 2 MiB per agent file. The normal local sharing ZIP
already includes these CSV/JSON/TXT files. No automatic uploads or new permissions.

## Interpretation and validation gate

Compare upload calls/pixels/duration per second and per frame, texture/client tick
cost, swap/terrain durations, Render thread CPU delta versus elapsed capture time,
GC deltas and matched thermal/memory samples. This can separate candidate routes
and CPU consumption from long wall waits, but cannot identify exact GPU driver
internals. Effective Kerria state and coverage are prerequisites for attribution.

Compile/package and signature inspection are necessary; actual phone startup,
hook coverage, observer cost and diagnostic usefulness still require the user run.
No end-to-end/FPS claim is justified by a successful APK build. Disable the FPS
checkbox to restore the ordinary wrapper/thermal recording without the agent.

## 0.1.10 follow-up

The first armed0.1.9 attempt had only launcher_prepared and no JVM/agent output;
its next unarmed attempt reached the world and exited normally. The cause remains
unknown. See [diagnostic10 and hardening](VIVO-DIAGNOSTIC-10-2026-10-09.md).
0.1.10 uses a minimal premain and starts heavy agent resolution from MemoryMain
after basic telemetry exists, fixes settings JSON export and preserves prior
attempt logs/crash-page reports. The existing capture design/boundaries remain.
