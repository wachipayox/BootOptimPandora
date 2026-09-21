# Optional Windows security integration — Defender process exclusion

Authority: agent/integration-current@2e2bc373341f4e7f0676f56756a4a22796a77357.

This continuation implements the explicitly authorized Microsoft Defender process exclusion for the canonical Pandora executable. It does not add file/folder exclusions, does not exclude Java or Minecraft, and does not change Defender globally, SmartScreen, firewall, real-time protection, AppCDS, instance contents, profiles, or launch arguments.

## User-visible contract

On the first Windows launch Pandora shows a compact “Optimize Defender scanning for Pandora” recommendation. The single explanatory sentence states that Defender will not scan files opened by this Pandora executable. Enabling requires an explicit click and a normal UAC prompt. Dismissing changes nothing and the launcher continues normally.

A dedicated Windows security settings page shows the local ownership state and offers Enable optimization, Remove optimization, or Remove previous exclusion. Detailed Defender semantics are linked rather than placed in the primary prompt. No Defender query or mutation runs from Play/Start or prelaunch.

## Exact privileged operation

The normal Pandora process remains unelevated. The action launches the same shipped Pandora executable with ShellExecuteExW and the runas verb. The elevated process exits through a fixed internal mode before launcher data directories, backend, frontend, networking, instance handling, or Java launch are initialized.

The internal CLI accepts only --internal-defender-process-exclusion enable or remove. It accepts no path argument.

For enable, the helper derives and canonicalizes its own executable path and rejects UNC/network targets, wildcards, non-absolute paths, non-.exe targets, and non-files. PowerShell receives the target only through the helper-created PANDORA_DEFENDER_TARGET environment variable; path text is never interpolated into PowerShell source. The elevated helper resolves the Windows system directory with GetSystemDirectoryW and invokes its fixed WindowsPowerShell\\v1.0\\powershell.exe path, so an inherited user PATH cannot substitute another executable.

The only Defender writes are:

    Add-MpPreference -ExclusionProcess $env:PANDORA_DEFENDER_TARGET -ErrorAction Stop
    Remove-MpPreference -ExclusionProcess $env:PANDORA_DEFENDER_TARGET -ErrorAction Stop

The query uses Get-MpPreference and inspects only ExclusionProcess. There is no EncodedCommand, Set-MpPreference, registry write, WMI mutation, firewall operation, global protection change, ExclusionPath, extension exclusion, or game-directory exclusion.

Microsoft documents a full process-image path as excluding files opened by that specific process. The Pandora executable itself is not excluded from scanning by ExclusionProcess.

## Ownership and reversal

Before adding, the helper queries the exact canonical path. If absent, Pandora adds it, verifies it became present, then writes .pandora-defender-process-owner-v1 beside the launcher executable. If the exact process exclusion already exists without Pandora's owner record, Pandora reports present but not owned and does not claim, replace, or remove it. Invalid ownership state fails closed.

The owner record contains only the exact Windows path encoded as UTF-16LE plus a fixed format marker. Removal requires that record and validates that its target is an absolute wildcard-free .exe in the same canonical launcher directory. UI and IPC never supply the removal path. This also lets an updater that renames Pandora within the same installation directory remove the previous owned exclusion before enabling the new one.

If the recorded exclusion is already absent, Pandora clears its local owner record without modifying Defender.

### Defender ownership limitation

Defender exposes process exclusions as list values, not individually tagged objects with a Pandora-specific rule ID. Pandora can prove it originally observed the value absent, added it, verified it, and retained its ownership record, but it cannot distinguish an administrator removing and later recreating the same exact path value while Pandora is offline. Under that edge case, removal removes that same process-path value. It cannot remove a different process exclusion and never broadens the deletion target.

## Failure states

UAC cancellation is normal. Defender unavailable, third-party security management, tamper protection, enterprise policy, cmdlet failure, query-after mismatch, corrupt ownership state, or path validation failure all leave normal launcher/game launch available. No silent retry occurs.

If adding succeeds but writing the ownership record fails, the elevated helper attempts to roll back that exact process exclusion and verifies rollback before returning failure.

## Threat boundaries

The privileged mode cannot be used as a general command runner: operation is a fixed enum and enable always targets the running Pandora executable. Removal is constrained by the helper-written owner record to the same launcher directory. No arbitrary UI/IPC path is accepted.

A portable launcher directory may be user-writable. Tampering cannot cause an arbitrary exclusion to be added because enable always targets the running Pandora executable. A tampered removal record is rejected unless it remains an exact .exe sibling of the canonical launcher, and removal is security-tightening rather than creation of a new exclusion.

## Focused validation

Hosted Windows CI runs scoped rustfmt, cargo check for command/frontend/pandora_launcher tests, focused command::windows::defender tests, and a Windows pandora_launcher build. Tests cover the fixed enable/remove authorization enum, rejection of arbitrary operations, same-directory removal validation, wildcard/game-path rejection, ownership-required removal planning, and fixed ExclusionProcess-only PowerShell contract.

Hosted CI does not approve UAC, mutate Defender, test enterprise/tamper policy, or establish performance impact.

## Physical Windows smoke protocol

1. On an authorized disposable/test Windows PC, record the candidate SHA-256 and full canonical Pandora path; confirm that exact value is absent from (Get-MpPreference).ExclusionProcess.
2. Start Pandora normally, choose Enable optimization on the first-run prompt, and approve UAC.
3. Confirm the launcher remains unelevated after the helper exits. Confirm exactly the full Pandora path appears in ExclusionProcess; confirm no Java/Minecraft path, .minecraft, mods, downloads, parent directory, wildcard, drive, firewall rule, or global Defender setting was added.
4. Open Settings → Windows security and require Enabled and owned by Pandora.
5. Restart Pandora and verify the first-run prompt does not repeat.
6. Choose Remove optimization, approve UAC, and confirm only that exact Pandora process value disappears. All other pre-existing exclusions remain unchanged.
7. Deny UAC for enable and remove in separate runs; require a non-fatal status and normal launcher operation.
8. Manually pre-create the exact Pandora process exclusion, then choose enable. Pandora must report present but not owned, create no ownership marker, and later offer no automatic removal of that entry.
9. If practical, exercise a managed/tamper-protected device. A rejected write must remain non-fatal and must not create successful ownership state.
10. Any later performance measurement must compare like-for-like physical cold/warm conditions and keep Start→Java separate from Java→menu. This change makes no timing claim.
