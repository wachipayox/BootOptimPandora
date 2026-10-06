# First on-phone memory attribution — alpha 0.1.5

## Origin and termination

User supplied `wachiland-android-diagnostic (2).zip` after a black screen and
manual stop. Physical Vivo V2041 / Android 13, MobileGlues, OpenJDK 21.0.1-internal,
`-Xmx3072m -Xms3072m`. These are within-run crash diagnostic samples, not a
comparable performance benchmark or a heap dump.

Game session PID 4167, marker 2026-10-06T23:14:31.009Z; probe origin .016Z.
Matching exit record: 23:20:05.916Z, LOW_MEMORY (3), foreground importance 100,
no JVM completion callback. Later USER_REQUESTED at 23:26:53.625Z refers to
**different PID 6077**. Manual stopping did not erase evidence and is not the
cause of the earlier game termination. Black screen after the first process
death may reflect activity restoration; no activity trace proves that mechanism.

Both probes worked: 34 JVM samples, 30 Android samples, 5 mapping summaries,
18 trim-memory events including critical running level 15. No probe errors.
Last JVM sample 23:20:03.466Z (2.45 s before kill); last Android sample
23:19:52.428Z (13.49 s before kill). Do not present them as simultaneous.

## Measurements

Final JVM sample:

- Heap occupied 2,816.8 MiB (sampled peak), committed/max 3,072 MiB.
  Occupied includes collectable objects; this is not a retained/live-object census.
- JVM nonheap used 392.8 MiB; direct buffer pool 15.96 MiB, mapped pool zero.
- 102 cumulative collections / 9,382 ms collection time; 67 JVM threads.

Final Android sample:

- ART heap used 29.2 MiB; initial 33.1 MiB. No growing Android object-heap leak
  appears in this run. This does not exclude launcher native allocations.
- Bionic native allocated 908.4 MiB versus initial 138.0 MiB. Ownership unknown:
  it can include JVM, game, renderer, mods and launcher.
- Vendor graphics summary 618.6 MiB versus initial 87.5 MiB. Supports substantial
  graphics growth but does not identify textures/mods or all driver allocations.
- /proc/self/status RSS 4,944.8 MiB; available system RAM 352.1 MiB via
  ActivityManager and 313.6 MiB via slightly later /proc/meminfo reading.
- Vendor Debug summary PSS 5,251.3 MiB exceeds proc RSS; these different sources
  still do not form a coherent additive resident-memory breakdown.

Do not sum these rows: counters overlap and have different time/measurement
domains. JVM heap/nonheap/buffers are logical allocator metrics, not resident
pages. No exact "outside Minecraft" number or per-other-app culprit is proven.

## Phase association and remaining attribution

Native allocations rise to ~871 MiB around t=233 s during resource reload.
Graphics summary jumps from ~203 MiB at t=301 s to ~574 MiB at t=311 s and
~619 MiB at t=321 s. Game log creates the **8192x8192x2 block atlas** at local
01:19:33 (UTC 23:19:33), before that graphics growth; additional atlases and EMF
entity resource warnings follow. The log ends at 01:19:50 during shader/resource
initialization, no Java OOM, fatal signal or JVM exit.

This makes resource/texture upload a concrete next attribution front, not proof
of a specific leaking mod or that an atlas alone caused the kill. No optional
file resource packs appear in the reload list; it lists vanilla/mod/builtin
resources. Do not suggest removing a named external resource pack without evidence.
Latest smaps grouping remains predominantly anonymous-unattributed (~4,399 MiB
RSS), which includes game heap/native pages; small libmobileglues.so file RSS
does not measure its heap/driver allocations.

The pack plus runtime approaches the phone's memory budget during startup.
Increasing Xmx does not establish more total-memory headroom. Root allocation
owners still require further targeted attribution; do not remove mods or change
renderer/heap based only on the last warning. No memory fix implemented here.

## Probe validation and cost

The user phone confirms embedded-JVM wrapper launch, counters, persistent files,
OS exit correlation and sharing. Partial logs survived both the OS kill and
later manual stop. This closes the basic collection gate for 0.1.5.

Measured sampler durations (wall, not CPU or added critical-path time):

- JVM: 177 ms summed over 34 samples; first initialization 118 ms, most later
  samples 0–3 ms; final 22 ms. No forced collection occurred.
- Android Debug/process snapshots: 11,584 ms summed, maximum 1,168 ms.
- Five smaps scans: 19,855 ms summed, maximum 5,232 ms.

These run on the diagnostic background thread and are not sequential critical-
path startup overhead, but the full scans are expensive on this phone under
pressure. Reduce/disable repeated full smaps scans for future diagnosis unless
they serve a new attribution premise; most pages remained anonymous. Existing
non-instrumented 0.1.4 already suffered explicit LMK, so instrumentation is not
the sole origin of the repeated memory problem. Do not call this zero-cost
instrumentation or use this run for a startup performance comparison.
