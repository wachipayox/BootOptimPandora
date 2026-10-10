# Android support 0.2.4: fresh basic memory baseline

## Conditions and measurement boundaries

Physical Vivo Y72 / Android 13 / Mali G57 MC2 / MobileGlues 2.0.0.
Support 0.2.4-jit-attribution, SHA-256
`2be106c9d9234a97442058740c654f2f22fd09a5b09009a5ae0700aeb32a798f`.
Staged uploads, direct terrain draws and frame wait enabled explicitly;
diagnostic render probes disabled. Existing launcher basic memory sampler only:
no Spark, JFR, heap histogram, heap dump or requested full GC.
Java max heap 3 GiB, original 30 FPS cap, render distance 3, simulation distance 5,
thermal mode retained. No resource reload or quality changes.

Local raw evidence: `C:/BootOptimBench/android-device-memory-basic-20261011`.
Session `memory-1791673594552-6511`; session origin is the existing memory
wrapper, not launcher setup. Main menu reached after the logged ModernFix
startup duration of 218.008 seconds. Existing snowy creative world entered at
01:11:59 game-log time. World saved and game quit normally afterwards.

## Point measurements

| Metric | Menu | In-world |
| --- | ---: | ---: |
| JVM session elapsed | 232.003 s | 402.090 s |
| Java heap used | 2,840,529,584 B | 3,052,195,296 B |
| Java heap max | 3,221,225,472 B | 3,221,225,472 B |
| G1 old pool used | 2,579,881,008 B | 2,909,692,504 B |
| Cumulative GC time | 9,838 ms | 16,242 ms |
| Android session elapsed | 233.972 s | 407.417 s |
| Process RSS | 5,455,464 KiB | 4,608,376 KiB |
| Process swap | 148,976 KiB | 1,187,024 KiB |
| Kernel MemAvailable | 325,512 KiB | 405,568 KiB |
| Android available-memory estimate | 387,710,976 B | 603,365,376 B |
| Bionic native heap allocated | 901,788,688 B | 904,541,664 B |

JVM and Android samples are close in time, not simultaneous. These categories
overlap and must not be summed. RSS dropping while swap rises is not evidence
that resources were freed. Android's available estimate and kernel
MemAvailable are different readings. G1 old occupancy is not a retained-object
census: a basic sample does not prove all occupied objects remain reachable.

At the in-world screenshot the original cap was being met (29 displayed FPS).
Thermal sample at 400.850 s reported battery 34.3 C, thermal status NONE and
USB charging. This is a bounded point measurement, not a thermal endurance run.

## Decision and next attribution gate

Memory is the next priority: the in-world heap is approximately 2.84/3 GiB,
process swap approximately 1.13 GiB, and system headroom remains small.
The run did not crash, but does not establish resource-reload safety.
Do not increase the heap based only on the phone's advertised 8 GiB RAM.
Do not force a guarded histogram under these conditions.

Keep the existing heap attribution tool rather than adding a duplicate
profiler. Prior rootless JFR allocation samples remain hints, not full retained
owners. No model cache, ZIP filesystem or NativeImage was freed in this run.

Source audit found vanilla TextureAtlas retains SpriteContents until
clearTextureData, which closes their images. Static CPU image/mipmap ownership
is a candidate for bounded attribution, not an approved release mechanism:
custom sprite pixel reads, generated mipmaps and subsequent uploads must be
audited before releasing anything. The roughly 0.84 GiB Bionic allocation
does not prove those images own all native memory; GPU driver allocations and
other native owners need separate attribution.

BootOptim integration was refreshed to
`058ac544aef11c0c10dc32e0aeacba2e39176d40`. No BootOptim source changed.
PRs 244/247 forbid casually collapsing live nested ZIP owners, and retired
Ferrite retention/presize experiments 310/317 must not be revived as safe RAM
optimizations. No RAM saving or production promotion is claimed here.
