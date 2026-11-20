#!/usr/bin/env bash
# Run from the repository root after exporting PANDORA_RELEASE_VERSION.
set -euo pipefail
icon_policy_temp=$(mktemp -d)
trap 'rm -rf "$icon_policy_temp"' EXIT
rustc --edition 2024 package/icon_policy.rs -o "$icon_policy_temp/icon-policy.exe"
PROFILE=release "$icon_policy_temp/icon-policy.exe"
