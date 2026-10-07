# Vivo visual-mod reduction and resource-reload LMK — diagnostic8

User requested analysis only, specifically whether FancyMenu again loads excess
backgrounds; user will choose the next focus. No implementation or pack changes.
BootOptim remains frozen. Current session memory-1791407958182-5823, PID5823,
start2026-10-07T21:19:18.180Z, launcher0.1.8/code9. Previous diagnostic7 session
is retained in the ZIP but is not this run. Deep attribution false.

## Changes and outcome

Compared with7, discovered mod filenames removed: Colorwheel, Connectivity,
IrisVeilCompat, ETF, Iris, CIT Resewn, EMF, Spark, HoldMyItemsReforged and
automodpack-mod.jar. No new discovered filenames. This is filename comparison,
not content hashes or a controlled mod-by-mod experiment. WorldEdit/SableEdit
remain absent. User reports essentially unchanged~1FPS.

Launch options also changed: renderDistance8->3,maxFps120->30,vsync true->false;
simulationDistance5 remains. Basic sampler captures options at launch only,
not every later in-game change. Shader/scene conditions are not exact A/B.

ModernFix menu marker21:25:37Z,378.146s (6m18s); compare cautiously with432.863s
in7 because pack/settings/cache/stages differ. Player joined21:28:10Z. JEI
total1.120min (~67s), ingredient-filter25.71s, versus prior4.569min/~148s.
These are inclusive JEI wall timers, not CPU or proof of one removed mod's effect.
User opens pause/options/resourcepack screens21:30:46–21:30:55Z.
Resource reload begins21:31:01Z. Last game log21:31:28Z; no reload-finished marker.
Android matched foreground LOW_MEMORY at21:31:34.543Z, no JVM return callback.
This is an Android LMK, not a reported Java OutOfMemoryError in this new log.

## FancyMenu evidence

Actual discovered JAR: fancymenu-3.9.0-welite-main-neoforge-1.21.1.jar.
First reload's late FancyMenu preloader21:25:27Z:

- selected group0 bg_terracota_noche_shaders;
- panoramas=1,slideshows=0,suppliers=6,failed_suppliers=0;
- kept1background source,added0,skipped21configured background sources.

This supports the active-background filter, not reloading all configured assets.
Six suppliers belong to one panorama, not six configured backgrounds. Native
video reset log has backgroundsReset30,stoppedPlayers0,videoResourcesReleased0;
resetting registered background objects is not evidence30videos were loaded.

The failing reload logs FancyMenu's STARTING hook but never reaches a second
ResourceHandlers/ResourcePreLoader background-load marker before LMK. The visible
work remains SpriteResourceLoader/ModelManager. No evidence in this ZIP of the
old all-background preload regression. It does NOT prove the chosen panorama
has negligible footprint, that every old texture is released, or complete
ownership of the peak: no native allocation/image/GL texture owner tracing exists.

## Memory trajectory and limits

Snapshot timestamps UTC; MiB, not additive ownership categories:

| Time | Native allocator allocated | Proc RSS | System available |
| --- | ---: | ---: | ---: |
|21:30:54.038 (before reload)|913.2|4021.4|551.4|
|21:31:04.189|919.8|4068.8|477.6|
|21:31:14.332|971.3|4379.4|392.6|
|21:31:28.120|1147.1|4520.8|386.1|

Before->last native+233.9MiB,RSS+499.4MiB,available-165.3MiB. Counters overlap
Java/native/GPU/system classification and are sampled before death; cannot add
them or claim exact peak/saved bytes. Last Android sample is6.423s before LMK.
Last JVM sample21:31:28.566Z:2965.9MiB occupied of3072MiB,old2894.2MiB,
nonheap481.3MiB. Heap was already~2.8GiB before reload. No full live census.
Graphics attribution841.8MiB at21:31:14.332Z,not the last/final sample; cannot
attribute it to FancyMenu or add to native/RSS. Proc swap is also overlapping.

This supports a reload pressure peak on an already tight working set. Possible
old/new-generation overlap and image/model preparation are hypotheses, not a
measured retaining graph or a proven texture leak. Android's lowMemory boolean
was false at last sample; this does not contradict the later explicit LMK record.

73thermal/JVM samples,68Android rows. Battery38.2–38.9C,AC charging throughout;
headroom0.870–0.971,NONEstatus,unsupported sustained policy,60Hz display/vote.
World-before-pause process CPU~2.80core equivalents,similar to diagnostic7~2.82
for a different/longer phase. No measured GPU/frame times;~1FPS is user report.
This shorter run cannot establish a thermal win or exclusive FPS cause.

Texture errors include create fluids with corrupt/unknown PNG and invalid model
rotation. Two image errors occur before the failed reload and two during it;
similar image errors already existed in7. They are useful asset-validation
signals, but this log does not establish them as the cause of LMK or~1FPS.

## Disposition

Broad visual-mod/settings reduction did not visibly fix FPS according to user.
FancyMenu all-background preload not reproduced; new failure confirmed Android
LMK during general resource preparation. Best-supported next investigation front
is peak memory during reload (native images/atlas/model generation and lifetime),
with native/GPU ownership measurement needed before blaming a cache/mod.
Wait for user's chosen focus. No mod patch, APK rebuild, server/global-profile
or BootOptim change; no additional runtime tests.
