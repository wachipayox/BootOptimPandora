# Windows installer

`scripts/build_windows.sh <version>` produces, in `dist/`:

- `WachilandLauncher-Windows-x86_64-Setup.exe`: the installer, about 2.3 MB. It
  does not contain the launcher: it downloads the latest published one (below).
  The build fails if it reaches 10 MB, Discord's upload limit without Nitro.
- `WachilandLauncher-Windows-x86_64-Portable.exe`: the launcher exe. Its name
  contains "portable", so it keeps its data next to itself. This is also the
  exe to publish as the Windows artifact of a Distribution release.

`LAUNCHER_EXE=<path>` packages an already built exe instead of running Cargo.

## Download

The wizard downloads the launcher while installing, so a setup never goes
stale and stays small. `crates/setup_fetch` (`wachiland-setup-fetch.exe`, built
with the size-oriented `setup` profile, about 1.9 MB) runs from `$PLUGINSDIR`:

1. `GET /v1/launcher/pandora/releases?platform=windows-x86_64&channel=player`
   and takes the newest release.
2. Verifies it with `crates/launcher_artifact`, the same code as the in-place
   updater: Ed25519 over the exact statement bytes, with the built-in release
   key or a key discovered at `/v1/signing-keys` over HTTPS that never gives
   the built-in id another key; then app, version, platform, size, SHA-256 and
   the CAS path. Redirects are not followed.
3. Streams `/v1/objects/sha256/<sha256>` into `WachilandLauncher.exe.wbombo-new-setup.part`
   next to the installed exe, checks size and SHA-256, and only then renames it.
   It moves the wizard's progress bar and status line through their window
   handles, and writes its log lines to the details view.

The origin is `WBOMBO_DISTRIBUTION_DEFAULT_URL`, the launcher's own build-time
default (a GitHub secret in CI), so the server address is not in the
repository. A build without it produces a setup that only reports that.

Nothing is installed until the download is verified: the registry, shortcuts
and uninstaller are written afterwards, and the old exe is swapped out by
renames that are undone on failure. Exit codes and what the wizard does:

| Code | Meaning | Wizard |
|---|---|---|
| 0 | verified exe ready | installs it |
| 20 | server unreachable or not answering | "Reintentar" / "Cancelar" |
| 21 | no release published for Windows yet | "Reintentar" / "Cancelar" |
| 30 | index, signature, size or SHA-256 rejected | error, nothing installed |
| other | local error, or built without an origin | error, nothing installed |

A cancelled or failed install removes the partial download and, on a new
install, the empty folder; silent and passive runs exit with code 3.

A downloaded exe carries no Mark-of-the-Web, so only the setup itself goes
through SmartScreen.

A setup that embeds the launcher is no longer built. It would only help an
install without internet, and the launcher needs the server anyway to install
any profile; the portable exe covers that case.

### Testing without the real server

`scripts/test_launcher_release_server.py` (needs `cryptography`) serves one
synthetic release of a given exe from `https://127.0.0.1`, signed with a
throwaway key and a throwaway CA, in the modes `ok`, `bad-signature`,
`bad-hash`, `no-release` and `server-error` (`--rate` slows the download).
`WBOMBO_SETUP_TEST_ORIGIN` and `WBOMBO_SETUP_TEST_CA` point the setup at it;
the downloader accepts that origin only on a loopback address.
`scripts/verify_windows_installer.ps1` runs the whole matrix in CI.

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
page, closes a running launcher after asking, and replaces the exe with the
downloaded one. `DisplayVersion` and `EstimatedSize` are the downloaded
release's. The installed exe may have been updated by the launcher itself, while
the registry `DisplayVersion` is not updated. So the installer compares the
downloaded version against the exe's own `FILEVERSION` and asks before replacing
a newer launcher (silent runs refuse).

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
   `servers.dat`, `hotbar.nbt`, `command_history.txt`, `ponders_watched.json`
   (merged into the globally synced list), and the resource packs and
   shaders the player added (in neither the old AutoModpack list nor the new
   profile). Mods and other configs are not copied; the mods the player had added
   are listed in `.minecraft/mods-del-launcher-anterior.txt`.
4. Makes the old `options.txt` the [global Minecraft settings](global-minecraft-settings.md),
   which new instances are created with, and turns them on.

If the profile cannot be installed (no connection…), the half-made instance is
removed and nothing is copied. The finish page then says so; otherwise it
reminds the player that the old folder was not deleted and where it is.

The command-line run creates `config.json` without opening a window. A
`first-open-pending` marker keeps the first window's intro the first-install
one.

Silent use: `/MIGRATE=<folder>` and `/MIGRATEVOXY`.

## Code signing

`scripts/sign_windows.ps1 <file>` signs:

- the launcher exe, before it is published;
- the setup's downloader, before it is embedded;
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
