# Agent 175 — `prelaunch_setup_mods` contract and attribution

Base authority for this diagnostic branch is PR #7 final `agent155/launch-prejava-probe-20260914@55f05135396e9682a5d0f76a24e62f21850fbc03`. This deliberately excludes the later assets-USN/AppCDS candidates and does not change launch semantics.

## Source contract

`BackendState::prelaunch` first applies cross-instance syncing and then calls `prelaunch_setup_mods`. Syncing uses either the configured global `sync_targets` or an empty/default target set when the instance disables syncing. Therefore any future reuse key must include the effective sync configuration and must not move or suppress syncing.

`prelaunch_setup_mods` is not a simple immutable-mod copy. Its observable contract is:

1. Abort if a process is already running. Require an existing `mods/` directory.
2. Reload current Mods content with `Instance::load_content`; build the known-file set including Pandora aux files.
3. Scan the current top-level `mods/` directory. Anything not in the known set is treated as user/extra content and must survive the launch layout transformation.
4. Run `apply_modpack_and_collect_mods`. This resolves modpack children, including disabled-child state; writes eligible built-in/content-library modpack files outside `mods/` (for example config/yosbr-style paths) through the existing safe-write path; may request/download missing content; and returns the effective immutable mod copies for the launch.
5. Mark the Mods folder frozen, rename `mods/` to instance-root `original_mods`, recreate `mods/`, and materialize the immutable launch selection with `fastcopy` (reflink first, ordinary copy fallback; hard links are not requested by this call).
6. Copy every unknown/user extra top-level entry from `original_mods` into the launch `mods/` tree. Directory entries use `copy_content_recursive`, preserving the existing symlink/junction behavior and non-strict error policy.
7. On stop/close/crash recovery, `restore_mods_folder_if_stopped` does nothing while a process, closing process, or launch keepalive is alive. Once stopped, if `original_mods` exists it copies the runtime `.connector` cache back into `original_mods/.connector` when sandboxing is **off**, removes the temporary launch `mods/`, renames `original_mods` back to `mods/`, and unfreezes the folder. In sandbox mode that Connector merge is intentionally skipped.

These rules mean `original_mods` is mutable recovery state, not a reusable prepared snapshot. Reusing it as if immutable can lose user additions, Connector output, or crash recovery state.

## Diagnostic harness

`crates/backend/src/prelaunch_mods_probe.rs` is `#[cfg(test)]` only. It does not execute in Pandora and cannot change launch behavior. The hosted representative fixture deliberately uses the same production filesystem primitives used by this path: `fastcopy`, `copy_content_recursive`, and `write_safe`.

It records one non-overlapping sample per modeled filesystem subphase:

- `scan_mods_top_level`
- `rotate_original_mods`
- `materialize_immutable_mods`
- `copy_user_extra_entries`
- `write_modpack_extra_file`
- `restore_original_mods`

Each sample prints `bootoptim.prelaunch_mods_probe.v1` with monotonic wall nanoseconds, Linux process-CPU nanoseconds, bytes, file count, and directory count. These hosted numbers are attribution/resource evidence only. They are not the user's HDD timings and are not Start→Java or TTMM evidence.

The harness also exercises:

- repeated stock-style layouts;
- user-added files and directory extras surviving the cycle;
- Connector runtime cache merge when sandbox is off;
- sandbox preserving the original Connector state instead of merging runtime output;
- interrupted/prepared state restored through the stock recovery shape;
- a proposed fail-open identity boundary that invalidates on managed-mod selection, modpack files, disabled children, extra/user inputs, config inputs, sync targets, sandbox state, missing prepared state, abnormal previous exit, or ambiguous `original_mods` state.

## Optimization decision

A safe candidate is still plausible, but **not** by reusing `original_mods` and not by skipping sync/content/modpack discovery.

The strongest architecture worth implementing after physical attribution is a separate launcher-owned prepared-layout snapshot/cache. The cache may only replace the expensive final materialization step after Pandora has freshly recomputed the effective launch selection and all mutation-sensitive inputs. It must never become the source of truth for user `mods/`.

A production candidate would need all of the following before reuse:

- normal launcher state with no running/closing/keepalive process;
- no existing/ambiguous `original_mods` recovery state;
- previous prepared-layout publication completed atomically and previous launch exit/restore recorded cleanly;
- exact current Minecraft/loader and managed content selection;
- exact current modpack identity, effective files, and disabled-child state;
- exact user/unknown extra-entry identity sufficient to detect additions, removals, renames and changes;
- exact effective sync-target/config state **after** stock syncing has run;
- exact sandbox state;
- all relevant extra-file/config sources that `apply_modpack_and_collect_mods` can write;
- Connector handling kept separate from snapshot identity so runtime Connector output is merged according to stock restore semantics;
- filesystem capability check before any reflink/hardlink strategy, with ordinary stock rebuild on unsupported/cross-device/error cases.

Any missing/corrupt/partial identity, unexpected file type/reparse state, stale publication, abnormal exit, restore failure, content/download/update activity, or uncertain state must execute the complete stock transformation.

Hard links are not a default recommendation: linking a prepared file to mutable user-visible state can violate the stock ownership model. Reflink/copy from immutable content-library inputs remains the safer primitive if a candidate is later justified.

## Physical protocol

Do not compare the supplied 10.91 s / 12.35 s coarse physical observations as candidate savings. They only motivate this front.

For a future runtime candidate, use the same PR #7 launch-root contract and identical endpoint in every run:

1. Same Pandora branch/artifact family, same instance, Java, account, pack/config, and launcher settings; no assets/AppCDS experimental changes between A/B conditions.
2. Fresh `BOOTOPTIM_LAUNCH_PROBE` path each run. Reject captures without exactly one valid `launcher_pre_java.begin/end` and Java spawn inside the root.
3. Alternate stock → candidate → stock → candidate. Restart Pandora between conditions if candidate activation is process-static.
4. Record the existing coarse `prelaunch` span and Start→Java (`launcher_pre_java.begin` → successful `java_spawn.end`) separately. Do not add overlapping child spans.
5. Record candidate-specific materialization hit/miss reason, files/bytes reused versus rebuilt, and whether Connector merge/restore completed. A cache seed/rebuild run is not a reuse A/B.
6. Reject runs containing modpack/content download/update work that is not matched across the pair.
7. Negative fixtures before promotion: add/remove/replace a user mod, change a config/extra source, change sync targets, change modpack/disabled child, sandbox toggle, interrupted launch/kill, restore failure simulation, stale/corrupt/partial snapshot, and cross-device/unsupported copy capability. Every uncertainty must rebuild stock and preserve the user-facing post-stop `mods/` tree.
8. Report physical storage origin as HDD and keep `prelaunch`, Start→Java, and Java→menu/TTMM separate. A reduced hosted fixture phase or reduced copy count is not a performance claim.

## CI tier

This diagnostic branch intentionally uses the Agent 175 minimal Rust tier only: `cargo fmt --check`, `cargo check -p backend --tests --frozen`, and the targeted backend probe tests on Ubuntu. The inherited release/matrix jobs are branch-gated off here. No dev Windows build, release Windows build, final artifact, or full platform matrix is required for this test-only diagnostic.

A production candidate that changes runtime semantics must first pass the same minimal gate, then a Windows dev semantic build only if needed, and only after the candidate is coherent should it proceed to the release Windows/package gate and full matrix.
