# Small staging texture experiment

## Premise and scope

Physical measurements in VIVO-UPLOAD-ORDER-AND-NATIVE-WAIT-2026-10-10.md identify
the first atlas sub-upload in each frame and Mali sync waits as a large runtime
cost. Zero repeated keys rejected batching; render-ahead 3/1/3 was inconclusive
for benefit. This changes the native update architecture rather than retrying
those mechanisms. No BootOptim modifications, quality reduction, renderer
replacement or automatic mod removal.

The exact inspected MobileGlues release source is
f56af31d3e490d95808b3adc4ace20234249a37a. Its color glCopyTexSubImage2D forwards to
GLES; mg_fsr_read_scope_t redirects only when the tracked read FBO is zero.
Our private nonzero read FBO avoids that FSR path. Installed native build source
identity is not proven. Runtime pixel testing is therefore required. The audit
checkout also contains newer work; it was not silently treated as installed code.

## Implementation invariants and limitations

Experimental default off. Native GL invocation is wrapped inside Minecraft's
_texSubImage2D, preserving outer callbacks and measurement scopes. Exact vanilla
sprite/atlas scope, immediate CPU RGBA8 upload only; no worker GL or retained
pointers. Temporary small texture, read FBO, GPU copy to original texture ID and
mip coordinates. No duplicate full 8192x8192 atlas. See mod README for gates,
state restoration, probe, GL error consumption and unknown callback limitations.

Allocation/query overhead and driver-side lifetime of deleted in-flight textures
may defeat the premise. GPU copy may still synchronize with previous sampling.
The existing recorder is reused rather than introducing another frame profiler.
Instrumentation remains opt-in and the result must include both full-frame and
upload inclusive wall time. A moved stall alone is not an optimization.

## Validation status

Compile/package successful on JDK 21 / NeoForge 21.1.248. Physical trial prepared
on Vivo V2041, MobileGlues, thermal mode, USB charging, same saved world. Runtime
application, pixel self-test, reload, world correctness, A/B/A and native memory
results are pending. No cross-GPU or production validation claimed.

First physical launch caught a packaging error before game initialization:
the renamed mod metadata referenced a nonexistent renamed mixin JSON. Metadata
now references the existing packaged JSON. Compile alone did not catch this;
the failed launch is not a timing or performance result. Raw report retained in
the local trial directory, outside Git. The corrected trial is being repeated.
