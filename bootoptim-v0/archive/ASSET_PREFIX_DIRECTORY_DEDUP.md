# Agent 163 — asset object prefix directory deduplication

Base: PR #10 head `635171f4241320c0de4507fbb67ed45f1cfabd17`.

## Scope

This candidate changes only the redundant creation attempts for Mojang asset-object prefix directories inside `do_asset_objects_load`. It does not alter SHA-1 verification, URLs, object paths, downloads/repair, progress accounting, hashing/download concurrency, `AssetVerificationMode`, the USN cache foundation, AppCDS, metadata/network policy, or UI.

Pandora stores an ordinary asset object at `<assets objects>/<first two SHA-1 hex chars>/<full SHA-1>`. The stock loop decodes the full SHA-1, calls `std::fs::create_dir(prefix)` while ignoring the result, and then schedules the unchanged SHA-1 verification/download task. For Mojang's lowercase SHA-1 object names there are at most 256 first-level prefix directories, so an index with thousands of objects repeats a directory-creation syscall many times.

## Candidate

The loop keeps a process-local, per-call set of prefix paths that this invocation has already established as directories. The identity is the exact two ASCII bytes from the validated hash prefix, not the decoded first SHA-1 byte; this deliberately keeps valid `ab...` and `AB...` paths distinct on case-sensitive filesystems. For each object, full hash decoding still happens first and returns the same `InvalidHash` error on malformed input. A prefix is marked ready only when:

- `create_dir` succeeds; or
- `create_dir` reports `AlreadyExists` and the path is confirmed to be a directory.

Once marked ready, later objects with that exact prefix skip the redundant creation attempt. No persistent state or cache is added.

Crucially, every other directory-creation error remains ignored exactly as stock and does **not** mark the prefix ready. A later object with the same prefix retries creation. This preserves the current recovery opportunity after a transient `PermissionDenied`, missing parent, or other creation failure instead of turning one ignored failure into a launch-wide preflight error.

Concurrent launches are benign: one launcher may create the prefix while another receives `AlreadyExists`; after confirming the path is a directory, both use the same stock object paths. An `AlreadyExists` result caused by a non-directory path is not cached as ready and later objects retry, matching the stock failure/recovery shape as closely as possible.

## Why there is no feature property

No property is added. This is not a verification shortcut or policy choice: every object still executes the same SHA-1 check, and every download/write path is unchanged. The only skipped operation is a repeated `create_dir` after this same invocation has already established that the exact prefix directory exists. Failed creation attempts continue to be retried. A property would add configuration state without protecting a semantic boundary.

Physical performance remains unclaimed until A/B. If the physical gate shows no material benefit or an unexpected regression, this candidate should not be promoted despite semantic equivalence.

## Tests

`bootoptim_asset_prefix_directory_tests` covers:

- an empty assets directory and creation of only the required prefix;
- repeated objects sharing one prefix, proving one successful creation attempt is cached;
- an already-existing prefix directory and reuse thereafter;
- deterministic simulated `PermissionDenied`, proving the error is ignored and the next object retries;
- exact case-sensitive prefix identity (`ab` and `AB` remain distinct);
- an ordered asset-object map containing a valid object followed by an invalid hash, proving the valid prefix side effect happens first and the malformed hash still returns `LoadAssetObjectsError::InvalidHash`.

The cross-platform build workflow runs the targeted backend tests before the release build.

## Physical A/B gate

Use the PR #7 Start→Java probe endpoint unchanged. Compare the exact PR #10 base build against this candidate with the same instance, Java, launcher settings, repaired asset state, network state, and comparable cold/warm OS-cache condition. Alternate base/candidate runs rather than batching all of one condition first.

Record `launcher_pre_java` Start→Java and `assets_verify_download`. Do not claim savings from source inspection or syscall counts. Any changed integrity result, changed download/repair behavior, path difference, progress-count difference, launch failure, or regression is a NO-GO. The existing physical observation of 3,911 objects / 0.768 GiB and 232.559 s assets time motivates the experiment but is not evidence of savings from this candidate.
