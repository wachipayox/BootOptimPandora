# Wachiland Launcher for Android — 0.1.0 alpha

First mobile prototype, based on a pinned FCL / HMCL / Amethyst Android engine.
This is a companion client for the same Distribution server. It is not an Android
compilation of GPUI or the desktop Rust binary.

## Included

- Landscape orientation across launcher, game, dialogs and crash pages.
- Dark Wachiland home, touch sized profile and instance cards, original profile
  icons, native account/settings/mod management and game controls.
- Microsoft browser authentication using the desktop launcher's public client ID.
- Distribution catalog, immutable signed revisions, up to eight ancestors,
  content-addressed downloads with SHA-256 verification, fresh configuration rules.
- Minecraft / NeoForge installation through the engine, installation progress and
  normal native game launch. Each installed pack uses an isolated game directory.
- Incomplete installations cannot start; launcher folders use app scoped storage.
- Separate app ID `net.wachiland.launcher`, persistent private signing certificate,
  no original engine automatic updater or prelaunch advertisements.

## Prototype boundaries

Android 8.0/API 26 or newer, ARM64 only. The build includes Java runtimes and native
rendering libraries, not Minecraft or paid game assets. A licensed Minecraft Java
account is required for Microsoft launch.

Local derived branches, shared save groups, parent update reconciliation, the
desktop diff editor and its global ignored-path settings are not ported yet. The
catalog currently presents individual profiles rather than the desktop family
tree. Configuration rules support ordinary TOML tables/keys, properties and
physical `text_lines`; unsupported TOML constructs fail explicitly. This prototype
only installs new instances and never upgrades existing trees.

Desktop mods with native libraries or desktop graphics assumptions may require
Android variants or removal. The APK building successfully does not establish
that the user's complete NeoForge pack runs on their device.

## Source and licensing

Engine: https://github.com/FCL-Team/FoldCraftLauncher

Pinned upstream commit: `5e76d7485d6ca34fe2adf86b31156f2714f5ccd9`.

Desktop integration source at start: `8c9d18ca9021103f4735b1a36f57d781091c7755`.

`wachiland.patch` contains all mobile source changes, including new source files.
The mobile code is GPL-3.0-or-later as required by the engine; see `COPYING` and
preserved engine/dependency notices. This does not change the desktop repository's
license. Supply the corresponding modified source with the APK to recipients.
The delivered source archive contains the complete modified engine and this
reproduction bundle. Signing credentials are excluded.

## Rebuild on Windows

Install Git, JDK 21, Android SDK platform 35, NDK `27.0.12077973` and CMake `3.22.1`.
Gradle 8.14.4 is downloaded by the upstream wrapper; dependency downloads require
network access. Then run:

```powershell
.\build.ps1 -AndroidSdk 'C:\path\to\Android\Sdk' -JavaHome 'C:\path\to\jdk-21'
```

The script prepares a pinned engine checkout, applies the patch, creates a signing
key automatically in `%LOCALAPPDATA%\WachilandBuild\private-signing`, and emits an
APK plus checksum in `out`. Back up the builder's key and password for future
compatible updates. Users installing the APK do not handle these files. A newly
generated developer key will not update an APK signed with the delivery key.

The delivered alpha uses the debug Gradle variant with a dedicated persistent
certificate and `testOnly=false`; it can be installed normally without ADB.

## Installation / first run

1. Transfer the APK to the phone and allow installation from the app opening it.
2. Open Wachiland Launcher, complete first-run runtime setup, and sign in through
   Microsoft. The launcher remains horizontal; the external browser has its own UI.
3. Select a global profile, create its instance, wait for installation, then Start.

Allocate enough free storage for the game, full modpack and download cache. Logs
are under the application's scoped `files/logs` folder and can be obtained with
the engine's log / crash export tools. Do not uninstall to troubleshoot a crash
before exporting worlds: Android uninstallation removes app scoped files.

## Evidence recorded for this build

Compilation and APK signature/manifest inspection are recorded in `BUILD.md`.
No physical Android device was attached during implementation. Microsoft's device
authorization endpoint accepted the public client ID; user account authentication,
touch layout and actual Minecraft / modpack startup still need the phone run.
