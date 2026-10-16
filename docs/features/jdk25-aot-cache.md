# JDK 25 AOT cache (per instance)

Status: implemented for Wachiland global profiles on PC with Java 25. Research:
BootOptim `docs/research/jdk25-aot-cache-reopen-2026-10-15.md` (#364) and
`docs/research/jdk25-aot-cache-pandora-2026-10-16.md`.

## What it does

The JDK 25 AOT cache (JEP 483/514) stores loaded and linked classes and JIT profiles. On the
reference PC (Java 25.0.1, Beta rev `7d5b9e50…`, startup to `main_menu`, medians of 4) a start
with the cache is about 5 s faster than one without it (55.5 → 50.5 s, −9 %, ranges do not
overlap); on the next real revision, retrained, −4.3 s (−7.5 %, 16 runs). The cache is about 300 MB. Measurements: BootOptim
`docs/research/jdk25-aot-cache-pandora-2026-10-16.md`.

Pandora trains the cache **while the player plays**, with no separate start:

1. First Start of an instance with no valid cache: the game runs with
   `-XX:AOTCacheOutput=<instance>/aot-cache/training/train-<nonce>.aot`. When the game closes by
   itself the JVM assembles the cache before it exits (35–43 s after the title screen in the
   measured sessions; the process stays alive meanwhile and the launcher cannot quit).
2. The launcher promotes the file (rename) only if the game exited with code 0, the JVM removed
   its temporary `*.aot.config` file (it does that only when the assembly finished) and the file
   is at least 1 MiB.
3. Later Starts run with `-XX:AOTCache=<instance>/aot-cache/<key16>.aot`.

## Why one step (and not record + create)

Both ways were measured on the reference PC (BootOptim research entry above). They give the same
cache and the same gain; the one-step way has fewer parts.

| | One step (`-XX:AOTCacheOutput`), chosen | Two steps (`AOTMode=record`, then `AOTMode=create`) |
|---|---|---|
| Gain at the main menu | −5.0 to −6.0 s | −5.7 s |
| Training run, to the menu (control 55.5–57.7 s) | 64.5–69.5 s | 63.1–72.9 s |
| Extra time to close the game | about +35 s (config write 17 s + assembly 18 s) | about +18 s, then a 15 s job later |
| Later job | none | `create` at the next launcher start: 15 s, 1 GB RAM, one core; it did not slow a simultaneous start |
| Parts that can fail | one process; the JVM does the assembly | saved JVM arguments, a background job and its state |

The extra 17 s at close does not justify a launcher-side job, a saved copy of the launch
arguments and a state machine across two sessions. Reopen it if real play sessions make the
one-step close much longer than measured here (the measured session ended at the title screen).

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
