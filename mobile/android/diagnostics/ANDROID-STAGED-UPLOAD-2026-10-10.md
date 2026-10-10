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

## Corrected physical A/B/A result

Source ffc2756c5; tested packaged JAR SHA-256
ead1195d5eb377639dc5c2ea0ae1cdd3ba577912bd2494ea3f648eceb750d938.
All seven hooks applied. MobileGlues reports Mali-G57 MC2 / GLES 3.2, desktop
interface 4.0.0, framebuffer/copy available, immutable storage/image copy absent.
Small-pattern RGBA byte readback at destination mip 1 and nonzero offset passed.

Origin is the existing full Minecraft runTick wrapper in an unobstructed loaded
world, not launcher preparation or process-start time. Same world, saved camera,
mods, renderer and charging state. Start epoch 1791656496041; first 60.205 s are
warm-up and excluded from the comparison. Each measured phase has its own
begin/end boundary and full-frame count. No GPU-time interpretation of inclusive
native wall durations; they are inside frame wall, never added to it.

| Phase | Elapsed s | Frames | FPS | Inclusive upload wall s | Native upload calls |
| --- | ---: | ---: | ---: | ---: | ---: |
| Stock 1 | 59.911 | 373 | 6.226 | 36.077 | 3792 |
| Staging | 59.912 | 1733 | 28.926 | 3.427 | 12468 |
| Stock 2 | 60.039 | 541 | 9.011 | 35.169 | 5070 |

Staging completed 12453 uploads with zero fallback; 15 other native uploads kept
stock. Probe completed and restored original false flag. Both surrounding stock
phases are substantially slower; control drift prevents an exact general speedup
claim. Scene and visibility effects remain a limitation. No texture/animation
setting was reduced. Stationary terrain screenshot showed no obvious corruption,
but that is not exhaustive animated-sprite or cross-GPU validation.

Phase-entry process RSS: 3480296 / 3506780 / 3547500 KiB; Swap: 1433324 / 1397032 /
1381428 KiB; system MemAvailable: 432412 / 502252 / 455992 KiB. These are snapshots,
not peak memory or GPU allocation measurements. Battery temperature 35.1 / 35.7 /
35.8 C under USB charging. No controlled thermal improvement is demonstrated.

Afterward config was reloaded to diagnostics=false, stagedUploads=true; the same
scene displayed about 30 FPS. F3+T reload was explicitly requested at 20:27:45.
Its resource preparation logged four corrupt/unknown-image decode failures and
missing model warnings, before atlas upload. Reload completion/correctness is
still pending; do not promote a production mechanism or claim reload equivalence
from the successful short FPS trial. Root cause of the decode failures has not
been assigned. The initial launch did not report those four decode failures.

Raw evidence and analysis JSON remain outside Git at
C:/BootOptimBench/android-device-support-staging-20261010. No user logs or phone
preferences are committed. This is a successful candidate premise, not a merged
production release. General defaults remain off for the unvalidated mechanism.
