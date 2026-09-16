# Persistent profile identity and cross-process lock candidate

Status: Agent 189 correctness/security slice stacked on Agent 188 / PR #36. This changes neither Minecraft-visible behavior nor the Start path and makes no startup-time or TTMM claim. Upstream/MIT notices remain unchanged.

## Exact duplication hook and identity semantics

Pandora's real instance-copy path is `crates/backend/src/duplicate.rs::duplicate_instance`, which calls `duplicate_with_content_library` over the whole instance root. Without an explicit exclusion this also copies `.pandora-layout-v1`, including its durable UUID and potentially transaction/recovery state.

This candidate intercepts that path before the destination directory is populated:

1. A source with no `.pandora-layout-v1` is explicitly `Legacy`. It is copied as legacy payload, while the new destination receives a fresh persistent UUID before copying. No manifest is invented, so a later persistent open/reconcile sees a distinct identity that still needs reconcile.
2. A persistent source is accepted only while an OS lock for its existing UUID is held and only if it is demonstrably `Ready`: valid matching identity/manifest, no journal, no non-empty staging/backup/conflict evidence, and every managed live regular file still hashes to its committed `applied_hash`.
3. `Prepared`, `Publishing`, `Recovering`, `NeedsReconcile`, missing/corrupt identity/manifest, active journal, unresolved staging/backup/conflict evidence, unsafe path/reparse state, or a changed managed byte rejects duplication. Duplication never runs recovery to manufacture a copyable state.
4. The copier skips the complete source `.pandora-layout-v1` subtree. The destination control namespace is created independently with a new UUID before payload copy and is locked for the duration.
5. A Ready source gets a reminted generation-1 Ready manifest only after the copied managed live set is re-hashed successfully. The copied manifest resets `transaction_id` to `None`. Journal, staging and backup from the source are never copied or reinterpreted for the clone.
6. Failure/cancellation removes the incomplete destination. The source lock is retained through the copy so a cooperating second Pandora process cannot reconcile/publish the source concurrently.

The clone still shares immutable content-library sources through Pandora's existing duplication optimization. That sharing does not transfer persistent profile ownership or recovery identity.

## Per-profile cross-process exclusion

The lock is local to one durable profile UUID:

```text
<instance>/.pandora-layout-v1/locks/<profile-uuid>.lock
```

The lock file is permanent metadata; its mere existence is never treated as ownership. Successful acquisition writes diagnostic owner metadata (`profile_uuid`, PID, acquisition wall time and random nonce) while the OS lock is already held. Liveness comes only from the kernel primitive.

Acquisition is non-blocking. Contention returns a clear `ProfileBusy(uuid)` service error; the future consumer decides how to expose Busy/NeedsReconcile. This PR does not block or wire the Start button.

### Unix (Linux/macOS)

The candidate opens the lock file with `O_NOFOLLOW|O_CLOEXEC`, takes `flock(LOCK_EX|LOCK_NB)`, then verifies the pathname and held descriptor still refer to the same regular-file device/inode. `EWOULDBLOCK/EAGAIN` is Busy. The guard releases with `LOCK_UN` and descriptor close.

### Windows

The candidate opens the lock file with zero share permissions plus `FILE_FLAG_OPEN_REPARSE_POINT`. A sharing/lock violation is Busy. After open, file metadata must be regular and must not carry `FILE_ATTRIBUTE_REPARSE_POINT`. The zero-share handle also prevents a cooperating process from replacing/deleting that pathname while the guard is live. Handle close is the release primitive.

### Crash/stale behavior

A crash closes the process descriptor/handle in the kernel, so the lock becomes acquirable without deleting any pathname. The next owner reuses the permanent file and overwrites owner metadata only after acquiring the OS primitive. There is deliberately no age-based stale-file deletion and no PID-only authority, so one process cannot erase a live lock merely because a timestamp or PID heuristic looks stale.

### Acquisition order

`BackendState::reconcile_persistent_profile_layout` retains the existing stopped-instance, sandbox and legacy-restore gates first. It then creates/loads the durable UUID and acquires the per-profile OS lock **before** `PersistentProfileLayout::open`. That matters because `open` may recover a journal. The same RAII guard remains held through recovery, planning, staging, Prepared, Publishing, manifest publication and cleanup. Therefore a second process arriving while the first is Publishing receives Busy before it reads or acts on the first process's journal/backup.

UUID initialization itself uses create-new publication: concurrent first migration contenders converge on the one identity that wins creation, then contend on that UUID lock.

## Focused coverage

The Agent 189 gate runs affected-path `rustfmt --check`, `cargo check -p backend --tests --frozen`, and `cargo test -p backend profile_layout --frozen -- --test-threads=1`. The inherited Agent 188 ownership/publication/recovery tests remain in that filter.

Added coverage includes:

- second acquisition of one UUID is Busy; dropping the guard permits reacquisition;
- two distinct profile UUIDs can be locked concurrently;
- a real second test process holds the lock, the parent observes Busy, the child is killed, and the parent then reacquires the same persistent lock file without stale-file deletion;
- Unix symlink lock-path substitution is rejected without modifying the target;
- Legacy clone mints a distinct identity but does not invent Ready state;
- persistent Unknown/non-Ready clone is rejected;
- a real Ready duplication remints UUID/manifest namespace, skips journal/staging/backup, preserves source bytes while the clone is reconciled independently, rejects a journal forged with the original UUID, and deleting the clone does not remove the original profile state.

## Validation tier and promotion boundary

Tier is focused Rust correctness only. General Linux/Windows/macOS release builds and the BootOptim-v0 Windows artifact workflow are branch-gated off for this draft so iteration does not spend the final promotion gate repeatedly.

After review accepts the lock/clone contract, the release Windows gate should specifically verify:

- the Windows zero-share/reparse-point open compiles under the pinned MSVC target;
- two independently launched backend processes receive one owner/one Busy result for the same UUID;
- abnormal termination releases the handle and permits safe reacquisition without deleting the lock file;
- junction/symlink/other reparse substitution at the lock/control path fails closed;
- Ready duplication under Windows reflink/content-library behavior still never copies the source control namespace.

macOS should also compile the `flock` path before promotion. Any filesystem/ACL/antivirus behavior that prevents safe lock acquisition remains fail-open/error rather than weakening exclusion.

## Future Linux service / Beta seam

The future signed distribution service remains upstream of the existing verified-local-desired-set API. It needs only these local client states: durable `profile_uuid`, Ready/NeedsReconcile, and Busy from lock contention. It may provide authenticated revision/object identities and already-verified local desired content, but it must not carry or select a client journal/backup/transaction UUID, bypass local ownership hashes, or force recovery while another process owns the profile lock. Beta visibility/channel policy remains server/UI work and is not implemented here.

## Metrics

This is correction and safety before any Ready Start fast path. PR #30's HDD Start-to-Java observation remains motivation only and is not an objective or result of this slice. Future measurements must keep click/request to Java spawn, Java spawn to usable menu, and AppCDS training/exit work separate. No TTMM benchmark or improvement is claimed here.
