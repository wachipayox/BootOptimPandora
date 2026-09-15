# Clean USN assets-cache promotion candidate

This branch is recomposed from the stock authority `master@4eb6c7849561151695288443c106519774ee05ea`. It is not a merge or rebase of the experimental BootOptim chain.

## Included semantic slices

- PR #10: `AssetVerificationMode::{Normal, FullVerification}`. Only the existing GUI `root::start_instance` constructs `Normal`; default, CLI/`StartInstanceByName`, legacy and unknown callers remain `FullVerification`.
- PR #9/#11: schema-v1 manifest, fail-closed reuse decision, atomic publication, and the minimal `do_asset_objects_load` integration. Stock verify/download/repair ordering is retained.
- PR #27: the active Windows capability is the direct unprivileged NTFS adapter (`windows.rs` + `windows_direct.rs`) with protected no-reparse handles, direct journal/FileId/file-USN queries and final identity re-check. The assets consumer contains no elevated helper, named pipe, `usn_protocol`, or helper executable.
- PR #24/#27: activation sidecar v2 is observational only. It reports `capability_mode=direct_unprivileged_ntfs` and `elevation_requested=false`; it never authorizes reuse.
- PR #22: only the tiny hash-observation primitive needed for activation-probe accounting is retained. The PR #13/#22 attribution sidecar is excluded because its final composition depends on the independent PR #7 launch-probe lineage.
- PR #27 standalone restricted-token probe is retained as a Windows semantic gate; it is not production authority.

## Explicitly excluded

PR #7 click-to-Java instrumentation, AppCDS/interposer work, helper/UAC runtime, named-pipe IPC, `usn_protocol`, `bootoptim-usn-helper.exe`, helper packaging, classpath changes, mod/update/UI/login changes, and experimental workflows are not part of this candidate.

## Integrity boundary

Reuse still requires explicit `BOOTOPTIM_ASSET_USN_CACHE=1`, `AssetVerificationMode::Normal`, canonical launcher `assets/objects`, exact asset-index SHA-1, exact expected object SHA-1, NTFS volume identity, journal ID/bounds continuity, FileId, file USN, a protected write/delete-denying no-reparse handle, and unchanged final handle identity. Any uncertainty falls back to Pandora's stock SHA-1/download path.

The separately assigned policy change that must prevent a *massive* all-assets SHA-1 fallback is intentionally **not** implemented here. This clean promotion candidate preserves the currently validated fallback semantics so that Agent 177's policy change can be reviewed as a subsequent isolated PR.

## Physical evidence inherited from the exact PR #27 artifact

On the target HDD, normal GUI launch reported direct unprivileged NTFS capability with `elevation_requested=false`, no helper and no UAC. The measured seed/reuse pair was 3,911 SHA-1 reads followed by 3,911 verified reuses with zero SHA-1 bytes/network on reuse; the assets phase was 163.319 s then 2.313 s. This is a paired assets-phase observation, not TTMM and not a median. Mutating or deleting one asset caused one SHA-1/repair of 8,565 B. Corrupting the manifest JSON fell back to 3,911 SHA-1 reads and rebuilt it.

Do not deliberately reset/restamp the OS-global USN journal on a user's machine. Journal discontinuity remains semantic-test coverage. Promotion still requires a physical run from a genuinely non-admin Windows account because the hosted restricted-token probe is only an approximation.

## Physical protocol for this artifact

1. Verify the artifact's commit SHA and checksum, use normal GUI Start, canonical launcher assets, and set `BOOTOPTIM_ASSET_USN_CACHE=1`. Use a fresh `BOOTOPTIM_ASSET_USN_PROBE` path for each launch.
2. Seed from a valid repaired asset set; require a full baseline and v2 probe `direct_unprivileged_ntfs`, `elevation_requested=false`, with no UAC/helper process.
3. Launch immediately again unchanged; require verified reuse for every eligible object and zero stock SHA-1 reads/network for that phase.
4. On disposable copies, test one same-size/restored-mtime mutation and one delete/recreate; only the affected object may take the stock verification/repair path under the current semantics.
5. Corrupt/truncate the manifest and confirm safe fallback/republication under this candidate. Do not interpret that current fallback as the final mass-hash policy; the follow-up policy PR owns that change.
6. Repeat the capability/seed/reuse gate while logged into a real standard (non-admin) Windows account. Keep assets-phase time, Start-to-Java, Java-to-menu and TTMM separate.
