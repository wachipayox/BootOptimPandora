# Optional Windows security integration — Agent 195 decision

Base authority: `agent/integration-current@2e2bc373341f4e7f0676f56756a4a22796a77357`.

This candidate deliberately **does not change Microsoft Defender automatically**. It adds a Windows-only recommendation in Pandora Settings that shows the exact launcher executable path when that path is narrow enough, explains the risk, and opens the documented Windows Security settings page. It never changes Defender, SmartScreen, firewall, real-time protection, game files, modpack files, AppCDS, launch arguments, Quickplay, profile persistence, or update behavior.

## Defender scope decision

Microsoft documents two materially different exclusion concepts:

- `ExclusionProcess` excludes files opened by the named process. That is broader than excluding Pandora itself and is rejected for BootOptim.
- `ExclusionPath` can name a specific fully-qualified file. If automatic integration is ever enabled, the only currently defensible candidate is the exact canonical installed Pandora `.exe` as one file-path exclusion.

No `.minecraft`, `mods/`, downloads directory, parent install directory, user-data directory, or drive exclusion is permitted. Portable launcher builds and launcher executables located under a `.minecraft` or `mods` component are not recommendation candidates. No launcher-owned cache is proposed yet because this source tree does not establish a fixed, installation-owned cache identity that is independent of game/modpack/user content.

Primary Microsoft references:

- https://learn.microsoft.com/defender-endpoint/configure-exclusions-microsoft-defender-antivirus
- https://learn.microsoft.com/powershell/module/defender/add-mppreference
- https://learn.microsoft.com/powershell/module/defender/remove-mppreference
- https://learn.microsoft.com/defender-endpoint/common-exclusion-mistakes-microsoft-defender-antivirus
- https://learn.microsoft.com/defender-endpoint/prevent-changes-to-security-settings-with-tamper-protection
- https://learn.microsoft.com/windows/apps/develop/launch/launch-settings

## Why the automatic button is blocked

Defender path exclusions are exposed as path values. The documented add/remove operations operate on those values; they do not provide a launcher-owned rule identifier or owner metadata.

A local ownership journal could prove that Pandora once observed the exact path absent and then added it. It cannot prove that a future identical value is still Pandora-owned if an administrator/security product removes it and later recreates the same path while Pandora is not running. Removing that value would then risk deleting a third-party/admin rule, violating the product requirement that undo remove **only** rules created by Pandora.

Therefore this candidate does not ship `Add-MpPreference`, `Remove-MpPreference`, WMI/CIM writes, registry writes, PowerShell, an elevated helper, or UAC. The visible Settings action is only **Open Windows Security** using Microsoft's documented `ms-settings:windowsdefender` URI.

This is not a performance claim. A Windows CI build can prove only compilation/UI policy logic, not Defender behavior or Start→Java / Java→menu timing.

## UI/state contract

Current safe states:

- **Eligible manual recommendation**: show the exact canonical launcher EXE and the risk. The user may open Windows Security and make their own choice.
- **Rejected/dismissed**: no system change; launching Minecraft remains available.
- **Ineligible location**: portable builds, relative/non-EXE paths, and launcher paths under `.minecraft` or `mods` receive no exclusion recommendation.
- **Open-settings failure**: non-fatal; no system change.

Automatic-only states are specified but intentionally unreachable until ownership is solved:

- **UAC denied**: normal non-fatal result; must return to the launcher without changing game launch behavior.
- **Defender unavailable / third-party antivirus active**: do not offer or claim an exclusion.
- **Tamper protection / enterprise policy blocks change**: report blocked, do not retry silently, do not promise success.
- **Applied**: only after a privileged operation has verified the exact path is present and has durable ownership proof.
- **Undo**: only when that same durable identity can prove the current entry is Pandora-owned. If ownership is ambiguous, fail closed and leave the setting untouched.

## Future elevation/helper contract, if ownership becomes representable

Pandora already has a Windows point-operation elevation pattern: the unelevated launcher can spawn an explicit command through `ShellExecuteExW` with the `runas` verb. A future Defender operation should reuse that model rather than keep the main launcher elevated or spawn hidden/obfuscated PowerShell.

The privileged request must be minimal and auditable:

1. operation enum: add or remove exact launcher-file exclusion;
2. exact canonical absolute `.exe` path, with no wildcard and no parent-directory derivation;
3. expected launcher identity/version bound to the running binary;
4. explicit rejection of `.minecraft`, `mods`, portable locations and non-file scopes;
5. query-before, perform one documented Defender operation, query-after, and return a small status enum;
6. no firewall mutation and no unrelated Defender setting mutation.

The main launcher must treat UAC cancellation, Defender absence, tamper protection, enterprise policy and helper failure as normal non-fatal outcomes. It must never place this operation on the Play/Start critical path.

## Firewall extension point

No firewall rule is added. A future feature may define a separate contract only after it has a concrete inbound listener requirement with a specific executable, protocol, local port/direction, explicit consent and independent removal identity. Ordinary outbound login/update traffic is not such a requirement.

## Focused validation tier

The smallest gate for this candidate is:

1. focused `rustfmt --check` on the two touched Settings Rust files;
2. `cargo check -p frontend --tests --frozen`;
3. `cargo test -p frontend windows_security::tests --frozen -- --test-threads=1` on `windows-latest`.

This tier checks that the Windows-only UI compiles and that the path policy remains exact-file-only while rejecting portable/game/mod locations. It does **not** prove Defender behavior, tamper-policy behavior, UAC behavior, physical performance, or Minecraft startup improvement.
