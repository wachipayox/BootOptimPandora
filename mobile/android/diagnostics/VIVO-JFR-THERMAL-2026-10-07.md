# Vivo 0.1.7 — usable JFR, OEM stop during world creation

## Evidence and boundaries

User supplied `wachiland-android-diagnostic (4).zip`, reporting a temperature
warning/termination at roughly 3% world creation while charging. They replaced
FancyMenu with the controlled main fork, which selects only the active background.
They explicitly explained missing MCEF as the randomly chosen background requiring
an unavailable component; do not classify that exception as the termination cause.

ZIP valid, 16 flat entries, 4104067 compressed bytes. Current session is
`memory-1791399704754-18338`, Vivo V2041 Android13, APK0.1.7/code8, MobileGlues,
effective `-Xms512m -Xmx3072m -XX:ActiveProcessorCount=8`.
Process marker 2026-10-07T19:01:44.369Z; memory origin .754Z.
ModernFix menu proxy 19:12:13Z (`Game took 627.372 seconds to start`).
Spawn preparation starts19:15:33Z; game log ends19:15:44Z with repeated2% status.
The user-visible3% need not have been flushed to the file. No successful world
entry is established. This run is diagnostic, thermally affected, and has a
different pack/menu outcome from earlier runs: not a valid performance A/B.

Android matched PID18338 foreground importance100 at19:15:45.300Z:
`USER_REQUESTED (10), status=0`, description
`stop net.wachiland.launcher due to stop by com.vivo.pem`.
No JVM completion callback. The record does not say LOW_MEMORY or CRASH.
The user's thermal warning plus OEM power-manager stop are consistent with a
thermal intervention; no temperature/thermal-status/charging telemetry was
included by0.1.7, so the reason cannot independently prove a thermal threshold.
Do not interpret USER_REQUESTED as the human necessarily pressing force-stop.

## Capture survival / runtime capability

- 81 Android samples through19:15:42.138Z, 3.162s before stop.
- 84 JVM samples through19:15:41.060Z, 4.240s before stop.
- Complete JFR15584594bytes, parse/stream read successful. Latest dump finished
  19:15:34.700Z,10.600s before stop. Rolling retained events range
  **19:10:57.192310135Z–19:15:34.350435152Z**, roughly277s. Earlier allocation
  traffic was evicted; old samples can refer to objects created earlier.
- JFR contains4199 allocation samples,2124 old-object sample events and53GC
  events. It also includes114 Minecraft ServerTickTime events published by the
  game. Presence is not a CPU execution profile or a complete heap graph.
- JFR initialization1918ms.28dump completions sum11768ms background inclusive
  operation duration, max1123ms; these are not measured critical-path overhead.
- Basic Android operation sum28400ms, max3286ms. No claim of negligible overhead.
- Menu request received19:12:24.620Z, but census skipped by the fresh >=768MiB
  Android headroom guard. Nearby sample available754.5MiB, lowMemory=false.
  **No occupied/live histogram and no diagnostic fullGC were performed**.
  Runtime capability for JFR is now physically demonstrated; histogram support
  and full live census remain unvalidated. Existing guard should not be weakened
  merely to obtain a histogram from a pressured phone.

An abrupt OEM stop truncated the observation window but did not corrupt the
completed snapshots. The remaining limitation is the skipped census/rootless
sampling, not destruction of the entire diagnostic.

## Memory observations

Final JVM occupied2776.6MiB, committed/max3072MiB, old-pool2680.0MiB,
nonheap453.5MiB, direct14.9MiB. Occupancy remains high and is not a proof of leak,
complete live set or exact model ownership.

Final procRSS3859.3MiB, VmSwap1624.0MiB, available835.4MiB,
Bionic native allocated882.8MiB, ART occupied11.9MiB.
Maximum sampledRSS4313.6MiB; peakVmSwap2564.9MiB; minimum available422.8MiB.
95callback/events rows. Last detailed graphics sample at19:15:21.110Z reports
772.8MiB; it is not simultaneous/additive with final counters. OEM PSS disagrees
with procRSS as before; no exact process decomposition is established.

Previous0.1.6 failed run ended with native1857.4MiB and available268MiB.
Latest native counter is about974.6MiB lower, consistent with removing eager
background decoding. These differing stage/system/instrumentation conditions
do not establish a controlled saving or permanent resolution of memory pressure.

Exact log proves installed `fancymenu-3.9.0-welite-main-neoforge-1.21.1.jar`:
`Layout preload kept 0 background sources, added 0 from active layouts, skipped
22 configured background sources`; parallel suppliers0. Selected group0 is
`bg_arbol_carton`, the MCEF-dependent source the user described. This verifies
the filter and that the old all-background path did not run; it does not validate
visible panorama/video rendering or all possible randomly selected backgrounds.

## Java attribution findings, not production savings

Streaming analysis separates sampled allocated traffic from old sampled objects.
The last retained277second window estimates9.20GiB allocation traffic; it is
not9.20GiB simultaneously retained/resident memory.

### Long-lived nested-JAR buffers: strongest retained-object candidate

Last old-object emission (~19:15:34Z) includes98`byte[]` samples whose creation
stacks pass through JarJar PathFileSystem mounting, with **204.66MiB summed
shallow objectSize**.27other ZIP-read byte-array samples account for24.62MiB.
Nested-buffer sample ages reach802.2seconds. These are actual sampled arrays,
not a claim that all are collectible or that this is the total subsystem heap.
Rootless sampling is not a complete post-fullGC live census.

Representative stack:
`InputStream.readAllBytes -> ZipFileSystem.newByteChannel -> UnionFileSystem.
newReadByteChannel -> Files.newByteChannel -> ZipFileSystem.<init> -> JarJar
PathFileSystem -> JarContents -> JarInJarDependencyLocator`.
This points at nested archive backing storage rather than voxel models. Public
OpenJDK21u ZipFileSystem constructor retains its opened seekable channel for
archive random access. Source interpretation is corroborating mechanism evidence,
not proof that the phone's patched internal JRE is byte-identical to current21u.

Next bounded premise: identify exact nested archives, duplicate mounts and open
channel lifetimes; consider read-only disk-backed extracted storage only if it
preserves JarJar identity, dependency selection, resource lookup, multi-release
semantics and invalidation. Do not close channels still used by class loaders
or decompress every mod indiscriminately. No implementation/heap patch yet.

All540unique old samples across the entire retained window were also grouped,
but summing their bytes is **not** a simultaneous-live total: samples disappeared
between dumps. Repeated2124events must not be counted as2124different objects.
The latest emission, not historical aggregate, is used for the figures above.

### Voxel/Palladium traffic: candidate transient work, not retained GiB

Allocation stacks in the retained window estimate964.11MiB through Palladium
FastBitSet/LongOpenHashSet creation (111samples), plus2068.50MiB through other
voxel-shape join/optimization paths (284samples). Nearly all is pre-menu.
They identify an allocation/GC workload front; no CPU/thermal or critical-path
share is measured and no inference that3GiB remains live is justified.

Open BootOptim#73 already studies CPU/site/shape attribution and explicitly
mentions fastBitSets. Read its body and reporting-fix comments; do not create a
duplicate shape profiler. #324 concerns ResourceLocation string constructor
shortcuts and is NO-GO; it does not settle FastBitSet storage. #110 rejects
batch-union/limited-domain changes on exact semantics/economic coverage.
Integration was refreshed to058ac544; current source/docs are authoritative.
No reason to reintroduce those rejected geometry changes or remove Palladium
without separately testing its feature-level storage behavior.

### Models/data

Model-pipeline sites are present in old samples, but sample reservoir size/bias
does not measure their total footprint. Last ordinary GC reports2701.2MiB
occupied; these are ordinary collections, not the missing complete live census.
JSON/tag/recipe traffic estimates1652.53MiB during menu/world configuration.
These are investigation fronts, not proof of recoverable caches.

## Thermal front / next decision

0.1.7 has no thermal/battery/process-CPU or effective FPS counters, so this ZIP
cannot determine the heat slope, CPU-vs-GPU ownership or an appropriate throttle.
ActiveProcessorCount8 is confirmed; FPS/vsync configuration is not in this ZIP.
Charging is user-reported, not sensor-measured. Do not blame charging as the sole
cause or claim one renderer/hardcoded CPU count will solve other GPUs/devices.

Next instrumentation should add Android thermal-status/headroom (support may
vary), battery temperature and plugged/charging state, plus process CPU deltas,
to the existing low-frequency local samples. Battery temperature is not SoC
temperature. A missing/zero Android status is not proof of no vendor throttling.
Then choose a bounded workload policy (menus/loading FPS or CPU parallelism,
with hysteresis/correct thread ownership), preserving graphics quality and game
semantics. Do not disable vendor thermal protection. No new APK or automatic
throttle was introduced in this diagnostic analysis.

Primary references:

- https://developer.android.com/games/optimize/adpf/thermal
- https://developer.android.com/reference/android/os/PowerManager
- https://github.com/openjdk/jdk21u/blob/master/src/jdk.zipfs/share/classes/jdk/nio/zipfs/ZipFileSystem.java
- https://github.com/neoforged/JarJar

Disposition: physical JFR capture gate passed, full live/root ownership pending;
world gate failed due to OEM stop, thermal threshold unmeasured. Keep diagnostic
PR#83 separate from production claims. Preserve old failed-memory evidence and
the user's FancyMenu update decision; no mod/server changes during this analysis.
