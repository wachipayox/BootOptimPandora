# Android discrete atlas upload coalescing: experimental candidate

2026-10-10 disposition update: the [bounded physical attribution run](VIVO-ATLAS-ATTRIBUTION-2026-10-10.md)
observed 5068 original uploads and zero repeated keys. The 0.1.2 diagnostic
therefore defaults batching off on Android as well as desktop. Earlier default
enablement statements below describe the initial experimental artifact only.

Follow-up: [first direct Vivo ADB trial](VIVO-ATLAS-BATCH-ADB-2026-10-10.md)
confirms startup/world entry and applied hooks, but records zero queue replacements.
It does not establish a performance win. The draft candidate requires further
coverage attribution and a matching control before promotion.

## Authority and origin

Based on [physical capture 11](VIVO-FRAME-CAPTURE-11-2026-10-09.md), not on
unavailable TextureManager counters or an assumed driver defect. The matched
120.818-second world window contains 2329 client ticks / 412 frames, 57.111s
inclusive GL texture-upload wall time and 111 of 112 native upload stack tops
through discrete AnimatedTexture.uploadFrame. Overlapping durations are not
additive, and Java GL wall time is not GPU execution time.

User authorized implementing the proposed optimization on 2026-10-09. Isolated
launcher branch `codex/android-atlas-upload-batch-20261009` starts from
47743e8ff0fa3e28b068866c352cbcb71551cb87. Mod source is under
`mobile/android/mods/atlas-batch`; the launcher/engine APK is unchanged.
BootOptim integration was refreshed read-only to
058ac544aef11c0c10dc32e0aeacba2e39176d40. This is not a BootOptim promotion;
the user's instruction to leave its active work alone remains in force.

Source authority is the mapped NeoForge 21.1.248 Minecraft 1.21.1 artifact.
AsyncParticles source audit 89317906 shows its TextureManager wrapper queues
the original tick in some configurations. This candidate does not replace,
skip or move that wrapper/task; only atlas work inside the eligible render-frame
scope is coalesced. The exact phone mod binary/source match remains unverified.
MobileGlues is unchanged, and no GPU vendor-specific mechanism is introduced.

## Mechanism and lifetime

- Only in-world frames with more than one scheduled logical client tick,
  no screen/overlay, and render-thread ownership are eligible. Every ticker
  and logical tick runs normally. Menus and single-tick frames stay stock.
- Exact vanilla TextureAtlas / SpriteContents objects and their original mip
  image arrays are required. Binding/unit/coordinate checks guard enqueueing.
  The queue key is sprite identity + atlas identity + unit + atlas coordinates.
- The last dirty discrete upload replaces previous uploads of that same key.
  It is replayed through the original SpriteContents.upload, including mipmaps,
  before Minecraft's clear/render event boundary and GameRenderer.render.
  A transition on an earlier tick is not lost when the last tick has no upload.
- Interpolation uploads remain immediate; a later interpolation supersedes a
  queued discrete upload of that sprite. No interpolated buffer is retained.
- Queue entries hold Java image references for the current frame, not raw native
  pointers or copied pixel buffers. Close/mipmap/atlas disposal ends or invalidates
  the scope; worker invalidation performs no GL work. Unknown shapes and queue
  overflow (2048 entries) flush and use stock for the rest of that frame.
- Replay restores managed active-unit/bindings and four GL pixel-unpack fields.
  Pixel-unpack restoration matters when a flush occurs in another upload's call:
  its row length/offset/alignment may already have been configured.
- Managed draw/read/copy/blit/texture disposal calls flush pending updates. Frame
  end also flushes on exceptions. No GL work is moved to a worker thread.
- An optional mixin plugin gates activation on successful application of all
  seven mixins; failed/partial injection leaves batching ineligible. Runtime
  `hooks_ready=true` must be confirmed, not inferred from compilation.
- Loaded Kerria disables batching because its upload-cache/PBO callbacks have a
  separate lifecycle, even if its runtime option is off. No callback ownership
  claim is made for that configuration.

## Instrumentation and kill switches

One cumulative log record every 30 seconds reports effective enablement,
compatibility, hooks_ready, eligible multi-tick frames, deferred submissions,
replacements, submitted queue entries, flushes, queue length and overflow
fallbacks. These are software submission counts, not pixel bandwidth or GPU time.
`replaced` also includes a discrete upload superseded by interpolation.
They do not prove a frame-time win; there is no precise queue-allocation CPU
measurement yet. The queue and per-flush collections add temporary allocations.

Commands `/wachilandatlas on|off|status` support repeated local trials without
another transfer. Commands do not persist. Android JVM os.version enables the
candidate by default; desktop defaults off. `-Dwachiland.atlasBatch=false` disables
at launch. Removing the JAR restores stock. No settings/config are rewritten.

## Residual semantic risks and decision gate

This candidate is **not production**. Mod mixins attached to each upload can have
per-upload side effects; replacing intermediate uploads changes their frequency.
The known Kerria owner is excluded, but arbitrary custom callbacks are not proven
safe. Direct LWJGL draws/readbacks or pixel-store/binding mutations that bypass
Minecraft's managed wrappers are not universally intercepted. Interleaved custom
atlas edits/overlapping sprites are another unproven case. Image close on a worker
invalidates rather than submits that generation; illegal concurrent lifetime
mutations are not made safe by retaining a Java reference.

On the audited stock path, only the final animation state is visible between
render boundaries. That equivalence argument needs physical confirmation with
the actual enabled pack, AsyncParticles, terrain, particles, menus and reload.
Do not promote solely because a JAR compiles, counters decrease or CI is green.

Phone gate: same APK 0.1.11, world/scene, MobileGlues, packs, distances, heap and
thermal mode; arm the same FPS capture, allow JEI setup to finish, collect a
complete world window and exit normally. Confirm startup/mixins, positive queue
replacements, correct animations/particles and no new crashes. Then compare an
off run under matching conditions. A later resource reload gate must account for
the existing low-memory reload failure; a death during reload is not automatically
attributed to this candidate. Do not request reload during the first FPS run.

Acceptance needs improved comparable frame/presentation rate or reduced measured
upload cost without visible regression, excessive allocation, thermal regression
or new lifetime errors. There is no gain claim or promise that this fixes all
performance problems. Driver/native residency and memory pressure remain separate.

## Build disposition

Gradle 9.2.1 / ModDevGradle 2.0.144 / JDK 21, `assemble` only. Compilation and
artifact metadata/class/static target-signature inspection are the local evidence.
No runtime tests were added/run; Minecraft startup, Mixin application, physical
render equivalence and performance remain pending. Distributed JAR and matching
GPL source ZIP are experimental artifacts for the user phone gate.
