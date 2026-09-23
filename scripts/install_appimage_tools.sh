#!/usr/bin/env bash
set -euo pipefail

DESTINATION="${1:-target/release-tools}"
LINUXDEPLOY_ASSET_ID="538917371"
LINUXDEPLOY_SHA256="36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62"
APPIMAGETOOL_ASSET_ID="324406736"
APPIMAGETOOL_SHA256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
GITHUB_API_VERSION="2026-03-10"

require_command() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "Missing required command: $1" >&2
        exit 1
    }
}

download_verified_tool() {
    repo="$1"
    asset_id="$2"
    sha256="$3"
    output="$4"
    temp_file="$(mktemp)"

    if ! curl --fail --location --silent --show-error \
        --proto '=https' \
        --tlsv1.2 \
        -H "Accept: application/octet-stream" \
        -H "X-GitHub-Api-Version: ${GITHUB_API_VERSION}" \
        "https://api.github.com/repos/${repo}/releases/assets/${asset_id}" \
        --output "$temp_file"; then
        rm -f "$temp_file"
        return 1
    fi

    printf '%s  %s\n' "$sha256" "$temp_file" | sha256sum --check --strict
    install -Dm755 "$temp_file" "$output"
    rm -f "$temp_file"
}

for command_name in curl sha256sum install mktemp; do
    require_command "$command_name"
done

mkdir -p "$DESTINATION"

download_verified_tool \
    "linuxdeploy/linuxdeploy" \
    "$LINUXDEPLOY_ASSET_ID" \
    "$LINUXDEPLOY_SHA256" \
    "$DESTINATION/linuxdeploy"

download_verified_tool \
    "AppImage/appimagetool" \
    "$APPIMAGETOOL_ASSET_ID" \
    "$APPIMAGETOOL_SHA256" \
    "$DESTINATION/appimagetool"

printf '%s\n' "$DESTINATION"
