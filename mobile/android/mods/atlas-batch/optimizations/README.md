# Retained Android startup mechanisms

## Early loading screen

Launcher stages the addon before NeoForge. An exact official Drippy provider
pin permits replacement of legacy GL calls with owned modern drawing. Actual
MobileGlues/core context selects the default route; no GPU-vendor heuristic.
Logo scales proportionally to fit with horizontal margins. GL ownership stays
on the provider thread; cleanup on handoff. `earlyWindow.mode=legacy`,
`earlyWindow.fitLogo=false`, or `experimental.earlyWindowCompatibility=false`
override individual behavior. No Drippy fork or assets copied. Physical phone
visual/startup gates passed; other provider versions/GPU families retain stock
or still require their own visual gate. See the early-window research ledger.

## JRE module graph closure

Exact JRE class resource plus canonical method instructions gate a bitset
transitive closure. Original maps/read-set identities, dependencies, validation,
callbacks and publication order preserved. No persistent world or classloader
cache. Unknown shape/runtime or more than2048nodes uses stock. Disable with
`startup.moduleGraph=false`. Host semantic/mutation tests and phone menu/server
gates passed. Main closure 2.882s ->0.213s in one pair; whole startup surrogate
only ~0.91s different. Retained for bounded CPU removal, not a large end-to-end
startup claim. Pin and evidence: ANDROID-MODULE-GRAPH-2026-10-13.md.

## Mixin opcode names

Exact Mixin class and wrapper, plus actual target-loader ASM table/constant
semantics gate an inline direct lookup. No transformed-class cache, loader
replacement or skipped validation. Unknown/already-loaded target stays stock.
Disable with `startup.opcodeNames=false`. Host golden equivalence8209values
and phone startup/server gates passed. Direct pair observed196.560/200.923s;
one pair does not establish a universal gain. The earlier BootOptim delivery
experiment missed the SERVICE class; this existing Android premain delivery
is distinct. BootOptim source unchanged.

Defaults request these two startup hooks on launcher0.1.16+, independently of
Drippy. JVM overrides remain available for controls. Ordinary-launch default
and rare override gates are recorded in the research ledger, including any
pending checks. These startup mechanisms do not promise higher world FPS.
