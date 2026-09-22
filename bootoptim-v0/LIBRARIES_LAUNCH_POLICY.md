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

New instances start as `incomplete`. Their creation path performs a dedicated **provision-missing**
transaction before publishing: existing library files are trusted by existence only, missing
libraries are downloaded, and only newly downloaded bytes are checked against advertised size/SHA-1.
There is no cryptographic pass over already-present libraries. A standalone instance publishes only
after that provisioning succeeds. A new content/modpack install keeps the same marker incomplete
through the final content copy and publishes only after the entire install path completes. If an
existing-instance content install changes the loader, its provision-missing dependency step is
awaited inside that install operation before publication rather than being left as background work.

Launcher-to-launcher imports keep their pre-policy compatibility behavior and are outside this
game-library transaction marker. Minecraft/loader/loader-version changes persist `incomplete`
**before** mutating dependency identity; if that state write fails, the change is rejected. After a successful
identity mutation Pandora schedules the same provision-missing transaction against the new identity.
It publishes only if the instance still has exactly that Minecraft/loader/loader-version identity.
Each incomplete transition also receives a process-local monotonically increasing generation token.
Publication is a serialized compare-and-set on both the expected reason and that exact generation,
so even a repeated A→B→A update or two same-kind Repairs cannot let an older completion publish a
newer transaction. Provisioning
failure leaves the update incomplete and Start directs the user to Repair.

Repair writes `repair-in-progress` before touching libraries and publishes `published` only after
the strong route returns success and no cancellation is pending. Publication is compare-and-set;
the modal is finished immediately after a successful final transition, and a cancellation racing
that transition is written back to an incomplete marker. Cancellation, network failure, hash
mismatch after download, crash, or launcher termination therefore leaves the installation
incomplete. Content-only mod/resource updates do not change library identity and do not alter this
marker.

Start itself never becomes an installer: first-install provisioning belongs to instance/content
creation, and later integrity recovery belongs to the explicit **Repair game files** action.

## Launch-fast boundary

The fast `load_libraries` route still parses the resolved in-memory library artifact list and
rejects illegal relative paths. It performs **no per-artifact filesystem stat/open/hash, directory
creation, or library-network request**. The O(n) in-memory pass is still required to construct the
classpath; the removed O(n) work is filesystem integrity probing.

Forge/NeoForge launch-fast also skips installer `.sha1` fetches, library mirror lookup, embedded
Maven-library extraction/SHA-1 rewrite, processor-input extraction to the Forge temp directory, and
both provisioning/strong library download paths. Two existing launch
consumers remain intentionally outside that statement: Forge/NeoForge must open their already-local
installer archive to derive launch metadata/processors, and Pandora still opens selected native
archives when extracting natives. Neither path SHA-1 verifies or repairs the game-library set.
Consequently a missing/corrupt ordinary classpath library is left for Java/loader/Minecraft, while a
missing/corrupt Forge/NeoForge installer archive can still fail earlier during loader-version
construction. In both cases Start performs no hidden library repair/download.

Unchanged, deliberately outside this change:

- Minecraft/version JSON and loader metadata resolution remains stock. Missing or stale metadata
  follows the existing metadata-manager fetch/error behavior; this policy does not reinterpret it as
  a library.
- Java runtime verification/download remains stock on Start, including its existing integrity
  checks and download behavior.
- assets index and asset object verification/download remain stock on this branch; PR #45 is not
  composed into the Agent 202 head.
- log configuration behavior remains stock, including its own SHA-1/download path.
- AppCDS, incremental identity, persistent layout, login, argv/classpath/module-path ordering,
  parallel-launch policy and graphical work are untouched.
- Forge/NeoForge post-processors are **not** rerun from LaunchFast. Published Start trusts the
  transaction marker and leaves missing/corrupt generated loader outputs to fail later instead of
  hashing or regenerating them. The strong Repair path retains the stock processor output checks
  and processor execution. Repair may resolve the Java runtime when processor construction requires
  it. Repair does not verify assets.

## Residual risk accepted by product

A library can be absent or corrupt while the marker is published. Start deliberately does not
discover this. Java, the loader or Minecraft can fail after spawn/open instead of Pandora reporting
a pre-Java integrity error. The marker records only whether Pandora's own install/repair operation
completed; it is not a cryptographic statement about current library contents.

## Focused tests

- published/legacy state permits launch without a library scan;
- missing and corrupt libraries map to classpath paths with no file or network access in LaunchFast;
- first-install provisioning leaves an existing corrupt library untouched and downloads only a
  missing library, validating only the downloaded bytes;
- a newly downloaded library with the wrong SHA-1 is rejected and is not installed;
- strong repair detects a corrupt SHA-1 and replaces it from a test HTTP origin;
- interrupted/update-in-progress state blocks Start without auto-repair;
- compare-and-set publication refuses to overwrite a newer marker transition, including a repeated
  transition with the same textual reason but a newer generation token;
- cancelled Repair leaves an incomplete marker even when cancellation races with final publication;
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
7. Change Minecraft/loader version and interrupt its provision-missing update before publication.
   After restart, confirm Start stays blocked and does not provision/repair from Start; recover with
   **Repair game files**.
8. Report Repair duration separately from Start→Java. Java→menu/TTMM is a different metric and no
   savings claim should be inferred from this launcher-phase change.
