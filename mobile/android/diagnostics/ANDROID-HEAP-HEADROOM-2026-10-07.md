# Android heap headroom candidate — 0.1.6

## Decision and measured premise

User requested deeper investigation of repeated foreground LOW_MEMORY deaths and
the simplest credible fix. Physical-device evidence remains authoritative:
[0.1.5 samples](VIVO-MEMORY-SAMPLES-2026-10-07.md). This is not a claim that an
8 GiB phone has unlimited usable memory or that Android is irrelevant. The pack,
OpenJDK, native allocations and graphics collectively exhausted available RAM.

Inspection of the actual FCL source found `setMinMemory(vs.getMaxMemory())`:
the maximum selected by the user was also forced as the minimum, ignoring the
existing nullable `VersionSetting.minMemory`. The supplied log confirms
`-Xms3072m -Xmx3072m`. No AlwaysPreTouch was present. Committed heap does not mean
every page was resident immediately.

Within that same run, closest JVM samples (under three seconds apart from the
Android readings, not simultaneous) show heap occupied / process RSS in MiB:

| Seconds from memory origin | JVM occupied | Process RSS | GC count |
| --- | ---: | ---: | ---: |
| 23 | 655 | 1432 | 13 |
| 33 | 960 | 3139 | 16 |
| 43 | 959 | 3156 | 17 |
| 110 | 973 | 3484 | 27 |

Private-other anonymous pages grew sharply while bionic allocated memory was
about 295 MiB at seconds 33/43. This is consistent with touched heap pages being
retained under a high minimum, not exact proof of anonymous-page ownership.
Final occupied heap was 2816.8 MiB, so genuine object pressure also exists.
No reduced-memory or startup-time measurement exists for the candidate yet.

## Candidate implementation

Engine commit `6b9df7460a58b59e3472af9d6d9a3e88b62e4918`, based on previous
delivered engine `411419dc4b33c17638a77eab2fcb66334aa2f3c4`. Separate candidate
branch `codex/android-heap-headroom-20261007`; Android prototype lane, not a
BootOptim production optimization.

- Calculate the same effective maximum once. Preserve configured positive
  minimum; otherwise use 512 MiB, clamped to the effective maximum.
- Explicit user JVM arguments still override default Xms/Xmx through the existing
  CommandBuilder rule. No automatic maximum policy change, forced GC, collector
  tuning, mod changes, renderer change or visual-quality reduction.
- Android basic sampling remains every ten seconds; Debug.MemoryInfo becomes
  every thirty seconds and full smaps only once at the start of sampling. Earlier
  repeated smaps scans took up to 5232 ms background wall time on this phone.
  Detailed fields are absent between full samples, not silently reused.
- Embedded JVM CSV adds optional G1 old/young occupancy counters (-1 if
  unsupported). They are not a live-object census or extra memory to sum.

G1 with a lower minimum can grow/shrink its heap rather than having its floor
fixed at Xmx. This cannot remove truly live objects, guarantee timely decommit,
or solve native/driver memory exhaustion. A smaller starting heap can increase
collections and change startup duration; performance and memory benefits require
the device run.

## Alternatives examined and disposition

- MobileGlues installed version is already 2.0.0, the current official release.
  Its advertised memory fixes are already in this run. Latest public source
  audit (main 97558a6, not proven identical to the installed release) found texture
  metadata and temporary conversion buffers, no demonstrated whole-texture CPU
  cache leak. Do not substitute an unvalidated renderer build across Mali/Adreno.
- The 8192x8192x2 block atlas and graphics growth are a concrete attribution
  front, not proof of a specific mod. No external ZIP resource pack was enabled.
  Removing named mods or reducing texture quality is deferred to user choice and
  actual allocation evidence.
- BootOptim integration refreshed to 058ac544aef11c0c10dc32e0aeacba2e39176d40.
  Read integration READMEs/catalog and PR #79 body/comments plus sprite-elision
  research: rejected visual/texture deletion is not resurrected for this phone.
  That experiment removed 3192 Decocraft sprites and changed the laptop atlas,
  but lacked this phone's pack/runtime and visual-equivalence evidence. Current
  phone pack did not contain that Decocraft build. Active PR #336 owns unrelated
  model-ancestry allocation diagnostics; no overlapping BootOptim profiler added.

## Required phone gate and next decision

Install 0.1.6 in place, keep MobileGlues, 3072 MiB maximum and this exact pack.
Export after one attempt before more launches overwrite the two retained sessions.
Confirm effective log arguments Xms512m / Xmx3072m unless an explicit override
exists. Compare occupied/committed heap and RSS/system availability by resource
phase and process-origin elapsed time, never by launcher setup time. A menu alone
is insufficient: check a representative world and export after failure/success.

If it fits without quality changes, retain the simple minimum-heap fix after
the runtime gate. If LOW_MEMORY persists, inspect G1 occupied old pool/total
heap and graphics/native growth. If old-generation occupancy stays high, real
pack objects/resources need attribution. If native/graphics growth dominates,
trace the resource/renderer owners. Do not keep escalating Xmx or add flags
blindly; Android's physical budget is still a real constraint. Desktop hosted
exact-pack CI cannot validate this Android runtime/driver mechanism.

Primary references:

- https://docs.oracle.com/en/java/javase/21/gctuning/garbage-first-garbage-collector-tuning.html
- https://developer.android.com/topic/performance/issues/lmk
- https://github.com/MobileGL-Dev/MobileGlues-release/releases/tag/V2.0.0

