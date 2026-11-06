#!/bin/bash
# Release build for Windows: the launcher exe, a portable copy and the installer.
# Usage: scripts/build_windows.sh <version>   (e.g. 1.0.0 or v1.0.0)
#
# Signing is optional and pluggable: scripts/sign_windows.ps1 signs the launcher,
# the uninstaller and the installer when a signing secret is configured, and
# prints a warning otherwise. See docs/windows-installer.md.
set -e

if [ -z "$1" ]; then
    echo "Missing version argument"
    exit 1
fi

version=${1#v}
export PANDORA_RELEASE_VERSION=$version
icon_root=$(bash scripts/package_icon_root.sh)
root=$(pwd -W 2>/dev/null || pwd)
# makensis rejects forward-slash drive paths in `File`, so files embedded by
# the template are handed to cargo-packager with backslashes (escaped for JSON).
json_path() {
    local path
    path=$(cygpath -w "$PWD/$1" 2>/dev/null || echo "$PWD/$1")
    echo "${path//\\/\\\\}"
}
main_exe=$(json_path target/installer/WachilandLauncher.exe)
sidebar=$(json_path target/installer/art/installer-sidebar.bmp)
header=$(json_path target/installer/art/installer-header.bmp)
sign="powershell -NoProfile -ExecutionPolicy Bypass -File $root/scripts/sign_windows.ps1"

# LAUNCHER_EXE packages an already built launcher instead of building one.
built=${LAUNCHER_EXE:-}
if [ -z "$built" ]; then
    cargo build --release --frozen --target x86_64-pc-windows-msvc
    built=target/x86_64-pc-windows-msvc/release/pandora_launcher.exe
    strip "$built"
fi

mkdir -p dist target/installer
# The installed name must not contain "portable": that word switches the data
# directory to the exe's folder (crates/pandora_launcher/src/main.rs).
cp "$built" target/installer/WachilandLauncher.exe
$sign target/installer/WachilandLauncher.exe

python -c "import PIL" 2>/dev/null || python -m pip install --quiet "pillow==11.3.0"
art_icon=release
[ "$icon_root" = "package/development" ] && art_icon=development
python package/windows/generate_installer_art.py target/installer/art "$art_icon"

config=$(cat <<EOF
{
  "name": "wachiland-launcher",
  "outDir": "./dist",
  "productName": "Wachiland Launcher",
  "version": "$version",
  "identifier": "com.moulberry.pandoralauncher",
  "publisher": "Wachiland Elite",
  "authors": ["Wachiland Elite", "Moulberry"],
  "copyright": "Wachiland Elite. Basado en Pandora Launcher de Moulberry.",
  "description": "Launcher de Wachiland Elite",
  "resources": [],
  "binaries": [{ "path": "$main_exe", "main": true }],
  "icons": ["$root/$icon_root/windows.ico"],
  "formats": ["nsis"],
  "windows": { "signCommand": "$sign %1", "allowDowngrades": true },
  "nsis": {
    "template": "$root/package/windows/installer.nsi",
    "installerIcon": "$root/$icon_root/windows.ico",
    "headerImage": "$header",
    "sidebarImage": "$sidebar",
    "installMode": "currentUser",
    "languages": ["Spanish"],
    "customLanguageFiles": { "Spanish": "$root/package/windows/Spanish.nsh" }
  }
}
EOF
)

if ! command -v cargo-packager >/dev/null 2>&1; then
    cargo install cargo-packager --version 0.11.8 --locked
fi
env -u CARGO_PACKAGER_SIGN_PRIVATE_KEY cargo packager --config "$config"

setup="dist/WachilandLauncher-Windows-x86_64-Setup.exe"
mv -f "dist/WachilandLauncher_${version}_x64-setup.exe" "$setup"
cp -f target/installer/WachilandLauncher.exe dist/WachilandLauncher-Windows-x86_64-Portable.exe
# cargo-packager signs the installer itself; this only reports if it did not.
$sign "$setup"

sha256sum dist/WachilandLauncher-Windows-x86_64-*.exe
