# Android Java object attribution — optional diagnostic, not an optimization

## User request and decision

User will update the controlled FancyMenu fork separately and asked to investigate
Java model/data memory in parallel. No mod/cache removal or renderer/quality
change is authorized by an attribution result alone. This change prepares a
local, shareable one-session diagnostic because no phone USB/ADB connection is
available. It does not claim that 2581.9 MiB of old-pool occupancy is necessary,
live, a leak, owned by models, or attributable to any particular mod.

## Starting evidence and history audit

See `VIVO-HEADROOM-AND-FANCYMENU-2026-10-07.md` for the physical 0.1.6 failure.
Matched foreground Android LOW_MEMORY, final occupied heap 2640.2 MiB, old pool
2581.9 MiB, nonheap 425.2 MiB; last Android available 268 MiB, RSS 4481.8 MiB,
VmSwap 1909.2 MiB. Different counters overlap and do not provide owner totals.
The user is replacing the old published FancyMenu binary which eagerly decodes
many panoramas; that native-image front is separate from this Java attribution.

BootOptim integration was refreshed to
`058ac544aef11c0c10dc32e0aeacba2e39176d40`. Read the operating guide, source/readme,
research index, model pipeline, exact-pack gate and optimization catalog. Open
PR #336 (model ancestry owner CPU/allocation attribution) was reviewed: it does
not measure the phone's live heap or retaining roots. Closed/rejected #43 showed
only ~41.7 ms estimated gain for ~55 MiB retained by a proposed mixin memoizer;
it is not evidence for adding another cache. Other historical bake/identity
experiments cannot justify memory deletion from call counts. This is a distinct
Android diagnostic premise, not another model-bake profiler or a BootOptim mixin.

Current Android integration branch `codex/android-prototype` remains
`c41874b1879654d7bf063cd41387897fcf340c95`. Open draft #82 carries the separate
0.1.6 heap-headroom candidate and its failed phone gate. This diagnostic starts
from the delivered 0.1.6 engine/reproduction state on its own branch,
`codex/android-java-object-attribution-20261007`; it is stacked on #82 for source
review and must not be promoted merely because packaging succeeds. BootOptim
integration/main and FancyMenu user workspace are untouched.

## Runtime capability evidence

Bundled JRE21 universal image (asset version 12) was inspected statically with
JDK21 `jimage list`. It contains `jdk.jfr`, `jdk.management`,
`jdk.management.jfr`, `jdk.jfr.Recording` and the HotSpot DiagnosticCommand
implementation. Bundled profile.jfc enables allocation/old-object stack samples,
with root-search cutoff 0 ns by default. This demonstrates classes/configuration
presence, not successful native JFR/census operation on the physical phone.

The companion remains Java8 bytecode loaded inside embedded OpenJDK; Android ART
does not parse it. JFR is reflected through public API so older/custom runtimes
can reject the optional mechanism without preventing game launch. No new JVM
agent, root, attach socket or USB endpoint is required.

## Diagnostic contract

- Basic 0.1.6 counters remain the default. Deep attribution defaults off.
- A visible checkbox arms exactly the next game launch and is then consumed.
  Arming also enables basic recording. Installing 0.1.7 alone does not start JFR
  or request a collection/census.
- JFR begins before the original Mio wrapper. Only sampled allocations with
  creation stacks (20/s target), old-object samples with stacks, and GC/heap
  summaries are enabled. No full profile/CPU sampling, object values, initial
  properties or Java argument events are requested.
- OldObjectSample cutoff is explicitly 0 ns. No reference-path/root traversal
  is requested in this first pass. Samples expose creation sites/age, not a
  complete dominator graph or exact per-mod retained-byte census.
- One low-priority worker is bounded to 20 minutes. Rolling repository target
  16 MiB is chunk based, not a hard file-size limit. Each 30-second snapshot is
  written to a staging file, then replaces the last complete snapshot. A file
  exceeding 32 MiB is discarded and JFR is stopped, keeping earlier evidence.
  SIGKILL cannot flush a final snapshot; the latest successful one may be old.
- The Android sampler incrementally reads at most 64 KiB of game log per tick,
  with overlap. It rejects logs not containing this session's origin argument
  and waits for ModernFix's `Game took ... seconds to start` marker. No marker
  means no automatic census; this is a menu proxy, not visual validation.
- At the first marker, requests one occupied histogram (`-all -parallel=1`)
  followed by one live histogram (`-parallel=1`). The latter requests a full GC.
  Both operations are high-impact and may pause OpenJDK. No retry after a
  partial attempt. A current Android guard is checked before each operation:
  <=15 seconds old, >=768 MiB available, lowMemory=false. This heuristic reduces
  avoidable pressure but cannot guarantee that Android will not kill the app.
- If JFR, the marker, headroom or DiagnosticCommand is unavailable, write the
  reason and preserve the working basic sampler. No hidden heap-dump fallback.
- Histogram text cap 2 MiB characters, status cap 128 KiB. Published JFR files
  join the existing ZIP export; temporary and request/guard files are excluded.
  Keep two sessions. Log masking remains; no automatic upload.

## What can be concluded from the next ZIP

1. Compare occupied vs post-requested-GC heap/class shallow totals to identify
   reclaimable garbage vs live-class footprint at that checkpoint. A change in
   the game caused by this diagnostic is not itself a production optimization.
2. Find dominant model/data/mod classes; arrays require creation stacks to
   propose an owner. Shallow sizes omit children and cannot be summed by class
   into complete mod-retained sizes.
3. Allocation sample weights estimate allocation traffic. Large traffic may be
   short lived; it is not equivalent to retained or resident memory.
4. Old-object samples give candidate long-lived objects and creation sites;
   duplicates can recur across JFR snapshots. Absence is not a proof of absence.
5. Before deleting/releasing anything, inspect the implicated owner source,
   lifetime/readers/invalidation and optional-mod contract. If ambiguity remains,
   consider a separately controlled bounded-root pass. Avoid a multi-GiB dump
   unless these smaller captures fail to answer the premise.

Offline PC helper `tools/HeapAttributionReport.java` streams JFR rather than
expanding a large JSON representation. It reports allocation groups and oldest
sample sites, with explicit traffic/retention limits. Compilation is checked;
its interpretation and phone event availability are pending actual data.

## Validation / physical gate

0.1.7 compiles/packages and is statically inspected. No device connected, no
live phone census, no measured memory saving, no equivalence/startup performance
claim. Do not treat diagnostic startup durations as performance benchmarks.

First complete the user's single-change FancyMenu trial on basic diagnostics.
For the attribution trial: install compatible 0.1.7, enable the deep checkbox,
keep renderer/Xmx/other settings fixed, stay at the menu >=30 seconds so the
one-time operation can finish, attempt world creation, then export before
another game launch. Read status durations/skips, live/occupied histograms, JFR
event summary and timestamp-correlated Android/JVM counters together. A missing
live census can still leave useful allocation evidence, but does not establish
retained sizes. Only then select a concrete source-level optimization.

## Primary method references

- https://docs.oracle.com/en/java/javase/21/docs/specs/man/jcmd.html
  (`GC.class_histogram` high impact; -all includes unreachable objects).
- https://docs.oracle.com/en/java/javase/21/troubleshoot/troubleshooting-memory-leaks.html
  (allocation sites/old-object evidence and expensive paths to GC roots).
- https://docs.oracle.com/en/java/javase/21/docs/api/jdk.jfr/jdk/jfr/Recording.html
  (recording limits, dump and close lifetimes).
- https://github.com/openjdk/jdk21u/blob/master/src/jdk.jfr/share/conf/jfr/profile.jfc
  (event names/default root cutoff).
