# Android memory attribution probe — 2026-10-07

## Premise and decision gate

The user's physical Vivo V2041 / Android 13 reported explicit LOW_MEMORY at
both 4096 and 3072 MiB max/min heap, MobileGlues. Prior Android exit PSS/RSS
samples are internally inconsistent (PSS > RSS). They establish no reliable
allocation breakdown. See LOW-MEMORY-2026-10-07.md.

User requested collection without USB. This is instrumentation, not a fix,
startup optimization or proof of a particular mod leak. Do not equate memory
outside OpenJDK's object heap with memory outside Minecraft: native textures,
buffers, libraries and VM machinery also belong to the game.

Next phone gate: update in place, keep MobileGlues and the same 3072 MiB setting
and pack, reproduce menu/world loading, reopen launcher, export Diagnóstico →
Compartir diagnóstico before further launches. Correlate session PID/epoch with
ApplicationExitInfo; compare within-session OpenJDK heap, ART and process/system
trends. Do not change heap, renderer or pack simultaneously. Missing counters
remain inconclusive. Sampling may miss a burst shorter than ten seconds.

## Two runtimes in the same process

Minecraft runs an embedded OpenJDK in the main application's native thread;
Android UI runs ART in that same process. Android Runtime.getRuntime() only
measures ART. The distinct OpenJDK Runtime counters must run inside game JVM.

A separate Java 8 classpath JAR is built from FCL/memory-probe source, copied
from packaged assets into private app storage, and used as a thin main wrapper
only for Minecraft. It starts a daemon sampler, then delegates the original
arguments unchanged to upstream mio.Wrapper.main. Original loader/error handling
remains intact. No javaagent/instrumentation native library is required. An
optional probe setup failure delegates to the upstream wrapper; unavailable
java.management counters do not prevent Runtime heap sampling. File extraction
failure keeps the original launch args and records an event.

## Files, origin and limits

Each run uses memory-<origin epoch milliseconds>-<PID>-* files in the private
exit-diagnostics directory. Wall timestamps align with Android exit records;
elapsed fields use the persisted probe origin. Mapping/sample durations are
recorded separately. This is crash attribution, not a timed startup benchmark.

- jvm.csv: OpenJDK heap used/committed/max, nonheap used/committed, direct/mapped
  buffer pools, GC counts/time and JVM thread count. Values in bytes, durations
  in ms. Missing optional metrics -1. Companion jvm-info.txt records runtime.
- android.jsonl: ART used/committed/max, process Debug.MemoryInfo PSS/private
  categories, bionic allocator size/allocated, /proc/self/status resident/peak/
  anonymous/file/swap metrics, system available/threshold/low-memory and selected
  /proc/meminfo fields, PSI and OOM score/adjustment when readable.
- mappings.jsonl: aggregate RSS/PSS/private/swap of /proc/self/smaps every minute,
  grouped by labelled ART/stack/GPU/native heap, library filename, other file or
  anonymous-unattributed. Filename mappings do not attribute a library's mallocs.
  Full file paths and mapping addresses are not saved.
- events.jsonl: Android trim/low-memory callbacks and optional collection errors.

Samplers are daemon/background priority, run every ten seconds (mapping scan
every sixty), stop after three hours, and flush each record by closing the file.
Two sessions retained, each data file capped at 4 MiB. JVM exit stops Android
sampling before stock process termination. Abrupt SIGKILL needs no exit callback
to preserve completed records. There is no forced GC, object walk, heap dump,
native allocation tracing/NMT, screenshot, upload or privileged enumeration.

Recording defaults on for this diagnostic alpha and is controlled by a persistent
checkbox in Diagnóstico; changes apply to the next game. Disabling it retains
existing evidence. Exit recovery and log/traces export remain available.

## Interpretation boundaries

**Do not sum overlapping counters or subtract logical heap usage from RSS to
produce an exact native-memory figure.** Committed JVM heap is not resident
memory; buffer/nonheap counters overlap native allocations. Android summary.java-
heap labels ART, while OpenJDK heap can appear as anonymous/private-other.
Bionic heap counters omit mmap allocations and may overlap game native usage.

Graphics counters depend on driver/vendor support. Zero/missing is not proof of
zero GPU allocations. Unknown anonymous pages stay unknown. System total minus
available is not "other apps": it also includes system, kernel and this process.
Modern Android limits other-process visibility; no per-app culprit is promised.
ExitInfo vendor samples are not assumed simultaneous with any live snapshot.

No device is attached. Compilation/signature/asset inspection is recorded in
BUILD.md. On-phone wrapper compatibility, available counters, sampling overhead
and attribution are unvalidated until the next exported ZIP. Optional sampling
cost is recorded; this build makes no performance or reduced-memory claim.

Primary references:

- https://developer.android.com/reference/android/os/Debug.MemoryInfo
- https://developer.android.com/reference/android/app/ActivityManager.MemoryInfo
- https://developer.android.com/reference/android/app/ApplicationExitInfo
- https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/Runtime.html
- https://docs.oracle.com/en/java/javase/21/docs/api/java.management/java/lang/management/BufferPoolMXBean.html
