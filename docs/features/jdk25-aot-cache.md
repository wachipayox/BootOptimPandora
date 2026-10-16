# JDK 25 AOT cache (per instance)

Status: implemented for Wachiland global profiles on PC with Java 25 (PR #104). Research:
BootOptim `docs/research/jdk25-aot-cache-reopen-2026-10-15.md` (#364) and
`docs/research/jdk25-aot-cache-pandora-2026-10-16.md`.

## What it does

The JDK 25 AOT cache (JEP 483/514) stores loaded and linked classes and JIT profiles. On the
reference PC (Java 25.0.1, startup to `main_menu`, medians) a start with the cache is 4–6 s faster
than one without it (rev 3: 55.5 → 50.5 s; next revision, retrained: −4.3 s). The cache is about
300 MB. Measurements: BootOptim `docs/research/jdk25-aot-cache-pandora-2026-10-16.md`.

It is built in **two steps**, so the game closes about as fast as without it:

1. **Record**, during a normal game: the first Start of an instance with no valid cache runs with
   `-XX:AOTMode=record -XX:AOTConfiguration=<instance>/aot-cache/training/train-<nonce>.aotconf`.
   When the game closes, the JVM writes the training profile (≈ 305 MB, ≈ 17 s on the reference
   PC) and exits. The profile is kept only if the game exited with code 0; `run.json` then stores
   its size.
2. **Create**, a launcher job: right after that, the launcher runs the same Java with the same VM
   arguments (saved in `run.json` when the record run started) plus `-XX:AOTMode=create`. It turns
   the profile into the cache (≈ 15 s, ≈ 1 GB RAM, one core on the reference PC). The output is
   renamed to `<key16>.aot` and named in `state.json` only after exit code 0.
3. Later Starts run with `-XX:AOTCache=<instance>/aot-cache/<key16>.aot`.

If the create step is cancelled, the launcher is closed or killed, or the PC goes off, the profile
stays. The create step resumes the next time the launcher opens, or after the next game of that
instance closes, **without recording again**. A Start while a profile waits runs without the cache
and does not record. Recording is repeated only if the profile is missing or has another size, or
the key changed.

The one-step way (`-XX:AOTCacheOutput`, used by the first version of this PR) gives the same cache
and gain but keeps the game process alive ≈ 35 s after the game closes; the two-step way moves half
of that into a job the player can see and cancel.

## What the player sees

- While the JVM writes the profile and while the create step runs, the instance's button shows
  **Optimizing launch** instead of Start/Kill. If the launcher was hidden or closed to the
  background when the game closed, it comes back.
- Clicking it opens a dialog with an approximate percentage, a **?** that explains what is happening,
  **Cancel optimization** (postpones it until the next launch; a profile already written is kept)
  and **Hide**.
- Closing the launcher while instances optimize asks first: confirming cancels their optimization
  until the next launch (the profiles that are written are kept).
- **Instance settings → "Optimize launch (Java 25 AOT cache)"**: on by default, with a **?** that
  explains it, its disk use (~300 MB) and that the first game takes a little longer. Turning it off
  deletes `aot-cache/` (when the instance is idle) and cancels a running optimization.

### Progress estimate (approximate)

Java reports no progress. The launcher estimates it once a second:

- record part: bytes of the profile written so far against the last profile size of this instance
  (305 MB the first time), or the elapsed time against the last write time (17 s the first time),
  whichever is further;
- create part: elapsed time against the last create time (15 s the first time);
- the two parts are weighted by the last measured durations and the total is capped at 99 %.

The last sizes and durations live in `aot-cache/timings.json` and survive a key change.

## Automatic turn-off

The option turns itself off (with a `[WBOMBO-COMPAT] FALLBACK … reason=option-turned-off (…)` line
and the reason shown under the option) **only when Java itself rejects the cache**:

| Case | Detected by |
|---|---|
| AOT error at start (`record` or `use`): the JVM exits with an error in the first 15 s and the immediate retry **without** any AOT flag starts fine (lives 15 s or exits with 0) | retry probe |
| The create step exits with an error, or exits 0 without a usable output | create exit |

It does **not** turn off for a Stop, a game closed by the player, a cancelled optimization, a
closed or killed launcher, or a game crash after the first 15 s (mods). If the retry without the
cache also fails early, the cache was not the cause and the option stays on (the Java 21 fallback
of #103 then applies as usual). The player can turn the option on again; that clears the reason.

Not detected: with `AOTMode=auto` (the default used here) a cache that Java ignores (truncated,
other JDK, other classpath) prints a message and the game starts normally without it. That costs
nothing, so it does not turn the option off; the next key change retrains.

`WBOMBO_AOT_CACHE=force-error` adds an invalid `-XX:AOTMode` value to a record/use start so the JVM
refuses to start: it exercises the automatic turn-off (testing only).

## When it applies

Only when **all** hold; otherwise the start is a plain start:

- the instance's applied revision belongs to a `profile_wachiland-*` global profile;
- the instance's option is on;
- the game runs on Java 25 (`release` file says 25; Mojang's `java-runtime-epsilon`);
- no custom `jvm_binary`, no `FORCE_EXTERNAL_JAVA`, no sandbox, no wrapper command;
- the player's JVM flags do not set `-XX:AOT*`, `-Xshare`, `-XX:SharedArchiveFile` or
  `-XX:ArchiveClassesAtExit`, and the legacy `BOOTOPTIM_LAUNCH_INTERPOSER` AppCDS is off;
- every launch argument is valid Unicode (it is saved in `run.json` for the create step);
- `WBOMBO_AOT_CACHE=off` is not set (support switch);
- recording also needs 2 GiB free on the instance's volume and no other process of the instance
  (game or create step) running.

Android is not touched (the launcher fork has its own code).

## One cache per instance, with a key

Everything lives in `<instance>/aot-cache/` (so deleting an instance deletes its cache, and
duplicating one does not copy 300 MB: `aot-cache` is not part of the duplicate). Two instances
never share a cache; playing another instance does not touch this one's.

```text
<instance>/aot-cache/
  state.json         the valid cache: key, file name, size (written last, atomically)
  <key16>.aot        the valid cache
  timings.json       last profile size, write and create durations (progress estimate only)
  training/          run.json + train-<nonce>.aotconf (+ cache-<nonce>.aot while creating)
```

`state.json` names the valid cache. It is used only if its key equals the current key. The key is
a SHA-256 over:

| Input | Source | Cost at Start |
|---|---|---|
| Exact JDK build | hash of the runtime's `release` file + size of `lib/modules` | 2 small reads |
| Profile revision | applied revision pin (profile id, revision id, manifest hash) | already read |
| Mod list | hash of the committed profile entries under `mods/` (path + stored content hash) | none: no scan |
| Classpath | path, size and mtime of each library jar (the JVM checks the same) | ~100 `stat` |
| VM arguments | every argument before the main class except `-D…` and the cache flags (memory, GC, `-XX`, modules) | none |

If the key changes, only that instance retrains (the old file and a profile for the old key are
deleted when the next Start notices). A mod file changed by hand inside the instance is **not** in
the key (Pandora does not scan the mods folder at Start); the JVM still compares class bytes and
ignores the entries that no longer match, so the result is a lower hit rate, not a wrong class.
Delete `aot-cache/` to force a retrain.

## Fallback

| Situation | What happens |
|---|---|
| Cache missing, truncated, empty, other JDK or classpath | The JVM runs in `AOTMode=auto`: it ignores the cache (a message on its output) and starts normally. |
| Record run killed, game crashed, PC powered off while writing the profile, launcher killed | No `recorded` in `run.json`: `training/` is deleted at the next Start or launcher start. A partial profile is never used. |
| Player presses Stop during a record run | The game gets up to 60 s to close normally (instead of 3 s) and the JVM up to 10 min to finish writing the profile; then the profile is kept as usual. Pressing Stop again kills it (profile lost). |
| Create step cancelled, launcher closed or killed during it | Partial output deleted, profile kept, create resumes later. |
| A run **with** an AOT flag exits with an error in the first 15 s | The cache and any training are deleted and the same Start is repeated without any AOT flag; see *Automatic turn-off*. This does **not** trigger the Java 21 fallback of #103. |

Log lines (always the fixed prefix, filter the launcher log for `[WBOMBO-COMPAT]`):

```text
[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism=jdk25-aot-cache mod=java@25.0.1+8-LTS mode=record key=0123456789abcdef
[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism=jdk25-aot-cache mod=java@25.0.1+8-LTS mode=recorded key=… profile_mib=294 write_seconds=17.2
[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism=jdk25-aot-cache mod=java@25.0.1+8-LTS mode=created key=… size_mib=289 create_seconds=15.1
[WBOMBO-COMPAT] ACTIVE project=Pandora mechanism=jdk25-aot-cache mod=java@25.0.1+8-LTS mode=use key=0123456789abcdef size_mib=289
[WBOMBO-COMPAT] FALLBACK project=Pandora mechanism=jdk25-aot-cache mod=java@25.0.1+8-LTS reason=custom-java-binary
[WBOMBO-COMPAT] FALLBACK project=Pandora mechanism=jdk25-aot-cache mod=java@25 reason=option-turned-off (create-failed (exit Some(1) after 3s))
```

## Disk and cleanup

About 300 MB per instance for the cache, plus ≈ 305 MB for the profile between the two steps (≈ 600
MB while creating). Limits: one valid cache per instance (older keys are deleted), recording needs
2 GiB free, and a cache of a deleted instance goes away with the instance directory. Turning the
option off deletes `aot-cache/`.

## `jvm_flags` and Windows paths

Pandora used to split the player's JVM flags with POSIX shell rules, so on Windows
`-Xlog:...file=C:\logs\x.log` arrived as `C:logsx.log`. On Windows a backslash is now a plain
character and only quotes group words (`"…"` and `'…'`); other platforms keep the shell rules.
The wrapper command uses the same splitter. Forward slashes always worked and remain the
recommended form in documentation.
