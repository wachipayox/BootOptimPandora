# Agent 204 — recomposed libraries launch-fast policy

Composition authority: `7a821e8d5f89c71677d149b3140e21f4ed171ec4` (merged PR #45). Semantic source: PR #49 head `241617f631f02463c3f9be2d558eb3cc4fc4ba11`. This recomposition preserves PR #45 assets/USN behavior unchanged and narrows PR #49 to the authorized libraries policy below.

## Product policy

Normal GUI **Start, first install and update never SHA-1-hash, cryptographically verify, download or repair game libraries**. A published installation may therefore contain a missing/corrupt library and fail later in Java, the loader or Minecraft. Pandora must not convert that failure into hidden recovery.

The explicit **Repair game files** action is the only normal UI authority for the strong stock library route: SHA-1 validation when metadata supplies a digest, download on missing/mismatch, downloaded size/hash validation, and Forge/NeoForge library preparation/post-processors.

## Transactional state

Each instance owns `.bootoptim/game-files-state-v1.json`.

- `published`: Start may use LaunchFast.
- `incomplete`: Start stops with “Installation incomplete; use Repair game files”; it never repairs automatically.
- no marker: legacy/pre-policy instance is treated as published **without scanning libraries**.
- unreadable/corrupt/unknown marker: Start fails closed to Repair.

New instance creation writes `incomplete` before publishing instance metadata. It performs no library provisioning. A standalone creation publishes after its launcher-owned creation transaction completes. A new content/modpack install stays incomplete through the final content copy and publishes only after that operation completes.

Minecraft/loader/loader-version changes persist `incomplete` before mutating dependency identity; if that marker write fails, the identity change is rejected. Completion performs no library I/O. It publishes only if the instance still has the exact expected identity. Every incomplete transition receives a process-local monotonically increasing generation, and publication is a serialized compare-and-set on reason + generation. A stale A→B→A completion or older same-kind operation therefore cannot publish a newer transaction.

Repair writes `repair-in-progress` before touching libraries and publishes only after the strong route succeeds with no cancellation pending. Cancellation/failure/crash leaves or restores an incomplete marker. A cancellation racing the final compare-and-set is written back to `repair-cancelled`.

## LaunchFast boundary

`load_libraries` parses the resolved in-memory artifact list, rejects illegal relative paths and maps artifacts to canonical library paths. It performs no per-artifact stat/open/hash, directory creation or library network request.

Forge/NeoForge LaunchFast skips installer `.sha1` fetches, mirror lookup, embedded Maven extraction/SHA-1 rewrite, processor-input extraction, strong library download and post-processor regeneration. It may still open the already-local installer archive to derive launch metadata/processors, and normal native extraction may open selected native archives. Neither is a hidden library integrity repair. Missing/corrupt installer or generated loader outputs may therefore fail before or after Java without network recovery.

Unchanged and outside this change: Minecraft/version/loader metadata resolution, Java runtime integrity/download, log configuration, assets behavior (including merged PR #45 USN cache), AppCDS #47/#48/#50/#51, Quickplay, Defender, persistent layout, login, classpath/module-path/argv ordering, parallel launch and Automodpack.

## Focused validation

The Agent 204 gate:
- rejects any active backend symbol for `ProvisionMissing`, `provision_game_files` or `do_libraries_install_missing`;
- runs `cargo check -p bridge -p backend -p frontend --tests --frozen`;
- tests LaunchFast missing/corrupt path mapping with no library I/O and explicit Repair corruption→network replacement;
- tests legacy/published/incomplete/cancel/corrupt marker behavior and generation/CAS stale-completion protection;
- builds exactly one Windows x86_64 release candidate only after the focused gate passes, then writes `COMMIT.txt` and `SHA256SUMS.txt`.

Hosted CI is correctness/packaging evidence only, never a Start→Java or TTMM performance measurement.

## Physical smoke protocol

Use only the exact checksummed Windows artifact from the draft PR. Record **Start→Java** separately from **Java→usable menu**.

1. **Published Start:** normal GUI Start from published state; confirm no library SHA-1, library network request or hidden repair.
2. **Missing library:** delete one ordinary classpath library while published; Start must not hash/download/repair it. Record any later Java/loader failure separately.
3. **Corrupt library:** alter one library in place; Start must not hash/download/repair it.
4. **Explicit Repair:** invoke **Repair game files** and confirm the missing/corrupt library is restored by the strong route; successful completion republishes state.
5. **Repair cancelled:** cancel an active Repair; state stays incomplete/cancelled and Start refuses until a later successful Repair.
6. **Interrupted update/loader change:** interrupt after the incomplete marker is written and before publication; on restart Start remains blocked and performs no library recovery. Complete/retry the operation or use explicit Repair as appropriate.
7. Report Repair duration independently. Do not infer Java→menu/TTMM improvement from this launcher-phase policy.

No physical smoke, promotion or merge is part of Agent 204.
