# Zero-coalescing follow-up: offline audit

## Scope and authority

The phone is off. Only previously collected files were used: no ADB, new runtime
tests, device changes or BootOptim edits. Candidate baseline a759407f4 / draft
PR #88. See [physical run](VIVO-ATLAS-BATCH-ADB-2026-10-10.md).

Inputs: mapped Minecraft 1.21.1 / NeoForge 21.1.248 source and bytecode;
the previously pulled phone AsyncParticles JAR (SHA-256
051408b17049742b6bb69661a59082aa48861f927047b126e7e6facb84521fd0);
pinned AsyncParticles source 89317906; local Sodium inner mod
0.8.12-beta.1+mc1.21.1 (SHA-256
912a12199fe120a99b94e225dc08886671d2052386c6e92beb0146c587eab1e4).
The phone log names that Sodium version and inner artifact, but exact binary
identity and effective Sodium settings were not collected.

## Findings

### Multi-tick frames need not repeat visible-sprite uploads

Sodium SpriteContentsTickerMixin.preTick checks animateOnlyVisibleTextures and
sodium$isActive. Inactive sprites advance frame/subFrame but cancel normal
tickAndUpload. Its postTick clears the active flag. PerformanceSettings defaults
animateOnlyVisibleTextures=true. A sprite activated by rendering may therefore
upload on the first logical tick and then advance without uploading until the
next render. Our queue can have nothing left to eliminate in that case.

The physical run's zero replacements and one nonempty flush per eligible frame
are consistent with this explanation, not proof of effective phone settings or
absence of scope-ending guards. Do not disable visibility filtering to create
queue hits: that would add work and change the comparison premise.

### AsyncParticles's enabled deferral flag does not establish actual deferral

The exact phone MixinTextureManager queues only when BOTH deferredTextureTick
and isShouldTickParticles are true. AsyncTickBehavior initializes the latter
false; preTick returns before updating it when isTickAsync is false.
ConfigHelper defines isTickAsync as particleTickMode != SYNCHRONOUSLY. The
collected launch configuration is SYNCHRONOUSLY. Under that configuration, absent
a later runtime mode transition, the immediate original tick path applies.
Runtime mode transitions were not measured. The deferral flag alone cannot
explain uploads escaping the batching scope.

Mapped runTick calls advanceTime before runAllTasks. Its earlier progressTasks
drain is a different queue. A pre-timer texture-task drain is not established.

### Interpolated uploads remain uncovered

Stock InterpolationData mutates and uploads activeFrame images. The audited
Sodium implementation likewise mutates activeFrame and calls a SpriteContents
upload invoker. Our candidate accepts only sprite.byMipLevel; interpolated
images remain immediate. Retaining a Java reference alone does not make their
mutating native-buffer lifetime safe.

The new capture's 114 native-upload-top samples include 64 immediate
interpolated, 42 immediate discrete, seven replayed and one light texture.
The older capture was predominantly discrete. These unpaired, sampled results
do not prove interpolation CPU is the cause or that the candidate regressed it.
GL call wall time includes driver blocking and is not GPU execution time.

### Existing counters cannot distinguish no opportunity from guard fallback

Unknown atlas/sprite classes, binding/shape checks and lifecycle invalidation can
disable batching. NativeImage.close currently invalidates the scope for any
image, even one unrelated to pending entries. This is conservative but broad.
No reason counters identify these paths. Removing guards would be an unsupported
correctness change. Zero replacements may be correct if each sprite uploads
only once per frame.

## Next decision gate

Extend the existing bounded diagnostic, avoiding a second profiler:

1. Record effective Sodium visibility and runtime AsyncParticles tick mode plus
   observed atlas/sprite classes.
2. Count atlas visits, discrete/interpolated uploads and repeated keys per frame,
   including immediate uploads rejected from the queue.
3. Attribute first scope-ending/flush reason and aggregate class, binding,
   lifecycle, worker invalidation and render-consumption counts. No per-upload
   logging or repetitive stack dumps.
4. Correlate with existing GL/frame counters in the same measurement window.
   Do not sum overlapping durations or compare FPS across unmatched scenes.

If no repeated keys exist with visibility filtering enabled, retire the queue
candidate and investigate unavoidable upload/driver-stall cost. If repeat keys
exist but guards end the scope, fix the specific boundary with a lifetime and
ordering argument. If interpolation repeats, a future final-state design must
advance ticker state normally, materialize only the final visible state at a
validated render boundary, and preserve activation changes, callbacks, mipmaps
and custom-mod fallback. Do not blindly enqueue mutable interpolation buffers.

## Disposition

No production change or performance claim; PR #88 remains experimental.
8.09 versus 3.41 FPS is not paired A/B evidence. The zero-replacement queue has
not demonstrated a win. Phone access is unnecessary for this offline audit.
