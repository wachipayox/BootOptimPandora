# Agent 171 — AppCDS GUI launch authority

## Provenance and scope

Base is exactly PR #21 head `agent168/appcds-identity-cache-runtime-20260915@7d39ca0fee2941f71d51361bbc89c383ed69f32e`. PR #21 is stacked on PR #20/#19 and ultimately the PR #7 launch-root lineage. `AGENTS.md` is absent at this authority snapshot (direct GitHub lookup returns 404).

This change implements only the missing AppCDS launch-source authority. It does not change `AssetVerificationMode`, asset repair/download behavior, classpath/module-path/order, Java argv or launch-plan bytes, AppCDS READY/TRAIN/lock/staging/promotion, visible UI, network, scheduler, Automodpack, MoreCulling/Properties canonicalization, or the capability-minimal USN helper.

## Authority contract

`AppCdsLaunchAuthority` has exactly two states:

- `Unknown` — the fail-closed default for every constructor/caller unless explicitly audited;
- `NormalGui` — constructed only by the existing frontend `root::start_instance` GUI launch helper.

`ModalAction::default()` remains `Unknown`, so backend `StartInstanceByName` (`--run-instance`), API/legacy callers that synthesize/default an action, uncontrolled tests, and future unknown routes cannot authorize AppCDS identity-digest reuse. Quick-play that enters the same audited GUI helper carries `NormalGui`. No argument, path, environment value, asset mode, or filesystem observation is used to infer source.

The immutable value is copied from `ModalAction` through `Launcher::launch` into `LaunchContext`, then into a private `PandoraCommand` control field. `PandoraCommand` supplies one interposer control argument before `--`: `--appcds-launch-authority unknown|normal-gui`. It is not part of Java argv and therefore cannot alter canonical launch-plan bytes. Direct/legacy interposer invocation without the field defaults to `Unknown`; duplicate or malformed values are rejected, and the helper main path fails open to `STOCK`.

The AppCDS identity cache remains default-off: `BOOTOPTIM_APPCDS_IDENTITY_CACHE=1` is still required. `NormalGui` is necessary permission only. Feature-off, non-Windows, non-NTFS, missing/mismatched helper pin, UAC cancellation/denial, IPC/protocol/PID/ACL uncertainty, protected-handle failure, reparse/non-regular input, journal reset/regression/discontinuity, FileId/USN/final-handle mismatch, corrupt/partial cache, and `Unknown` authority all remain stock full SHA-256 paths.

## Automated gates

Portable and Windows tests must prove default/legacy `Unknown`, explicit GUI constructor `NormalGui`, clone preservation, command transport default/explicit mapping, interposer control parsing, and `Unknown -> SourceUnknown` reuse rejection. CI also checks that production uses of `ModalAction::normal_gui_launch()` outside its defining test module exist only in `crates/frontend/src/root.rs`.

The Windows packaged gate executes the release interposer against one synthetic `plan` fixture with omitted/default authority, explicit `unknown`, and explicit `normal-gui`. `runtime_reuse_authorized` must remain false for omitted/unknown and become true only for `normal-gui`, while `launch-plan.json` remains byte-for-byte identical to the stock baseline. The release Pandora EXE is checked for the control markers. This establishes transport/linkage only; PR #21's runtime still decides every actual digest reuse using all capability/evidence gates.

## Physical protocol after a green artifact

Use only the matching green Windows artifact and verify `SHA256SUMS.txt`. Keep `BOOTOPTIM_APPCDS_MODE=plan`; combine the PR #7 valid Start-to-Java root probe with a fresh PR #19 AppCDS preflight sidecar and the aggregate identity-cache sidecar. Keep Java-to-menu separate.

1. **Stock fallback:** feature off. GUI Start produces the stock plan/probes.
2. **Unknown fallback:** opt in but launch through a non-audited CLI/legacy/API route where available. Reuse must remain unauthorized and full SHA-256 stock; source `Unknown` must not cause helper/UAC work solely for this candidate.
3. **First GUI seed:** opt in and use audited GUI Start. This is the baseline/seed run; require valid root/probe schemas and byte-identical plan versus stock for unchanged inputs.
4. **Second unchanged GUI run:** repeat audited GUI Start. This is the only reuse candidate after a valid baseline. Require the same plan bytes and `launch-plan.sha256` as stock/seed.
5. Exercise feature-off, non-Windows/non-NTFS, missing/mismatched helper, UAC cancel/deny, IPC/protocol/PID/ACL failure, protected-handle failure, reparse/non-regular input, journal reset/regression/discontinuity, FileId/USN/final-handle mismatch, corrupt/partial cache, and `Unknown`. Every case must fall back to full stock SHA-256.

Report `appcds_preflight`, PR #19 `build_launch_plan`, aggregate reused/stock bytes, and the outer PR #7 Start-to-Java span separately. The physical PR #19 observation supplied for this task, `build_launch_plan = 307.918 s` on pack inputs/mods, is motivation only. It is not an A/B result, startup saving, or TTMM claim.
