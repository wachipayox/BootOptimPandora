# Experimental artifact evidence

Local build 2026-10-09: JDK 21.0.4.7, Gradle 9.2.1, ModDevGradle 2.0.144,
NeoForge 21.1.248. `gradlew assemble --no-daemon --console=plain` succeeded.
No test, client-startup or benchmark tasks were run.

Artifact: `wachiland-atlas-batch-neoforge-1.21.1-0.1.0-experimental.jar`,
29,740 bytes; SHA-256:
`c5964d804b6f855e62c6e0696ed1b67a4f09bf60a8f877b756179e714c92ae7f`.

Static archive inspection found all seven configured mixins, their plugin,
helper and mod entrypoint, NeoForge metadata, access transformer and GPL license.
No Minecraft/NeoForge/Mojang classes were bundled. Generated merged-artifact
signature/bytecode inspection confirms SpriteContents.upload, TextureAtlas
animation/lifecycle methods, NativeImage.close, GameRenderer.render and the
managed GL methods used by the injections. Minecraft.runTick contains
Timer.advanceTime(JZ)I followed by RenderSystem.clear(IZ)V, matching the anchors.

These are compilation and static evidence only. They do not prove Mixin runtime
application, callback equivalence, safe custom direct GL calls, phone startup,
correct visuals or improved FPS. The first physical acceptance result is pending.
The source ZIP additionally contains the assemble log and artifact entry list.
