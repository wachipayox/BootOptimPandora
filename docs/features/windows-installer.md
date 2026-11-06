# Windows installer

`scripts/build_windows.sh <version>` produces, in `dist/`:

- `WachilandLauncher-Windows-x86_64-Setup.exe`: the installer.
- `WachilandLauncher-Windows-x86_64-Portable.exe`: the same launcher exe. Its
  name contains "portable", so it keeps its data next to itself.

`LAUNCHER_EXE=<path>` packages an already built exe instead of running Cargo.

## Tooling

The installer is the NSIS installer that Pandora already produced through
`cargo-packager` (pinned to 0.11.8). It uses our own template in
`package/windows/`:

- `installer.nsi`: derived from cargo-packager's default template.
- `Spanish.nsh`: the wizard text.
- `generate_installer_art.py`: builds the bitmaps at packaging time from the
  release cube, the café background and Chakra Petch.

NSIS stretches bitmaps with GDI, which aliases. So one bitmap is generated per
display scale (100–200 %) at the exact size of the MUI2 image controls, and the
page picks the matching one at run time.

The wizard uses the dark launcher palette. It sets a dark title bar, the Windows
dark theme on buttons, and coloured check boxes, progress bar and log. It has no
administrator prompt.

## Layout and upgrades

| What | Where |
|---|---|
| Program | `%LOCALAPPDATA%\Programs\Wachiland Launcher\WachilandLauncher.exe` and `uninstall.exe` |
| Launcher data | `%APPDATA%\WachilandLauncher`. Never touched by the installer or the uninstaller. |
| Shortcuts | `Wachiland Launcher.lnk` in the Start menu, and optionally on the desktop |
| Uninstall entry | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Wachiland Launcher` |
| Install location | `HKCU\Software\Wachiland Elite\Wachiland Launcher` |

The install is per-user, so the signed in-place updater
(`crates/backend/src/launcher_install.rs`) can replace the exe without UAC. The
exe name must not contain "portable", or the data directory would move next to
the exe.

Running the installer again reuses the recorded folder. It skips the folder
page, closes a running launcher after asking, and overwrites the exe in place.
The installed exe may have been updated by the launcher itself, while the
registry `DisplayVersion` is not updated. So the installer compares against the
exe's own `FILEVERSION` and asks before replacing a newer launcher.

The uninstaller removes:

- the exe and the uninstaller;
- the updater's leftovers (`*.wbombo-previous-*`, `*.wbombo-new-*`);
- the shortcuts and the registry keys.

The shortcuts set no AppUserModelID, because the launcher process sets none.
Explicit IDs would split the pinned icon from the running window.

Silent use: `/S` for a fully silent run, `/P` for passive mode. Both create the
desktop shortcut unless `/NS` is passed, and `/R` starts the launcher afterwards.

## Code signing

`scripts/sign_windows.ps1 <file>` signs:

- the launcher exe, before it is embedded;
- the uninstaller, through `!uninstfinalize`;
- the installer.

It reads environment variables only (GitHub secrets in CI), and the first
configured method wins:

| Method | Variables |
|---|---|
| Azure Artifact Signing | `AZURE_ARTIFACT_SIGNING_ENDPOINT`, `_ACCOUNT`, `_PROFILE` (optional `_DLIB`; otherwise Microsoft.ArtifactSigning.Client 1.0.128 is fetched from nuget.org), plus `AZURE_CLIENT_ID` / `AZURE_CLIENT_SECRET` / `AZURE_TENANT_ID` |
| PFX | `WINDOWS_SIGN_PFX_BASE64`, `WINDOWS_SIGN_PFX_PASSWORD` |
| Certificate in the user store (hardware token, cloud HSM) | `WINDOWS_SIGN_CERT_THUMBPRINT` |

`WINDOWS_SIGN_TIMESTAMP_URL` overrides the RFC 3161 server, DigiCert by default.
Azure always uses Microsoft's own server.

With no method configured, every file is left unsigned and a `[firma] AVISO`
warning is printed, which is also a GitHub annotation in CI.
`WINDOWS_SIGN_REQUIRED=1` makes a missing identity an error.

Validly signed files are skipped, so the script is idempotent. Sign the exe
before computing the SHA-256 that a Distribution release publishes.

The PE metadata (`CompanyName`, `ProductName`, versions, `OriginalFilename`) is
set in `crates/pandora_launcher/build.rs` and in the template.
`CompanyName` ("Wachiland Elite") should match the certificate subject.
