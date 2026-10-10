# Android support: memory follow-up, 2026-10-10

## Origin and scope

Physical Vivo V2041/Y72 5G, Android 13, MobileGlues/Mali-G57 MC2, same enabled
Android pack and saved snowy creative world as the staging trial. APK 0.1.11,
support mod 0.2.1-pixel-validation SHA-256
bf1e73a8ec512f3bc99eea48c6e36c5f6cacc544c5b4d587fd06e293c17e8d3f.
Thermal mode and USB charging stayed on. Existing bounded Java attribution was
reused; no new runtime profiler and no BootOptim code change. Startup/world times
under JFR are not an optimization A/B and are not compared to control timings.

Menu marker was received at 23:23:00. The existing >=768 MiB available-memory
guard rejected the histogram/full-GC census. This protection was not bypassed.
JFR allocation/old-object samples still completed. Allocation traffic is not
retained heap; root traversal was disabled and no complete retained-owner graph
was collected.

## Findings

The last occupied-heap sample saved in-world used 2,956,439,504 bytes of a
3,221,225,472-byte maximum; G1 old occupancy was 2,907,587,208 bytes. Occupancy
does not identify owners or prove that every occupied object remains necessary.
At session elapsed 436.735 s: RSS 4,706,232 KiB, Swap 1,060,196 KiB,
MemAvailable 361,192 KiB. Android's available-memory estimate differs from that
kernel field; do not add them. Bionic native heap allocation was 892,811,536
bytes; Java nonheap/direct counters overlap other residency categories and
must not be added to RSS. Battery was 34.5 C under charging. No controlled
thermal improvement or root cause of the earlier reload failure is established.

Use only the latest old-object emission second, 23:26:26, rather than summing
repeated snapshots. Sampled shallow bytes included:

| Allocation site | Samples | Shallow MiB in those samples |
| --- | ---: | ---: |
| InputStream.readNBytes/readAllBytes -> ZipFileSystem.newByteChannel | 91 | 198.671 |
| ZIP central-directory initialization | 32 | 22.878 |
| Sites containing model/Model names | 15 | 13.551 |
| EntityCulling ArrayOcclusionCache | 1 | 4.000 |

These are sampled objects, not complete class totals or per-mod retained size.
The new ZIP-channel stacks truncate before a retaining root or full JarJar
caller. Earlier longer traces implicated nested JarJar backing storage; this
run corroborates long-lived ZIP-channel arrays, not their exclusive owner.
The sparse model sample does not account for the 2.9 GB old pool, nor does it
rule out a model-memory issue. Closing live ZIP filesystems or dropping model
caches is not justified by these samples.

## Existing work and decision

Read-only BootOptim integration was refreshed to
058ac544aef11c0c10dc32e0aeacba2e39176d40. Open diagnostic PRs
[244](https://github.com/wachipayox/BootOptim/pull/244) and
[247](https://github.com/wachipayox/BootOptim/pull/247), including the latter's
result comment, already establish the FML 4.0.43 virtual jij: contract and
recursive detection/selected materialization. They reject the copy+SHA sidecar
premise and live-object/duplicate-collapse caching. That is not evidence that
disk-backed byte storage would be equivalent or already integrated.

Do not duplicate their discovery profiler or revive the rejected cache. A
future byte-storage proposal needs version-pinned source/lifetime analysis,
fresh stock filesystem/readers/callbacks and resource identity/selection/error
equivalence; the mandatory BootOptim review must finish before implementation.
No portable memory optimization is implemented or claimed here.

Disposition: memory pressure remains a confirmed concurrent front; the owner of
most old-generation occupancy is unresolved. Staging's pixel check passed and
FPS premise is retained, but resource reload remains an unpassed gate. Next
bounded investigation should distinguish old/current resource generations and
GC/reclaim from upload stalls before changing cache lifetimes. Raw evidence is
local at C:/BootOptimBench/android-device-support-validation-20261010.
