# Wachiland Android Support — 0.2.0 foundation

Separate client mod for the Android pack, currently NeoForge 1.21.1. The build
directory retains the historical atlas-batch name; the new artifact is
`wachiland-android-support-neoforge-1.21.1-0.2.0-foundation.jar`, mod ID
`wachiland_android_support`. Remove/disable the old atlas diagnostic JAR before
installing this one. It is a foundation, not a demonstrated FPS optimization.

Configuration: `config/wachiland-android-support.properties` in the instance
game directory. Defaults `enabled=true`, `diagnostics=false`; no reduction in
resolution, textures, mipmaps, animation or simulation. Invalid/unreadable
configuration disables the support mechanisms. Non-Android clients skip these
mixins. GPU vendor names are reported, not used to enable unvalidated fixes.
Defaults also include `experimental.stagedUploads=false`.
Capability presence is also only evidence of availability, not proof of correct
renderer implementation. No automatic mod removal or renderer change.

`/wachilandandroid status` reports Android detection, flags and the graphics
profile. `/wachilandandroid reloadconfig` reloads those flags and turns off the
legacy batching experiment; restart Minecraft for a fresh diagnostic window.
The profile logs vendor/renderer/version once on the render thread plus relevant
framebuffer/copy/storage/sync entry points. Diagnostics and marker probes require
explicit opt-in; diagnostic counters are not shipping performance claims.

Roadmap and mod audit: [Android support program](../../diagnostics/ANDROID-SUPPORT-PROGRAM-2026-10-10.md).

## Experimental staging route (not promoted)

Opt-in `experimental.stagedUploads=true` tests CPU upload into a temporary RGBA8
texture followed by a GPU framebuffer copy into the original atlas. It preserves
the uploaded rectangle, alpha, mip level and animation; it does not replace the
atlas or compress textures. Only exact vanilla SpriteContents/TextureAtlas in
unobstructed world frames, no Kerria, no legacy batching and no PBO are eligible.
Dimensions above 256 pixels keep stock. The texture/FBO are deleted in the same
call; no pixel pointer survives the call. Driver in-flight memory remains a
measurement gate, not a guaranteed bound of one physical allocation.

Before first use, a byte-exact 2x2 RGBA readback tests GPU copying to an offset in
destination mip level 1. Capabilities alone cannot enable this route. A failed
self-test or runtime GL error disables it for the session. Texture binding and
read framebuffer are restored; the self-test additionally restores pixel stores
and pack/unpack PBO bindings. Native uploads retain the caller's unpack settings.
The prototype queries GL errors and consumes any pre-existing error while
disabling itself: this diagnostic boundary is one reason it is not production.
Unknown modified callback contracts have not been validated across packs.

With `diagnostics=true`, a local `wachiland-staging-probe.txt` marker reuses the
existing full-frame A/B/A recorder: 60 s warm-up, 60 s stock, 60 s staging, 60 s
stock. It restores the original in-memory flag on completion or leaving the
unobstructed world. Do not also create the render-ahead marker. Remove the marker
and restore `diagnostics=false` after testing. A run where copying falls back is
not a candidate performance measurement. No FPS or thermal gain is claimed yet.

## Historical diagnostics retained for experiments

Version 0.1.4 adds per-mipmap wall time and attribution of the first managed
upload in each frame. It records the Linux render-owner TID once for system
trace correlation. Failure to read that optional identifier is harmless.
It never changes sprite ordering, image data, animation or graphics settings.
See [physical upload census](../../diagnostics/VIVO-UPLOAD-CENSUS-2026-10-10.md).
The [native wait follow-up](../../diagnostics/VIVO-UPLOAD-ORDER-AND-NATIVE-WAIT-2026-10-10.md)
confirms driver synchronization inside sub-upload and records system memory
pressure. Neither diagnostic is an optimized release.

Version 0.1.3 adds an upload census after 60 seconds of world warm-up, measuring
120 seconds of full-frame and managed native upload wall time. It tracks up to
256 sprite identities, atlas/name, mip call counts, pixels and maximum call wall
time. References are cleared after completion; image pixels/native pointers are
never retained. Without the render-ahead marker no setting changes occur.
These diagnostic measurements are not GPU execution times or shipping telemetry.

Batching now defaults **off** on all platforms: the physical attribution run
found 5068 uploads and zero repeated keys. `/wachilandatlas on` remains available
for isolated experiments, not as a shipping recommendation.

On Android only, an explicit `wachiland-render-ahead-probe.txt` file in the
instance game directory opts into a same-session Sodium render-ahead probe:
60 seconds warm-up, 60 seconds original limit, 60 seconds limit 1, 60 seconds
original limit. Frame and managed GL upload wall times are measured separately;
they overlap and are not GPU execution time. The original value is restored on
completion or opening a screen/leaving the world. Nothing is written to Sodium
config. Remove the marker after a trial. No automatic probe without the marker.
The attribution counters below are disabled during this probe to keep its
measurement overhead consistent across all phases.

Version 0.1.1 adds a bounded 180-second in-world attribution window. It records
all original sprite uploads, repeat sprite/atlas/coordinate keys, immediate
interpolation and queue fallback reasons. It reads Sodium visibility and
AsyncParticles mode once at the first world frame. This is diagnostic overhead,
not a timed performance candidate. No image references survive this observation
window's frame boundaries, and it does not change batching safety decisions.
See [offline audit](../../diagnostics/ATLAS-BATCH-OFFLINE-AUDIT-2026-10-10.md).

A separate client mod for Minecraft 1.21.1 / NeoForge 21.1.248+ (21.1 series).
It can be used with the existing Wachiland Android APK 0.1.11; it changes no
launcher code, server profile, BootOptim source or graphics settings.

## What is being tried

The Vivo diagnostic 11 recorded about 5.65 logical client ticks per visible
frame and substantial inclusive GL upload wall time. That did not prove repeated
updates to each sprite; the later attribution run found none in its scene.
When explicitly enabled, this candidate advances every animation ticker but retains
the last dirty discrete upload for each sprite until a rendering boundary.
An earlier transition remains dirty even if the last tick does not transition.

Resolution, animation clocks, mipmap generation and logical game ticks are
unchanged. Interpolated images use the original immediate path. That is the
intended quality contract, not a claim of runtime or visual validation.
See [research and acceptance gates](../../diagnostics/ANDROID-ATLAS-BATCH-2026-10-09.md).

## Phone trial

1. Put the JAR in the **local Android instance's `.minecraft/mods`** directory.
   Do not upload it into the global parent profile or install duplicate copies.
2. Keep APK 0.1.11, MobileGlues, memory, thermal mode, resource packs and distances
   unchanged. Kerria must be absent; when loaded this candidate stays stock,
   including when Kerria's own setting is disabled.
3. Arm **Investigar FPS en la próxima partida**; leave Spark and deep Java
   attribution off. Enter the same world and view the same scene. Wait for JEI
   initialization and remain in-world for about four minutes, then exit normally.
4. Share the usual diagnostic ZIP and report whether animation/particles/textures
   look wrong. The game log records `hooks_ready`, queue replacements and sends.

The candidate activates by default on the Android embedded JVM. It stays stock
in menus, overlays, single-tick frames and when any required mixin did not apply.
Desktop activation defaults off.

Client commands allow further comparisons without a new APK or JAR:

```text
/wachilandatlas status
/wachilandatlas off
/wachilandatlas on
```

These switches are session-only. Removing the JAR restores the original route;
`-Dwachiland.atlasBatch=false` is a launch-time switch if JVM arguments are exposed.
For a controlled repeat, use the same scene/settings and diagnostic configuration
with batching off. Do not compare a loading scene with an idle scene.

## Build and licensing

From this directory, with JDK 21 and network access for initial dependencies:

```bash
./gradlew assemble --no-daemon
```

The distributable is `build/libs/wachiland-atlas-batch-neoforge-1.21.1-0.1.0-experimental.jar`.
`assemble` does not run tests. Compilation and static inspection passed; no
Minecraft startup, mixin runtime, visual equivalence or FPS win has been
validated locally. Those gates are pending the phone trial.

GPL-3.0-only; see COPYING. The corresponding source ZIP includes this standalone
build, wrapper and research record. It does not include Minecraft or NeoForge
binaries. Keep this candidate separate from production until the gates pass.
