# BootOptim v0 configuration identity contract

This document applies only to the AppCDS **preflight identity** used by the `bootoptim-v0` interposer. It does not rewrite, repair, copy, or otherwise mutate instance configuration. AppCDS remains plan-only in this prototype.

## Default rule

Every file under `config/`, `defaultconfigs/`, `kubejs/`, and `scripts/` is hashed byte-for-byte. Parsing failure never removes an input from identity: it falls back to the raw file bytes. Symlinks/reparse points and unsupported file types remain fail-closed exactly as before.

## Exact Java Properties exceptions

Only these instance-relative paths are eligible for canonical identity:

- `config/asyncparticles/asyncparticles-mixin.properties`
- `config/fabric/indigo-renderer.properties`
- `config/iris.properties`
- `config/drippyloadingscreen/early_window_reference.properties`

The parser deliberately accepts only an ASCII, one-physical-line-per-entry subset written as `key=value`. It ignores blank/comment lines, rejects escapes, leading-whitespace syntax, duplicate keys, missing `=`, whitespace in keys, and non-ASCII. Rejected input is hashed raw. Accepted entries are sorted by decoded key/value text before hashing. The purpose is identity only; the source file is never rewritten.

This is narrower than the Java Properties grammar on purpose. OpenJDK 21 `Properties.load` skips comments and blank lines and builds key/value mappings, while `Properties.store` writes a generated date comment. Unsupported legal Java Properties forms are therefore intentionally raw rather than approximately parsed. The relevant 1.21.1 mod sources independently confirm these files are loaded/stored through `java.util.Properties`: AsyncParticles loads and rewrites its mixin config on startup, Fabric API Indigo 1.21.1 loads the exact `config/fabric/indigo-renderer.properties` path and stores it again, and Iris 1.21.1 loads/stores `iris.properties`. Therefore comments and textual entry order do not change their loaded property mapping, while every accepted key/value remains in the canonical identity.

- https://github.com/openjdk/jdk21u/blob/92fcfff6486d67144c32c67ca4678a7e0b572f4e/src/java.base/share/classes/java/util/Properties.java
- https://github.com/Harveykang/AsyncParticles/blob/91d4223a48da8390776ef47caee47cbacb22571b/common/src/main/java/fun/qu_an/minecraft/asyncparticles/client/config/AsyncParticlesMixinConfig.java
- https://github.com/FabricMC/fabric-api/blob/1.21.1/fabric-renderer-indigo/src/client/java/net/fabricmc/fabric/impl/client/indigo/Indigo.java
- https://github.com/IrisShaders/Iris/blob/1.21.1/common/src/main/java/net/irisshaders/iris/config/IrisConfig.java

For Drippy Loading Screen 3.1.2 / Minecraft 1.21.1, the official `v3-1.21.1` source shows the file is produced with `java.util.Properties`; `timestamp` is assigned from `System.currentTimeMillis()`. The early-window provider only persists the reference after resolving the effective width/height. Its in-game editor reads the timestamp into a record, but there is no subsequent `timestampMillis` use; width and height are the reference values. Therefore only the exact `timestamp` key is removed from identity, and only when the file contains exactly numeric `width`, `height`, and `timestamp` fields. Width/height remain identity inputs.

- https://github.com/MINEZ/DrippyLoadingScreen/blob/v3-1.21.1/earlywindow/src/main/java/de/keksuccino/drippyloadingscreen/earlywindow/window/sync/EarlyWindowReferenceSizeStore.java
- https://github.com/MINEZ/DrippyLoadingScreen/blob/v3-1.21.1/earlywindow/src/main/java/de/keksuccino/drippyloadingscreen/earlywindow/window/DrippyEarlyWindowProvider.java
- https://github.com/MINEZ/DrippyLoadingScreen/blob/v3-1.21.1/neoforge/src/main/java/de/keksuccino/drippyloadingscreen/neoforge/EarlyLoadingEditorScreen.java

## MoreCulling stays raw

`config/moreculling.toml` is **not** canonicalized. The 1.21.1 source deserializes `modCompatibility` into `Object2BooleanOpenHashMap`, and the config UI iterates `object2BooleanEntrySet()` directly to construct its visible compatibility entries. Consequently map iteration order is observable in the UI; without the two exact physical before/after files, the reported order-only hypothesis cannot be proven safe. Generic TOML sorting or reserialization is prohibited here, so any MoreCulling byte change continues to invalidate the plan.

- https://github.com/FxMorin/MoreCulling/blob/1.21.1/src/main/java/ca/fxco/moreculling/config/MoreCullingConfig.java
- https://github.com/FxMorin/MoreCulling/blob/1.21.1/src/main/java/ca/fxco/moreculling/MoreCulling.java
- https://github.com/FxMorin/MoreCulling/blob/1.21.1/src/main/java/ca/fxco/moreculling/config/ModMenuConfig.java

## Validation boundary

Tests require generated Properties comments and the Drippy timestamp to be identity-neutral only on the exact authorized paths; effective values remain invalidating. Unknown paths, lookalike nested paths, malformed/escaped/duplicate Properties syntax, malformed TOML, and MoreCulling ordering stay raw. Plan tests also retain invalidation for Java, classpath, mods, resource-pack selection, and effective configuration.

A hosted test pass establishes only implementation behavior. The next physical gate remains two otherwise-identical `plan` launches producing `MATCH`, followed by verification of stock fallback. Because MoreCulling intentionally remains raw, a repeated byte rewrite there is a **NO-GO blocker** until its exact delta can be attributed safely. No AppCDS archive generation/consumption or TTMM claim is authorized by this work.
