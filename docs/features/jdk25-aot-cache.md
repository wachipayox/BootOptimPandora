# JDK 25 AOT cache (per instance)

Status: implemented for Wachiland global profiles on PC with Java 25. Research and
numbers: BootOptim `docs/research/jdk25-aot-cache-reopen-2026-10-15.md` (#364) and
`docs/research/jdk25-aot-cache-pandora-2026-10-16.md`.

## What it does

The JDK 25 AOT cache (JEP 483/514) stores loaded and linked classes and JIT profiles. A start
with a cache reached the main menu about 9 s sooner on the reference PC (Beta rev
`7d5b9e50…`, 58.7 → 49.5 s). The cache is about 300 MB.

Pandora trains the cache **while the player plays**, with no separate start:

1. First Start of an instance with no valid cache: the game runs with
   `-XX:AOTCacheOutput=<instance>/aot-cache/training/train-<nonce>.aot`. When the game closes by
   itself the JVM assembles the cache before it exits (about 2 min, the process stays alive).
2. The launcher promotes the file (rename) only if the game exited with code 0, the JVM removed
   its temporary `*.aot.config` file (it does that only when the assembly finished) and the file
   is at least 1 MiB.
3. Later Starts run with `-XX:AOTCache=<instance>/aot-cache/<key16>.aot`.

## When it applies

Only when **all** hold; otherwise the start is a plain start:

- the instance's applied revision belongs to a `profile_wachiland-*` global profile;
- the game runs on Java 25 (`release` file says 25; Mojang's `java-runtime-epsilon`);
- no custom `jvm_binary`, no `FORCE_EXTERNAL_JAVA`, no sandbox, no wrapper command;
- the player's JVM flags do not set `-XX:AOT*`, `-Xshare`, `-XX:SharedArchiveFile` or
  `-XX:ArchiveClassesAtExit`, and the legacy `BOOTOPTIM_LAUNCH_INTERPOSER` AppCDS is off;
- `WBOMBO_AOT_CACHE=off` is not set (support switch);
- training also needs 2 GiB free on the instance's volume and no other process of the instance
  running.

Android is not touched (the launcher fork has its own code).

## One cache per instance, with a key

Everything lives in `<instance>/aot-cache/` (so deleting an instance deletes its cache, and
duplicating one does not copy 300 MB: `aot-cache` is not part of the duplicate). Two instances
never share a cache; playing another instance does not touch this one's.

`state.json` names the valid cache. It is used only if its key equals the current key. The key is
a SHA-256 over:

| Input | Source | Cost at Start |
|---|---|---|
| Exact JDK build | hash of the runtime's `release` file + size of `lib/modules` | 2 small reads |
| Profile revision | applied revision pin (profile id, revision id, manifest hash) | already read |
| Mod list | hash of the committed profile entries under `mods/` (path + stored content hash) | none: no scan |
| Classpath | path, size and mtime of each library jar (the JVM checks the same) | ~100 `stat` |
| VM arguments | every argument before the main class except `-D…` and the cache flags (memory, GC, `-XX`, modules) | none |

If the key changes, only that instance retrains (the old file is deleted when the next Start
notices). A mod file changed by hand inside the instance is **not** in the key (Pandora does not
scan the mods folder at Start); the JVM still compares class bytes and ignores the entries that
no longer match, so the result is a lower hit rate, not a wrong class. Delete `aot-cache/` to
force a retrain.

## Fallback

| Situation | What happens |
|---|---|
| Cache missing, truncated, empty, other JDK or classpath | The JVM runs in `AOTMode=auto`: it ignores the cache (a message on its output) and starts normally. Checked on 25.0.1: a truncated file reports "The AOT cache has been truncated", exit code 0. |
| Training killed, game crashed, PC powered off, launcher killed | No promotion. `training/` is deleted at the next Start (or promoted if it is provably complete and for the same key). A partial file is never used. |
| Player presses Stop during training | The 3 s kill timer waits (once, up to 10 min) while the JVM is assembling; otherwise the training is discarded. |
| A run **with** the cache exits with an error in the first 15 s | The cache is deleted, the failure is recorded, and the same Start is repeated without any AOT flag. This does **not** trigger the Java 21 fallback of #103; if the retry fails too, #103 applies as usual. |
| Two failures for the same key | The mechanism is disabled for that key until the key changes (delete `aot-cache/failure.json` to try again). |

Log lines (always the fixed prefix, filter the launcher log for `[WBOMBO-COMPAT]`):

```text
[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism=jdk25-aot-cache mod=java@25.0.1+8-LTS mode=use key=0123456789abcdef size_mib=304
[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism=jdk25-aot-cache mod=java@25.0.1+8-LTS mode=train key=0123456789abcdef
[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism=jdk25-aot-cache mod=java@25.0.1+8-LTS mode=trained key=… size_mib=304 seconds_since_start=…
[WBOMBO-COMPAT] FALLBACK project=Pandora mechanism=jdk25-aot-cache mod=java@25.0.1+8-LTS reason=custom-java-binary
```

## Disk and cleanup

About 300 MB per instance, plus up to ~1 GB transient during training. Limits: one valid
cache per instance (older keys are deleted), training needs 2 GiB free, and a cache of a
deleted instance goes away with the instance directory. There is no global cap and no UI yet; to
free the space delete `<instance>/aot-cache/`.

## `jvm_flags` and Windows paths

Pandora used to split the player's JVM flags with POSIX shell rules, so on Windows
`-Xlog:...file=C:\logs\x.log` arrived as `C:logsx.log`. On Windows a backslash is now a plain
character and only quotes group words (`"…"` and `'…'`); other platforms keep the shell rules.
The wrapper command uses the same splitter. Forward slashes always worked and remain the
recommended form in documentation.
