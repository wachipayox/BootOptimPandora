# Vivo Android termination — confirmed low memory, 2026-10-07

## Origin and endpoint

User-exported `wachiland-android-diagnostic.zip`, collected with alpha 0.1.4.
No phone is attached to the build PC. ApplicationExitInfo records and the
preserved log originate from the user's physical Vivo V2041 / Android 13.
This is crash attribution evidence, not a comparable performance benchmark.

Session marker: PID 27491, start `2026-10-06T22:25:34.799Z`, MobileGlues.
Android process exit: `2026-10-06T22:38:22.027Z`, same PID/main process,
`LOW_MEMORY (3)`, status 0, importance 100 (foreground). No JVM completion
callback. Session correlation is true; Android explicitly supports LMK reporting.

Game log confirms MobileGlues 2.0.0 and `-Xmx4096m -Xms4096m`. Title screen
was registered, then user attempted world creation. The log ends after loading
12,921 recipes, with no Java OOM, fatal native signal, Minecraft crash report or
OpenJDK exit code. The operating-system termination explains that abrupt ending.

## Memory interpretation

Physical memory reported at recovery: 7,631 MiB total, 4,925 MiB available.
This is after death, so it cannot show how much was available at termination.
The latest exit record reports RSS 3,813,200 KiB (~3.64 GiB) and PSS
6,183,708 KiB (~5.90 GiB). These are Android/vendor samples, not exact peak or
heap measurements; the reported PSS exceeds RSS, so do not treat them as a
consistent simultaneous resident-memory breakdown or add them together.
The explicit LOW_MEMORY reason, rather than these sample sizes, proves LMK.

Two earlier main-process records also explicitly report LOW_MEMORY. Other
SIGKILL records are not automatically assigned the same cause: stock FCL uses
intentional process termination after the JVM exits.

`-Xmx` limits the OpenJDK object heap, not the application's total memory.
Native renderer, buffers/textures, JVM runtime and Android UI allocations also
consume resources. Four GiB heap settings do not imply a four GiB process cap.
No particular mod leak or launcher/native allocation culprit is established.

## Disposition / next gate

- The recovery collection and sharing path worked on this phone; the 0.1.4
  diagnostic build delivered actionable OS evidence without USB access.
- Keep MobileGlues for this device; the older Krypton/Ponder error is a separate
  incident. The repeated closes now have an explicit operating-system cause.
- Next bounded user trial: 3,072 MiB max heap with MobileGlues. Earlier 2 GiB
  failed with Java heap OOM; 4 GiB reached menu/world loading but Android killed
  the process. Whether 3 GiB fits both budgets is unvalidated.
- FCL currently sets minimum heap equal to requested maximum. Decoupling Xms
  is a possible follow-up, not a proven LMK fix; committed heap and physically
  resident pages are different, and a smaller Xms cannot fix oversized live data.
- If the 3 GiB trial fails, collect its OS report and game log before another
  launch. Java OOM vs explicit LMK determines whether pack/native memory must
  be reduced rather than repeatedly increasing Java heap.

Veil shader/framebuffer errors, EMF repeated-model warnings, loot/tag/recipe
errors are present but do not establish the LMK allocation source. Do not remove
mods based solely on the last log line.

## Follow-up: 3 GiB also terminated by Android

Second user-exported ZIP, `wachiland-android-diagnostic (1).zip`, confirms
MobileGlues and `-Xmx3072m -Xms3072m`. Session marker PID 18327, start
`2026-10-06T22:45:50.008Z`; same-process matching exit at
`2026-10-06T22:56:54.788Z`: explicit LOW_MEMORY (3), status 0, importance 100,
no JVM completion callback. The log reaches title screen and ends after the
menu/startup messages; there is no Java heap OOM or native signal report.

Vendor samples: RSS 4,744,100 KiB (~4.52 GiB), PSS 5,242,282 KiB (~5.00 GiB).
Again PSS exceeds RSS; they do not establish a coherent simultaneous memory
breakdown. The smaller PSS compared with the 4 GiB attempt is not an A/B memory
win: sample timing and workload differ. The explicit OS reason establishes LMK.

The 3,072 MiB gate therefore failed. Do not repeat the same trial or assume the
pack fits by moving the heap slider again. The next attribution needs separate
embedded OpenJDK live/committed heap, native/graphics and Android UI memory.
Android Runtime.getRuntime() only measures ART, not the embedded game JVM.
EMF repetition remains a suspect signal, not a proven leak. Changing the minimum
heap or reducing native/render allocations would still require phone evidence;
no memory fix has been implemented or validated by these diagnostics.

References:
https://developer.android.com/reference/android/app/ApplicationExitInfo
https://developer.android.com/topic/performance/issues/lmk
https://docs.oracle.com/en/java/javase/21/docs/specs/man/java.html

## New attribution build

Alpha 0.1.5 implements the next instrumentation gate, separating embedded
OpenJDK, ART, process and system counters. It does not identify an allocation
culprit or fix the memory pressure before phone evidence. See
[sampling design and limits](MEMORY-SAMPLING-2026-10-07.md).
