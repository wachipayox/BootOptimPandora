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

## Migration from another launcher

An optional page after the folder page offers to migrate the player's
Wachiland Elite folder from another launcher. It is unticked by default. It
suggests the first folder in `%APPDATA%` with an AutoModpack `*Wachiland*`
modpack, and "Examinar…" always opens in `%APPDATA%`, because every player's
folder has a different name. The page says that the folder is only copied. A
`.voxy` box (unticked) adds the Voxy level-of-detail data.

After installing, the installer runs the launcher headless:

```
WachilandLauncher.exe --migrate-legacy <folder> --from-profile profile_wachiland-elite --migrate-name "Wachiland Elite" [--migrate-voxy]
```

`crates/backend/src/legacy_migration.rs` then:

1. Takes the revision the launcher itself installs (the `stable` channel, or the
   latest one), and copies pack files the player already has (same path, size
   and SHA-256 as the signed objects) into the download cache.
2. Creates the instance exactly like the GUI. It belongs to the player, not to
   the command line, so `--delete-instance` cannot remove it. Copying player files
   first would turn pack paths into local files that never update.
3. Copies only: worlds (one by one; a name taken gets "(migrado)"),
   `screenshots`, `xaero`, `journeymap`, `schematics`, `Sable-Schematics`,
   `music_sheets`, `replay_recordings`, `config/litematica`, `options.txt`,
   `servers.dat`, `hotbar.nbt`, `command_history.txt`, and the resource packs and
   shaders the player added (in neither the old AutoModpack list nor the new
   profile). Mods and other configs are not copied; the mods the player had added
   are listed in `.minecraft/mods-del-launcher-anterior.txt`.
4. Makes the old `options.txt` the [global Minecraft settings](global-minecraft-settings.md)
   and turns them on.

If the profile cannot be installed (no connection…), the half-made instance is
removed and nothing is copied. The finish page then says so; otherwise it
reminds the player that the old folder was not deleted and where it is.

The command-line run creates `config.json` without opening a window. A
`first-open-pending` marker keeps the first window's intro the first-install
one.

Silent use: `/MIGRATE=<folder>` and `/MIGRATEVOXY`.

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
