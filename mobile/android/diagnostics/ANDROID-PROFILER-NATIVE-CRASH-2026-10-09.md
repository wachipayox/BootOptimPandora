# Android FPS probe native startup failure, 0.1.10

## Confirmed evidence

User supplied [the complete first-attempt log](https://mclo.gs/2vaTDSt)
before retrying APK 0.1.10. This recovers the missing evidence for this attempt;
the similar 0.1.9 attempt remains unproven because its original log was lost.

Embedded OpenJDK 21.0.1-internal, Android 13/AArch64, main thread, JVM elapsed
1.639257 seconds. MemoryMain's basic recorder started before the failure.
HotSpot reports SIGSEGV / SEGV_MAPERR at address 0x8, with this chain:

```text
libinstrument.so: iconv_open
convertUtf8ToPlatformString
appendToClassLoaderSearch
InstrumentationImpl.appendToBootstrapClassLoaderSearch
PerformanceAgent.premain
MemoryMain.main
```

This attempt fails in our optional profiler's dynamic bootstrap append, before
Minecraft/mod startup. It is a native crash, not a catchable Java exception.
The trace establishes the failing call; the exact defect in this packaged
Android iconv implementation has not been reverse engineered. This evidence
does not classify unrelated earlier LOW_MEMORY or thermal exits.

## Isolated fix, 0.1.11

- Stage the same counters JAR before starting the JVM and supply it through
  `-Xbootclasspath/a`, as documented by the
  [Java 21 launcher](https://docs.oracle.com/en/java/javase/21/docs/specs/man/java.html).
- Combine existing bootstrap paths (including Cacio) and the counter path into
  one option, preserving their order, without changing application classpaths.
  Only do this when FPS preparation succeeds. Ordinary unarmed runs preserve
  the original bootstrap argument.
- Remove the dynamic append call entirely; no native fallback or retry.
- Resolve counters using the bootstrap loader, verify shared class identity
  before installing any transformation, and record counters_bootstrap_ready.
  Missing or inconsistent counters disable the optional agent via the existing
  bounded Java error-report path and normal game launch continues.
- FPS capture defaults off for fresh preferences. Existing explicit armed states
  remain respected, and successful preparation consumes the one-shot switch.

Minimal premain, wrapper activation, separate bootstrap-only counters,
allowlisted hooks, output bounds, normal memory/thermal recording and attempt
log recovery remain in place. No renderer/heap/pack changes or FPS win is claimed.

## Validation boundary and phone gate

Compile/package and static source/bytecode/asset/manifest/signature inspection
are recorded in BUILD.md. No runtime tests are run on this computer. Physical
Android startup, actual hook coverage, and observer cost still need validation.
On the corrected APK, manually arm FPS once, leave Spark/deep heap profiling off,
enter the world, wait for JEI plus approximately four minutes, and export the ZIP.
If startup fails again, export before another attempt; distinguish bootstrap
counter readiness, agent readiness, installed hooks and capture completion.

The user's ongoing 0.1.10 unarmed retry can still supply ordinary memory/thermal
evidence, but cannot validate the corrected FPS path or produce frame counters.
