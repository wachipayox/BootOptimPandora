# Microsoft OpenJDK 25: crash of the JVM from time-based G1 heap sizing (2026-10-22)

Decision: **adopted**. Wachiland profiles on Java 25 start with `-XX:+UnlockDiagnosticVMOptions -XX:-G1UseTimeBasedHeapSizing`
when the JVM knows the flag. Code: `crates/backend/src/launch/g1_heap_sizing.rs`.

## Symptom

Two crashes of the game JVM in a real play session on the fast PC (Java 25.0.1, Beta profile), 13 and 23 minutes into the
game, both with the same message and the same thread:

```
Internal Error (g1HeapRegionManager.cpp:55)
guarantee(Thread::current()->is_VM_thread() || FreeList_lock->owned_by_self()) failed:
  master free list MT safety protocol at a safepoint
Current thread: WatcherThread "VM Periodic Task Thread"
JRE: OpenJDK Runtime Environment Microsoft-12574223 (25.0.1+8-LTS), g1 gc, windows-amd64
```

Elapsed time at the crash: 781 s and 1381 s. The failing thread is the JVM's periodic task thread, not the game.

## Cause

A bug of Microsoft's OpenJDK 25 build, not of the game or the pack. Microsoft's builds carry a feature of their own,
`G1UseTimeBasedHeapSizing` (diagnostic, on by default), that shrinks the G1 heap from a timer. The timer runs on the
`WatcherThread` and calls the region manager without taking `FreeList_lock`, so the assertion above fails.
The runtime that Mojang ships as `java-runtime-epsilon` is that Microsoft 25.0.1 build.

Sources:

- https://github.com/microsoft/openjdk/issues/677
- https://github.com/PaperMC/Paper/issues/14123 (the same crash; it also fails with 25.0.3, so a newer Microsoft build
  is not a fix today)

Checked on the PC with the launcher's own runtimes: `java-runtime-epsilon` (25.0.1) lists
`bool G1UseTimeBasedHeapSizing = true {diagnostic} {default}` in `-XX:+UnlockDiagnosticVMOptions -XX:+PrintFlagsFinal`
and accepts `-XX:+UnlockDiagnosticVMOptions -XX:-G1UseTimeBasedHeapSizing`; `java-runtime-delta` (Java 21) does not list it
and exits with an error on the same arguments. So the flag cannot be passed blindly.

## What was done

- Pandora adds the two arguments for `profile_wachiland-*` launches that run on Java 25 (the same condition as the zipfs
  agent: `java25` and runtime major >= 25), only when this exact JVM lists the flag. The probe is one
  `java -XX:+UnlockDiagnosticVMOptions -XX:+PrintFlagsFinal -version` per JDK build (about 0.3 s), cached in memory and in
  `jvm-flag-probe.json` in the launcher folder, keyed by the hash of the runtime's `release` file and the size of
  `lib/modules`. A failed probe is not cached.
- The decision is by code, not by version (the user's rule for patches on other software): a JVM that stops listing the flag
  is left alone. `[WBOMBO-COMPAT] ACTIVE|SKIPPED|FALLBACK project=Pandora mechanism=g1-time-based-heap-sizing-off mod=java@<version> ...`
  goes to `launcher.log`. `FALLBACK reason=flag-not-in-this-jvm` means Microsoft removed or renamed the flag (reopen this
  note); `reason=jvm-probe-failed` means the JVM could not be asked.
- If the player or the profile already sets `G1UseTimeBasedHeapSizing` (any of `+`, `-` or `=`), nothing is added and the
  setting wins. `-XX:+UnlockDiagnosticVMOptions` is not repeated if it is already there.
- The arguments go with the profile JVM arguments, so they are part of the key of the AOT cache (the JVM shape changed):
  existing instances train the cache once more on their next game.

## Validation

- Player validation (2026-10-22, PC with RTX 3060, Microsoft OpenJDK 25.0.1, build `aced70cd`): `launcher.log` showed
  `ACTIVE ... arg=-XX:-G1UseTimeBasedHeapSizing` and a session of 30 minutes in a world ended without the crash. One clean
  session is a first signal, not proof; a new `g1HeapRegionManager.cpp:55` crash with the flag active reopens this.
- Not an issue for WachilandEliteCompanion: it is a JVM bug, not a defect of the pack.

## Reopen when

- A Microsoft build that fixes microsoft/openjdk#677 is the runtime of Mojang's `java-runtime-epsilon`: the flag can go.
- `[WBOMBO-COMPAT] FALLBACK ... mechanism=g1-time-based-heap-sizing-off` appears: the flag changed in the JVM.
- The same crash appears with the flag off: the cause is something else.
