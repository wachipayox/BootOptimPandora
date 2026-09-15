# Agent 168 — AppCDS identity-cache implementation foundation

Base: PR #20 head `8597819d4bbb8150be52f4601dac72b2937c9644`, which is PR #19 plus the Agent 168 design. PR #19 is stacked on PR #7 final `55f05135396e9682a5d0f76a24e62f21850fbc03`.

`AGENTS.md` is absent at this authority snapshot. Upstream provenance remains Moulberry/PandoraLauncher at `4eb6c7849561151695288443c106519774ee05ea`, MIT; the existing repository LICENSE/NOTICE are unchanged.

## What is implemented

The authenticated metadata capability from PR #11 has been extracted into the policy-free crate `bootoptim-v0/interposer/ntfs-usn-capability`.

The shared crate contains only:

- the fixed-size `BOPUSN01` protocol;
- exact volume-GUID and fixed-size FileId representation;
- an ephemeral Windows GUI-subsystem helper;
- `ShellExecuteExW(..., "runas", ...)` launch of that helper only;
- a random per-session nonce and pipe name generated through the OS CSPRNG;
- `PIPE_REJECT_REMOTE_CLIENTS`, explicit current-user + Administrators + SYSTEM DACL, and both client/server PID checks;
- helper-byte digest verification while a caller-owned read handle denies write/delete replacement until authentication completes;
- NTFS volume, FileId, journal and file-USN queries;
- journal-before/file-query/journal-after consistency in the elevated helper;
- non-elevated protected file handles opened with `FILE_FLAG_OPEN_REPARSE_POINT`, rejecting reparse points/directories and denying WRITE/DELETE sharing;
- same-handle identity re-read support;
- `MoveFileExW(REPLACE_EXISTING|WRITE_THROUGH)` atomic publication primitive.

It contains no `AssetVerificationMode`, Mojang SHA-1, asset index, `assets/objects` path rule, downloader or repair policy. Pandora, the AppCDS interposer and Java/Minecraft are never elevated by this crate.

The AppCDS side now has a policy engine for per-file digest records keyed by exact role + encoded path and authorized by volume/journal/FileId/file-USN/same-handle evidence. Size exists only as diagnostic consistency data and is explicitly not consulted by the reuse decision. Tests pin same-size/mtime-style mutation as a USN miss, rename as a record-key miss, delete/recreate as FileId miss, journal restamp/regression/discontinuity, every shared-capability failure and handle-identity failure.

The aggregate sidecar schema `bootoptim.appcds_identity_cache_probe.v1` is also present. It contains only reused/stock/miss file and byte counters, whether runtime reuse is authorized, and a boolean `helper_pin_compiled`; the helper digest itself is never emitted. It piggybacks on PR #19's aggregate inventory, so physical use must enable both sidecars. It never writes paths, hashes, argv or per-file timings and uses `create_new`.

## Concrete integration incompatibility: launch-source authority

The implementation deliberately does **not** skip one SHA-256 yet.

PR #19 invokes the interposer with instance dir, launcher executable, upstream commit, Java executable and JVM args. That protocol does not carry an authoritative stock-owned distinction between normal GUI Start and `StartInstanceByName` / `--run-instance` / other unknown callers. The requested cache contract requires CLI/unknown launches to perform the complete stock SHA-256 path.

`BOOTOPTIM_APPCDS_IDENTITY_CACHE=1` cannot solve this: an environment variable can request an experiment but cannot prove that the launch originated from the GUI. Treating it as launch-source authority would violate the fail-closed contract in PR #20.

Accordingly `current_identity_launch_authority()` returns `Unknown` unconditionally in this branch and the policy engine rejects runtime reuse with `SourceUnknown`. Because reuse cannot be authorized, the interposer also does not start the elevated helper: showing UAC for a launch that must full-hash anyway would be useless privilege.

The next integration change must propagate a small **AppCDS-specific** launch-source enum from the existing authoritative Start route to the final preflight invocation. It must not reuse `AssetVerificationMode`; unknown/default/CLI remains full hash, while only the audited normal GUI route may carry `NormalGui`. Once that value reaches the helper control protocol, the already-extracted capability and decision engine can be wired to the existing `artifact_from_path` / `pack_input_artifact` digest boundary without changing plan bytes.

## CI / packaged executable gate

The BootOptim v0 workflow now:

1. runs portable and Windows tests for the interposer and shared capability crate;
2. builds the capability-minimal `bootoptim-usn-helper.exe` first;
3. hashes that exact helper before the interposer release build, compiles the lower-hex SHA-256 pin into the interposer, and requires the packaged probe to report `helper_pin_compiled=true`;
4. builds the exact release interposer and patched Pandora;
5. executes the packaged interposer against a synthetic `plan` fixture;
6. repeats with `BOOTOPTIM_APPCDS_IDENTITY_CACHE=1` and requires `launch-plan.json` to be byte-for-byte identical while source authority is unavailable;
7. requires `runtime_reuse_authorized=false`, `reused_files=0`, `reused_bytes=0`, and positive aggregate stock/miss evidence;
8. verifies protocol/probe markers in the release binaries;
9. packages Pandora, interposer and capability helper together in `SHA256SUMS.txt`.

This gate proves only the hard-closed integration and release artifact. Interactive UAC behavior and physical performance are not established by hosted CI.

## Tests still required when the source-authority gate opens

Before `NormalGui` may enable reuse, the integration commit must add release-path tests that actually obtain protected handles and consume a baseline, then prove stock hashing for: same-size content with restored mtime, rename, delete/recreate, FileId mismatch, journal restamp/regression/discontinuity, reparse, protected-handle/read failure, corrupt/partial cache, helper absence/hash mismatch, UAC cancellation, pipe timeout, ACL/PID/nonce/protocol failure. It must also prove the Properties/MoreCulling final identity digest is byte-for-byte identical to the current builder and continue leaving `options.txt`, JSA and Java `release` outside v1 reuse.

No cache manifest is published in this branch because no runtime record can legally be consumed yet.

## Physical HDD protocol after reuse is enabled

No performance saving is claimed by this foundation.

The eventual A/B must use one exact green Windows artifact and:

- explicit `BOOTOPTIM_APPCDS_MODE=plan`;
- a structurally valid PR #7 Start -> Java root trace;
- a fresh PR #19 preflight sidecar per run;
- a fresh `BOOTOPTIM_APPCDS_IDENTITY_CACHE_PROBE` sidecar per run;
- stock/off baseline, full-hash cache-baseline creation, then unchanged reuse candidate;
- byte-for-byte equal `launch-plan.json` and equal `launch-plan.sha256` between stock and reuse;
- separately reported outer Start -> Java, `appcds_preflight`, PR #19 `build_launch_plan`, and aggregate reused/stock bytes;
- Java -> menu kept separate.

Any changed plan bytes, uncertain journal/handle evidence or failed mutation fixture is a NO-GO. There is **no measured HDD saving yet**.
