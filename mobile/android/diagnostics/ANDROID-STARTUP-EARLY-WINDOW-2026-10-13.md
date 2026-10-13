# Android early-window and startup investigation, 2026-10-13

User requests using each launch for startup diagnosis and then stationary server
frame captures. Existing resources/final loading bar is acceptable; first red
period is not. Keep retained MobileGlues renderer and thermal mode. Fan is a fixed
physical test condition; temperature sampling verifies comparability, not a new
primary optimization front. No BootOptim source modifications authorized here.

## Authority and prior work

Refreshed origin/agent/integration-current: 4813ab19aec2a508034cd7c5e36410ee3723411a.
Read project AGENTS/README, research/catalog indexes, boot-pipeline program and
Mixin pipeline. Actual current source retains EarlyStartupProbeService,
StartupDiagnostics and scan-cache/discovery mechanisms. Open #145, #275 and #276
bodies/results inspected. Their detailed diagnostics/notes are NOT integrated
merely because a PR documents success. #276 hosted futures all finished before
scan barrier; don't blindly move the barrier or infer Android distribution from
that hosted result. No generic side-load cache/class prewarm rewrite is proposed.

## First same-JVM run

APK0.1.13, support0.2.17 explicit live probe, original Sable, 2816MiB/G1,
MobileGlues, thermal mode, fixed fan, basic collector on (deep/JFR off).
Initial failed attempt: Create RegisterEvent unbound chocolate_bucket, then
Xercapaint layers missing after broken mod-state rollback. Exclude failure from
startup benchmark. Retry reached official server and correct dry glass view.

Existing ModernFix launch-age marker reconstructs process origin to ~11:37:59
CEST on October13 (second-resolution logs; +/-1s, NOT launcher click time).
Coarse observations:

| Boundary | Process age approx |
| --- | ---: |
| ModLauncher main | 5.09s |
| ModernFix Bootstrap reached | 58.09s |
| Resource reload begins | 134.09s |
| Drippy resource overlay registered | 144.09s |
| Quickplay progress transition | 202.09s |
| JEI startup completes | 254.09s |
| In-world ready reported | 304.46s |

No observed title-screen endpoint: direct quickplay bypasses it. Resource-ready
surrogate, JEI preparation and server/chunk/world work remain separate; don't call
304.46s time-to-menu or sum overlapping resource listeners. Main coarse early
gate is ~134s before resource reload. A phase gap does not identify exclusive CPU,
disk, transformation or a particular mod. Basic instrumentation overhead exists.
No startup performance win established.

## Red-window finding

Screenshot live-kit-retry-startup.png confirms uniform red. FCL actual
DefaultLauncher.java adds default `-Dfml.earlyprogresswindow=false` unconditionally,
independent of renderer. User-supplied Java argument overrides this default
(CommandBuilder.addDefault checks prefix). Current phone fml.toml has
earlyWindowControl=true and earlyWindowProvider="drippy_early_window".
Phone official Drippy early module was pulled and bytecode checked: bootstrap
preserves existing provider, GL window requests3.2 compatibility, render thread
loads textures then draws/swaps until Minecraft handoff. Existing Drippy module
and regular Drippy3.1.2 are present; no missing MCEF error is attributed here.

This explains suppressed early progress provider; it does NOT yet prove the
whole red-surface behavior is caused solely by the argument or driver regression.
FCL's default predates this experiment. Test the existing module with explicit
earlyprogresswindow=true on a future launch, preserving renderer and config with
rollback. Inspect actual early log/screenshot and Minecraft handoff; never promote
to a global Android default just because a screenshot is improved.

The provider loop relies on glfwSwapInterval(1), with no explicit Java frame cap.
Whether that swap blocks on the Android bridge is a separate CPU/thermal gate;
do not assume desktop vsync semantics. If needed, fix the user-owned early module
in its own isolated source worktree with documented semantic/visual gates.

Next launch: reuse existing bounded JFR settings for early-only owner samples,
duration capped before world FPS windows; enable only existing lightweight
startup report. No duplicate full profiler or live heap walks. Keep diagnostic
startup timing separate from clean startup A/B claims.

Raw markers: C:/BootOptimBench/android-device-server-fps-20261011/live-startup-markers.json.


## Current disposition: own Android addon, external Drippy unchanged

The user explicitly rejected a Drippy fork. The official installed early module
and regular Drippy remain byte-for-byte unchanged. Source is metadata-only on
upstream main; this implementation copies no Drippy code, textures or layouts.
Our support mod contains its own nested early-agent and JDK-only bootstrap bridge.
Launcher0.1.14 stages those components and initializes the agent before Mio/
NeoForge startup, independently of performance/basic memory recording. Android
Support0.2.19 includes the component. No BootOptim source was changed.

Actual installed MobileGlues ARM64 code was extracted and disassembled:
glBegin, glMatrixMode, glOrtho, glColor4f and glVertexPointer are no-op returns.
Explicit earlyprogresswindow=true alone still cannot draw this fixed-function
provider. The adapter rewrites only52 audited GL11 call sites in the exact provider
class SHA256 12bc661aed5dbce06c2907eaba9709714298bcde7d8edb67a580b12740d6edf3.
Unknown bytes/call coverage leave the provider unchanged. Its original progress,
layout and texture ownership remain authoritative. Only a MobileGlues GL_VERSION
selects the GLSL150/VAO/VBO path; other renderer implementations delegate legacy.

The first direct system-loader reference failed at the SERVICE module boundary
(NoClassDefFoundError), so its startup timing is invalid as a performance win.
The corrected version uses a pre-startup bootstrap facade and renderer loaded
with the exact Drippy LWJGL parent. No runtime bootstrap append. Draw work and
cleanup stay on the provider GL owner thread; owned native stream is96KiB,
released with GL objects at Minecraft handoff. A50ms frame period caps early UI
at20FPS without reducing game FPS. Shader/driver failures retain original route;
unknown matrix/text layouts log a bounded bridge failure rather than aborting
Minecraft. Broader font/layout/other-GPU visual gates remain outstanding.

Physical adapter2 test displayed logo/progress/watermark, logged52 replacements,
core drawing enabled and handoff after1128frames, and joined the server. It used
manual startup instrumentation. In the180s JFR windows, observed Drippy thread
machine-normalized CPU fraction mean changed0.0946395(73samples) to
0.00811855(62samples); maxima and coverage differ. These conditional sample means
are supporting evidence for reduced loop activity, NOT exact integrated CPU or
an end-to-end startup speedup. Native samples include waits. Process-to-server
and reload/task-sum boundaries must remain separate.

APK0.1.14 and support0.2.19 automatic test: memory_enabled=false,
performance_next=false, deep attribution off; instance custom Java arguments
restored to ZipFS-only baseline. G1/thermal profile,2816MiB,MobileGlues, fixed fan,
server and safe glass pose preserved. Automatic52-call/core-init markers and
visible early UI confirmed. Final handoff/server outcome recorded below once
available. Existing account/signing identity retained; APK installed with -r.

## Adjacent compatibility error, separate from FPS claim

Adapter2 in-world capture had a CullThread NullPointerException in Nowheel1.0.6
properAABB while EntityCulling1.10.4 dereferenced null bb. Its7.0825loops/s and
p99397.898ms window is not a clean comparison. No adapter
world-FPS win/regression inferred. New support0.2.19 wraps only setupAABB and
recovers that exact exception stack/message. It returns a shared oversized finite
box so EntityCulling's EXISTING size guard explicitly keeps that block entity
visible and completes its pass. Null would be unsafe: block entity caller has no
null guard. Other exceptions/valid boxes stay unchanged. Exact mod-version and
Android guards; config compat.nowheelNullableBounds=false disables it. A physical
error-trigger/recovery gate remains outstanding until observed.

Raw evidence lives in C:/BootOptimBench/android-device-server-fps-20261011:
phone-mobileglues-legacy-functions.txt, adapter2-display.png,
adapter2-complete-game.log, adapter2-early-cpu.json, adapter2-startup-analysis.json.


Automatic physical outcome: APK0.1.14/support0.2.19 reached the same stationary
server glass pose. Handoff logged1149frames with no bridge/core failure. No
performance agent/JFR was requested; sampler explicitly logged disabled. This
proves ordinary launch activation independently of diagnostic collection.
Screenshot auto-adapter-world.png confirms world rendering. The early logo was
cropped in this automatic run, while adapter2-display had full logo; preserve
this unresolved visual gate rather than claim complete layout equivalence.
Nowheel recovery did not trigger in the observed automatic log; installation/
world load passed, but this does not validate the exact recovery branch.
ModernFix reported194.203s versus earlier diagnostic launches; this is NOT a
clean startup A/B (instrumentation, caches and background differ), and is not
claimed as a speedup. No in-world FPS improvement is claimed by this change.


## Automatic route and logo fit, support0.2.20 / launcher0.1.15

Default earlyWindow.mode=auto is now passed from config by the launcher. Runtime
GL context chooses modern drawing for MobileGlues or a real desktop core profile
(which excludes legacy drawing). Other contexts retain the original calls. No
GPU-vendor heuristic. earlyWindow.mode=legacy keeps FCL stock; modern explicitly
forces the modern path for unusual drivers. experimental.earlyWindowCompatibility
=false disables the addon. No property needs to be added by users for auto.

The pinned provider's computeUiScale result is clamped only on the modern route
so the logo plus symmetric horizontal offset fits92% of window width. Natural
texture width is used when config doesn't specify width. The original aspect,
logo, progress and positions remain; background isn't rescaled. Unknown inputs
keep scale. earlyWindow.fitLogo=false overrides fitting. Fit2 early screenshot
confirms full logo/bar with margins, no crop. Log records854px viewport,861px
natural logo and scale1 ->0.9104056. Handoff succeeded in the preceding fit run.

First fit FPS capture was INVALID: diagnostic launch missed asm.commons and
asm.util exports. All performance transforms skipped with IllegalAccessError;
zero frames cannot be a valid performance result despite unchanged scene. Fixed
existing probe exports for fit2; no new profiler or Spark run. Capture uses
same dry glass position/frustum/culling, manual windows after world readiness.
