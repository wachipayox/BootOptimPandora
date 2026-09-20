# Optional Windows security integration — Agent 195 decision

Base authority: `agent/integration-current@2e2bc373341f4e7f0676f56756a4a22796a77357`.

This candidate deliberately **does not change Microsoft Defender automatically**. It adds a Windows-only Settings diagnostic that explains why the currently documented Defender exclusion types do not provide a narrow, evidence-backed remedy for the observed JAR/file scanning problem. It never changes Defender, SmartScreen, firewall, real-time protection, game files, modpack files, AppCDS, launch arguments, Quickplay, profile persistence, or update behavior.

## Technical decision: no exclusion candidate yet

Microsoft documents materially different exclusion concepts:

- An `ExclusionPath` may name one fully-qualified executable file. That excludes that file itself from scanning; it does **not** exclude the files the executable later opens.
- An `ExclusionProcess` excludes files opened by the named process. That is much broader than excluding Pandora itself and is rejected for BootOptim.

The laptop symptom motivating this work is Defender spending time on launcher-produced/scripts/JAR-related file activity. Excluding only the installed Pandora executable therefore does not address the relevant JAR/file scans and must not be sold as a performance remedy. A process exclusion could affect those opened files, but would create a broad blind spot and violates the narrow-scope requirement.

Accordingly, this candidate recommends **no Defender exclusion**. It does not propose `.minecraft`, `mods/`, downloads, user files, a parent install/game directory, or a drive. It also does not propose a launcher cache exclusion because the authority tree does not establish a fixed, installation-owned cache identity that is independent of game/modpack/user content.

The safer diagnostic next step is Microsoft's Defender Antivirus performance analyzer, which records and reports the files, paths, extensions, and processes with the highest scan impact. The Settings panel links to those Microsoft docs but does not launch an elevated recording itself.

Primary Microsoft references:

- https://learn.microsoft.com/defender-endpoint/microsoft-defender-antivirus-exclusions-overview
- https://learn.microsoft.com/defender-endpoint/configure-exclusions-microsoft-defender-antivirus
- https://learn.microsoft.com/powershell/module/defender/add-mppreference
- https://learn.microsoft.com/powershell/module/defender/remove-mppreference
- https://learn.microsoft.com/defender-endpoint/tamper-protection-antivirus-exclusions
- https://learn.microsoft.com/defender-endpoint/performance-analyzer-reference
- https://learn.microsoft.com/windows/win32/secbp/running-with-administrator-privileges
- https://learn.microsoft.com/windows/win32/api/shellapi/nf-shellapi-shellexecutew
- https://learn.microsoft.com/windows/apps/develop/launch/launch-settings

## Why automatic add/remove is additionally blocked

Defender path exclusions are exposed as path values. The documented add/remove operations operate on those values; they do not provide a Pandora-owned rule identifier or owner metadata.

A local journal could prove Pandora once observed a value absent and added it. It cannot prove that a future identical value is still Pandora-owned if an administrator or security product removes it and later recreates the same value while Pandora is offline. Removing it would then risk deleting a third-party/admin rule, violating the requirement to undo **only** rules Pandora created.

Therefore this candidate ships no `Add-MpPreference`, `Remove-MpPreference`, WMI/CIM write, registry write, PowerShell mutation, elevated helper, or UAC request. The visible actions only open Microsoft documentation and the documented Windows Security settings URI.

## UI/state contract

Current reachable states are intentionally small:

- **Diagnostic available**: explain that no safe/evidence-backed exclusion is recommended and link to Defender performance-analyzer documentation.
- **Dismiss/reject**: no system change; Minecraft launch remains available.
- **Open-link/settings failure**: non-fatal; no system change.
- **Non-Windows**: this Settings group is not compiled or shown.

If a future automatic operation becomes technically useful and ownership becomes representable, its required explicit states are:

- **Accept**: show the exact operation and exact target before requesting UAC.
- **UAC denied**: normal non-fatal result; return to the launcher with no launch behavior change.
- **Defender unavailable / third-party antivirus active**: do not offer or claim a Defender exclusion.
- **Tamper protection / enterprise policy blocked**: report blocked, do not retry silently, and do not promise success.
- **Applied**: only after a privileged operation has queried before/after and verified the exact intended value.
- **Undo**: only when durable identity proves the current value is still Pandora-owned. Ambiguity fails closed and leaves the setting untouched.

## Future elevation/helper contract

The exact base tree does not contain an integrated Defender/UAC helper. Open PR #11 was reviewed only as prior design evidence; it is not authority for this candidate.

Microsoft recommends keeping the main application unelevated and separating privileged point operations into a helper. A future implementation should use normal Windows UAC semantics such as `ShellExecuteExW`/`ShellExecuteW` with the `runas` verb rather than hidden/obfuscated PowerShell or silent self-elevation.

Any future privileged request must be minimal and auditable:

1. one fixed operation enum, with no generic command execution;
2. one exact canonical target with no wildcard or parent-directory derivation;
3. expected helper/launcher identity and version bound to the shipped binary;
4. fail-closed validation that rejects game/mod/user/download/drive scope;
5. query-before, perform one documented Defender operation, query-after, and return a small status enum;
6. no firewall mutation and no unrelated Defender setting mutation.

The main launcher must treat UAC cancellation, Defender absence, tamper protection, enterprise policy, helper mismatch, and helper failure as normal non-fatal outcomes. The operation must stay outside the Play/Start critical path.

## Firewall extension point

No firewall rule is added. A future feature may define a separate contract only after it has a concrete inbound listener requirement with a specific executable, protocol, local port/direction, explicit consent, and independent removal identity. Ordinary outbound login/update traffic is not such a requirement.

## Focused validation tier

The smallest gate for this candidate is:

1. `rustfmt --check` on the new Windows security module only. Existing Settings files have baseline formatting drift; this candidate does not mass-format unrelated code.
2. `cargo check -p frontend --tests --frozen`.
3. `cargo test -p frontend windows_security::tests --frozen -- --test-threads=1` on `windows-2022`.

This tier checks compilation and the pure scope policy: exact launcher-file exclusion is not represented as a JAR-scan remedy, process scope is rejected as broad, game/user scopes are forbidden, cache scope is unavailable without fixed ownership identity, and automatic mutation remains blocked by ownership ambiguity. It does **not** prove Defender behavior, tamper-policy behavior, UAC behavior, physical performance, Start→Java improvement, or Java→menu improvement.

No release/matrix run is required for this Settings-only candidate.
