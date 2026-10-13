# Android startup graph and opcode experiments, 2026-10-13

Isolated branch: `codex/android-module-graph-experiment-20261013`.
BootOptim is read-only. Reviewed integration origin
`4813ab19aec2a508034cd7c5e36410ee3723411a`, the boot/mixin pipeline ledger,
and relevant open/closed PRs #213, #215, #228 and #271. Earlier opcode work
validated semantics but its GAME transformation service missed the SERVICE
class. This experiment changes delivery through the existing Android premain
agent; it does not repeat that unsuccessful delivery premise.

## JPMS closure: mechanism and guards

JFR attributed repeated main-thread set traversal to JDK Resolver.makeGraph.
Replace only the transitive closure tail with bitset propagation, unioned into
the original read sets. Preserve module identities, parents, checks, callbacks,
publication order and classloader topology. No asynchronous publication.

Exact phone JRE21 Resolver resource SHA-256:
`164d37585babeb3e3c5cdeb13591f0bc6ae932b53da29f550331e425e1f676e2`.
Canonical makeGraph instruction digest (JVMTI constant-pool/debug repacking
allowed, changed instructions rejected):
`4988200ba1738b615358d9fca0bae4c04b9b4b849daf145c8d23fbe71dd237dc`.
Map/set implementation and graph size are checked before mutation. Unknown
runtime/shape returns to stock. Primitive closure buffers live only during the
call. Test property `wachiland.android.startup.moduleGraph=candidate|control`;
absent property does not install this experimental hook.

Host semantic gate: 280 random graph cases, cycles, partial parent graphs,
empty/small/dense graphs; original identities and resulting reads preserved.
Exact phone class rewrite verified; an instruction mutation rejected. Desktop
synthetic speed measurements are not phone startup measurements.

## Physical measurements

Launcher 0.1.15, support 0.2.22, same phone/JRE/pack, MobileGlues, 2816 MiB,
retained G1/thermal mode and fixed fan. Existing JFR/probe settings in both.
Single candidate/control pair; phase timers are monotonic scope timers.

| Field | candidate | control |
| --- | ---: | ---: |
| Main graph closure, 248 modules / 420 nodes | 213.411 ms | 2881.611 ms |
| All observed closure calls, summed sequential scopes | about 254 ms | about 2973 ms |
| ModernFix launch-to-bootstrap marker | 57.55 s | 58.26 s |
| ModernFix `Game took` startup surrogate | 210.941 s | 211.854 s |
| Process marker to server/world interval endpoint | 281.400 s | 289.697 s |

The scope removes about 2.72 seconds of repeated closure work in this pair.
The whole startup surrogate changes only about 0.91 seconds: one pair cannot
establish that end-to-end gain. Server/world intervals differ and must not be
credited to the graph mechanism. ModernFix's marker is explicitly a surrogate,
not a newly measured main-menu endpoint. No end-to-end or world FPS claim.

Support 0.2.21 whole-class retransform hash guard failed: stock ran. Exclude that
attempt from graph A/B evidence, though its ordinary world JFR is usable for
attribution. Support 0.2.22 canonical hook logged true and candidate/control.
Candidate and control joined the server without new bridge/verification errors.

Candidate world capture began before the world-ready marker; 2.077 loops/s is
not comparable with warm control's 9.322 loops/s. Separate startup and warmed
world windows; wait at least 60 seconds after the observed world-ready marker.

## Opcode direct lookup

Support 0.2.23 experimental delivery reuses the user-owned BootOptim #213
semantic algorithm. Exact Mixin 0.15.2 / Mixin 0.8.7 Bytecode class pin:
`f739f2b50dc1deb178df4cd16a977808795d965a0fe0118a5af233f5db8abe37`.
Five-instruction wrapper and actual target-loader ASM constants/Printer table
must match. Inline the existing ASM table; no extra runtime helper dependency,
cross-loader cache, retained mod data or skipped Mixin verification.

Golden host gate: 8209 integer values including every runtime ASM constant,
negative values, holes and extrema; stock and replacement names identical.
Changed class rejected. Property `wachiland.android.startup.opcodeNames` selects
candidate/control; unknown or already-loaded target retains stock. Not promoted.
Physical candidate logs `guarded=true route=candidate`; runtime and comparable
control passed startup and joined the same server. Whole startup surrogates in
this pair: candidate 196.560 s, control 200.923 s (4.363 s difference). Bootstrap
markers were 50.78 / 54.52 s. Graph hook stayed candidate in both; helper/APK,
pack, JFR configuration and other settings stayed identical. One pair supports
the premise, not a stable universal timing guarantee. Earlier 210--212 s graph
runs are not the direct opcode control and must not inflate the opcode claim.

In the first190s JFR window, candidate has one main-thread getOpcodeName sample
and control has103; control leaf Class.copyFields has74samples. The replaced
reflection cost disappears in this matched attribution window. This is supporting attribution; sample
counts are not seconds saved. Same warm dry server capture after more than five
minutes: 8.532 loops/s, p95 167.901 ms, p99 215.181 ms, maximum 302.960 ms, five
frames over250ms in90s; ~19.964 client ticks/s. No world FPS benefit claimed.

## Retention and automatic launch, 0.2.24 / 0.1.16

Retain the bounded exact-runtime mechanisms for removing repeated work with
unchanged graph/name results, with the modest whole-startup evidence explicitly
reported above. Launcher defaults request both hooks; each runtime guard still
decides whether it is applicable. `startup.moduleGraph=false` and
`startup.opcodeNames=false` override independently. `enabled=false` disables the
addon. Explicit expert JVM arguments override automatic requests. Unknown JRE,
Mixin or ASM table keeps stock. Stage startup independently of Drippy and of
early-window compatibility; use a separate addon property in the launch wrapper.
Physical automatic-default launch validation is pending below.

The existing one-shot live bench accepts an optional fifth boolean for staging
uploads. Four-field requests preserve the original staging setting. Snapshots,
10s transition settling, validity checks and restoration cover this flag too.
No new profiler, per-frame object sampling or texture-content changes. This
tests whether the earlier staging benefit generalizes to the current crowded
server. Its native samples are visible in the current world profile, which
alone does not establish a regression.

Raw evidence: `C:/BootOptimBench/android-device-server-fps-20261011` and
`C:/BootOptimBench/android-module-graph-20261013`. These contain complete logs,
JFR, phase CSVs, graph/class verification harnesses and stage scripts. Phone
account and world state preserved; no BootOptim source or Drippy fork modified.


Automatic staging verification: compiled launcher1.16 argument preparation passed
host checks without Drippy, legacy screen override, individual startup off,
all off, enabled=false, explicit JVM control precedence and native agent
unavailable. Existing bootstrap paths preserved; sampler off. Test Android stubs
are outside APK source sets. This verifies preparation only, not absent-mod
physical gameplay. Physical auto-default launch logs both audited hooks true
without those properties in the instance config. Early screenshot confirms full
logo with margins. Main-menu/server completion gate follows this launch.

Automatic physical completion: launcher0.1.16/support0.2.24 reached the server
and same stationary glass pose, both hooks guarded=true, sampler off, no manual
startup properties in fclversion.cfg. ModernFix surrogate198.921s and
process-to-world265.757s; this is a completion gate, not an additional clean
performance A/B because the APK and automatic staging changed. Early logo/bar
fully visible, scale0.9104056; provider handoff1079frames. No new addon hook,
bytecode-verification or class-format failure observed. Other-mod warnings are
not silently interpreted as new startup regressions.

Warm opcode control capture after five minutes in the same dry glass scene:
9.066loops/s, p95 156.531ms, p99 196.765ms, maximum253.303ms, one frame over250ms
in90s,19.953ticks/s. Candidate8.532 versus control9.066 is a small between-run
difference, not evidence of either an in-world gain or a meaningful regression
from this startup-only mechanism. External base activity and warm-up remain.
