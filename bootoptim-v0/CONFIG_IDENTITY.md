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

## MoreCulling exact-map identity exception

`config/moreculling.toml` is never rewritten, copied, repaired or normalized on disk. The preflight only accepts the exact observed MoreCulling 1.0.8 shape: ASCII/LF input; an otherwise raw byte-sensitive prefix; exactly one final `[modCompatibility]` table; and non-duplicate lowercase/digit/underscore keys with literal `true` or `false` values. It fingerprints that final table as an unordered boolean mapping. The raw order remains in the file, so the MoreCulling UI retains precisely its stock order for that run.

This recognizes the physically observed `tfmg`/`ldlib2` permutation without excluding values: a changed boolean, key addition/removal, preceding byte, comment, extra table, CRLF, whitespace variation, escape or malformed input falls back to raw hashing and invalidates the plan. This is identity comparison only, not a generic TOML parser or canonicalizer.

The two physical copies from the test pack were equal except for that table's `tfmg = true` / `ldlib2 = true` ordering. The MoreCulling UI does iterate the map directly, which is why this exception must not reorder the file itself.

- https://github.com/FxMorin/MoreCulling/tree/v1.0.8

## Validation boundary

Tests require generated Properties comments and the Drippy timestamp to be identity-neutral only on the exact authorized paths; effective values remain invalidating. Unknown paths, lookalike nested paths, malformed/escaped/duplicate Properties syntax, malformed TOML, and every MoreCulling shape outside the exact final boolean-map subset stay raw. Plan tests also retain invalidation for Java, classpath, mods, resource-pack selection, and effective configuration.

A hosted test pass establishes only implementation behavior. The next physical gate remains two otherwise-identical `plan` launches producing `MATCH`, followed by verification of stock fallback. The proven MoreCulling ordering permutation is neutral only through the exact-map exception above; all other rewrites remain a **NO-GO blocker**. No AppCDS archive generation/consumption or TTMM claim is authorized by this work.
