#!/bin/bash
# Release build for Windows: the launcher exe, a portable copy and the installer.
# Usage: scripts/build_windows.sh <version>   (e.g. 1.0.0 or v1.0.0)
#
# The installer does not embed the launcher: crates/setup_fetch downloads the
# latest `player` release from Distribution (origin from
# WBOMBO_DISTRIBUTION_DEFAULT_URL, like the launcher) and verifies it before
# installing. The launcher exe built here is the one to publish there.
#
# Signing is optional and pluggable: scripts/sign_windows.ps1 signs the launcher,
# the uninstaller and the installer when a signing secret is configured, and
# prints a warning otherwise. See docs/features/windows-installer.md.
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
template=$(json_path target/installer/installer.nsi)
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

# The installer's downloader, built small (profile `setup`) and signed like the rest.
cargo build --profile setup --frozen --target x86_64-pc-windows-msvc -p setup_fetch
cp target/x86_64-pc-windows-msvc/setup/wachiland-setup-fetch.exe target/installer/wachiland-setup-fetch.exe
$sign target/installer/wachiland-setup-fetch.exe
fetch_exe=$(cygpath -w "$PWD/target/installer/wachiland-setup-fetch.exe" 2>/dev/null || echo "$PWD/target/installer/wachiland-setup-fetch.exe")
python - "$fetch_exe" <<'PY'
import sys
template = open("package/windows/installer.nsi", encoding="utf-8").read()
assert template.count("@SETUP_FETCH_EXE@") == 1
open("target/installer/installer.nsi", "w", encoding="utf-8", newline="\n").write(template.replace("@SETUP_FETCH_EXE@", sys.argv[1]))
PY
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
    "template": "$template",
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
# The setup is shared on Discord, whose upload limit without Nitro is 10 MB.
setup_bytes=$(stat -c %s "$setup")
echo "instalador: $setup_bytes bytes"
[ "$setup_bytes" -lt 10000000 ] || { echo "el instalador pasa de 10 MB"; exit 1; }
# cargo-packager signs the installer itself; this only reports if it did not.
$sign "$setup"

sha256sum dist/WachilandLauncher-Windows-x86_64-*.exe
