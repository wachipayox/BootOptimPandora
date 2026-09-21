# Agent 202 — libraries launch-fast policy

## Product policy

Normal GUI **Start never verifies, hashes, downloads or repairs game libraries**. Library
corruption is intentionally allowed to surface later as the JVM/Minecraft/loader failure it
would naturally cause. This trades pre-Java diagnosis for lower Start→Java launcher work.

The explicit **Repair game files** action is the only normal UI authority for the stock strong
library path: SHA-1 validation where metadata supplies a digest, download on missing/mismatch,
download size/hash validation, Forge/NeoForge installer-library preparation, and publication of
the instance game-files state only after success.

## Transactional state

Each instance owns `.bootoptim/game-files-state-v1.json`.

- `published`: Start may take the launch-fast library path.
- `incomplete`: Start stops with “Installation incomplete; use Repair game files”. It never
  repairs automatically.
- no marker: legacy instances created before this policy are treated as published **without any
  library scan**. This is a compatibility rule, not an integrity assertion.
- unreadable/corrupt/unknown marker: Start fails closed to the same Repair instruction.

New instances and Minecraft/loader/loader-version changes publish `incomplete` first. Repair
writes `repair-in-progress` before touching libraries and publishes `published` only after the
strong route returns success. Cancellation, network failure, hash mismatch after download, crash,
or launcher termination therefore leaves the installation incomplete.

The current Pandora architecture has no separate game-dependency install transaction. For a new
instance, the first explicit **Repair game files** operation is therefore also the provisioning
operation. Start itself never becomes that installer.

## Launch-fast boundary

The fast route still parses the resolved in-memory library artifact list and rejects illegal
relative paths. It does **not** stat, open, enumerate, hash, create parent directories, or contact
library URLs. Forge/NeoForge launch-fast also skips installer `.sha1` fetches, library mirror
lookups, embedded Maven-library extraction, library SHA-1 checks, and library writes. Missing
installer/library files fail naturally when later loader/JVM code opens them.

Unchanged, deliberately outside this change:

- Minecraft/version JSON and loader metadata resolution remains stock.
- Java runtime verification/download remains stock on Start.
- assets index and asset object verification/download remain stock; PR #45 is not composed here.
- log configuration behavior remains stock.
- AppCDS, incremental identity, persistent layout, login, argv/classpath/module-path ordering,
  parallel-launch policy and graphical work are untouched.
- Forge post-processor output checks remain stock; they are not game-library verification. Repair
  may resolve the Java runtime when Forge/NeoForge processor construction requires it, exactly as
  the pre-existing loader setup path did. Repair does not verify assets.

## Residual risk accepted by product

A library can be absent or corrupt while the marker is published. Start deliberately does not
discover this. Java, the loader or Minecraft can fail after spawn/open instead of Pandora reporting
a pre-Java integrity error. The marker records only whether Pandora's own install/repair operation
completed; it is not a cryptographic statement about current library contents.

## Focused tests

- published/legacy state permits launch without a library scan;
- missing and corrupt libraries map to classpath paths with no file or network access in LaunchFast;
- strong repair detects a corrupt SHA-1 and replaces it from a test HTTP origin;
- interrupted/update-in-progress state blocks Start without auto-repair;
- cancelled Repair leaves `repair-in-progress` and never publishes;
- corrupt marker fails closed;
- diff excludes assets/AppCDS sources.

## Physical smoke protocol

Use the exact packaged Windows artifact and the same existing Start→Java marker/probe.

1. Run Repair once and confirm the marker is `published`.
2. Baseline a published Start and record **click/Start→Java only**. Do not read Minecraft logs while
   the run is in progress.
3. Remove one non-installer game library, Start again, and confirm there is no “Verifying integrity
   of game libraries”, no library SHA-1 read and no library network request; record the later
   Java/loader failure separately.
4. Restore via **Repair game files** and confirm the missing library is downloaded and the marker is
   republished.
5. Corrupt one library in place; repeat Start (no repair/network), then Repair (detect + replace).
6. Start Repair and cancel it; confirm marker remains incomplete and Start instructs Repair.
7. Change Minecraft/loader version and interrupt before Repair; confirm Start stays blocked and does
   not silently provision.
8. Report Repair duration separately from Start→Java. Java→menu/TTMM is a different metric and no
   savings claim should be inferred from this launcher-phase change.
