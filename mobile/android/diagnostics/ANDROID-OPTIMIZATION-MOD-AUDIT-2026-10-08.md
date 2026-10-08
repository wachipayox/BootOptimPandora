# Android optimization-mod audit: Kerria and desktop assumptions (2026-10-08)

## Scope and evidence

User observed improved performance after disabling Kerria's in-game option during
one live world session. They will supply a new Spark profile without Kerria.
This is a useful within-session observation, not yet a controlled quantified A/B.
Do not change the pack, APK or BootOptim while that comparison is pending.

Read-only clones under C:/BootOptimBench:
- Kerria tag 1.3.1: 694f33d78405cb7df8f416fd58e20f3788eca078.
- MobileGlues 2.0.0 release source marker f56af31d3e490d95808b3adc4ace20234249a37a.
- AcceleratedRendering version bump b9d3d59 (1.0.10.1), same version as game log.
- AsyncParticles history 89317906 declares 21.1.0b-beta.3, matching log version.
  Multiple commits declare that version; exact APK/pack JAR/source byte identity
  was not established. Findings involving that source are hypotheses to confirm.

Primary sources:
- https://github.com/decce6/Kerria/tree/694f33d78405cb7df8f416fd58e20f3788eca078
- https://github.com/MobileGL-Dev/MobileGlues/tree/f56af31d3e490d95808b3adc4ace20234249a37a
- https://github.com/Argon4W/AcceleratedRendering/commit/b9d3d59
- https://github.com/Harveykang/AsyncParticles/commit/89317906
- https://github.com/decce6/Kerria/issues/20

## Kerria: what actually runs in diagnostic (9)

Log reports Fast Texture Upload unsupported: OpenGL45=false,
DirectStateAccess=false, BufferStorage=true. Animated Texture Cache unsupported:
OpenGL45=false, DirectStateAccess=false, CopyImage=false, TextureStorage=false.

GlCapacityChecker / Kerria.shouldUseCache and shouldUseFastUpload gate the GPU
cache and persistent-buffer upload routes. Therefore this context does NOT execute
those purportedly faster algorithms through Kerria's normal NativeImage path.
NativeImage still calls the original CPU-pointer glTexSubImage2D upload.
Do not claim Kerria's GPU copy/PBO algorithm is slower here: it is unavailable.

Mixins remain applied (plugin shouldApplyMixin always true):
- AnimatedTexture.uploadFrame WrapMethod changes nesting counters and calls original.
- InterpolationData changes upload nesting counters.
- NativeImage._upload condition checks thread/config/pixels and feature gates.
- Texture binds/active units are tracked with integer state and a 32-entry array.
- GlStateManager._texImage2D converts unsized GL_RGBA to GL_RGBA8 unconditionally.
- NativeImage fields for PBO/cache remain, but GPU objects are lazy and feature gated.
These hooks add potential overhead with no enabled fast-route benefit, but no
measurement establishes that their overhead explains a large FPS collapse.

Spark (9) Kerria upload wrapper inclusive ~15.35%, self zero sampled weight.
The inclusive time is the underlying upload work, not Kerria's own measured CPU.
The combined audit scan of Kerria/AcceleratedRendering-related hooks has only
~0.225% self weight (includes some Sodium state tracking); do not overstate it.
Native texture upload leaf ~17.78%; Java sampling cannot separate CPU conversion,
driver synchronization, GPU execution or scheduler pauses.

## Why live toggling matters

The global enabled checkbox binds Kerria.config.enabled without a resource reload.
It bypasses extra checks in NativeImage._upload, but does not remove mixins,
nesting-counter wrappers, bind tracking or the unconditional RGBA8 rewrite.
Thus the texture-format rewrite alone cannot explain an immediate on/off change
in the same session, since already-created textures and the rewrite remain.

MobileGlues release source handles RGBA and RGBA8 through the same format/type
normalization branch (RGBA / unsigned byte), though their internalFormat is still
passed onward; this does not prove identical driver storage or timing. Format
interaction is a weaker hypothesis here, not a confirmed defect.

If next result shows a large reproducible gain, investigate actual effective
config/loaded JAR, interactions with upload wrappers and repeated live on/off
captures before asserting a general mobile algorithm regression. Camera, world
activity, thermal state, GC and sampler overhead remain confounders.

## Broader patterns worth pursuing

1. Optimization loaded while its fast path is unavailable. Kerria fits this.
   AcceleratedRendering also reports unsupported in the actual log. Its
   AvailabilityUtils explicitly rejects renderer names containing MobileGlues,
   gl4es or LTW, besides checking desktop GL extensions. CoreFeature.isLoaded
   returns false; many hooks remain as guarded fallbacks. This proves absent
   intended benefit, not a large measured fallback cost.
2. GPU offload/driver synchronization assumptions. A supported desktop operation
   may translate into extra copies, fences or driver work on GLES/mobile. API
   capability is a correctness check, not a guarantee of faster execution.
   Test individual features, including upload/copy/compute paths, rather than
   blanket-disabling all optimization mods or forcing unsupported extensions.
3. Asynchronous work falling back to the render thread. AsyncParticles' matching
   version history postTick has a !ConfigHelper.isTickAsync branch that runs
   endTickOperations.forEach(Runnable::run). Spark (9)'s render-thread postTick ->
   ArrayList.forEach -> DefaultEndTickOperation -> animateTick resembles that
   synchronous path. Confirm effective config and byte identity before conclusion.
   Parallel mode uses futures/task arrays; extra scheduling can itself cost CPU
   and heat, so enabling it blindly is not justified.
4. Cache tradeoffs. Extra resident images/meshes, allocation and reduced memory
   headroom can negate CPU wins in this pack. This is a separate memory/thermal
   mechanism, not proof that desktop Java algorithms intrinsically lose on ARM.

## Decision gate

Keep current comparison limited to Kerria. Compare same scene/settings/thermal
mode and actual FPS, plus sampled upload frequency/weight and client tick stacks.
Percentages alone cannot prove improvement because their denominator changes.
Need fresh matching telemetry (diagnostic (9) lacks it) before attributing heat,
GC or memory changes. No new repeated hardware run is requested before this
already-running comparison is reviewed. No code, tests or production changes.

Open/closed Kerria issues reviewed: #20 OpenGL ES anecdote lacks substantiated
measurements; #9/#10 historical XyCraft/Voxy cases evolved across 1.1-1.3 and
cannot be applied to current unsupported cache/PBO routes; #13 old memory issue
was reported fixed in 1.2; #15 launch/exit delay attributed to another mod's
network timeout; #21 is an unresolved mod interaction. They guide investigation,
not proof of this phone's cause.

## Subsequent user-authorized diagnostic

The user later requested a consolidated APK diagnostic while their Spark trial
continues. See [frame diagnostic design](ANDROID-FRAME-DIAGNOSTICS-2026-10-08.md).
This adds experimental launcher instrumentation, not a pack/mod/BootOptim change
or a measured performance fix. Existing trial results remain independently useful.
