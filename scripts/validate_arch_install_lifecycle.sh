#!/usr/bin/env bash
set -euo pipefail

PACKAGE_PATH="${1:-}"
EXPECTED_NAME="jame-prompt"
EXPECTED_BINARY="/usr/bin/jame-prompt"
EXPECTED_DESKTOP="/usr/share/applications/io.github.roymejia2217.JamePrompt.desktop"

fail() {
    echo "Arch install lifecycle validation failed: $1" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

for command_name in pacman readlink id; do
    require_command "$command_name"
done

[[ "$(id -u)" == "0" ]] || fail "lifecycle validation must run as root"
[[ -n "$PACKAGE_PATH" ]] || fail "package path is required"
[[ -s "$PACKAGE_PATH" ]] || fail "package is missing or empty: $PACKAGE_PATH"

PACKAGE_PATH="$(readlink -f "$PACKAGE_PATH")"
PACKAGE_ID="$(pacman -Qp "$PACKAGE_PATH")"
PACKAGE_NAME="${PACKAGE_ID%% *}"
EXPECTED_VERSION="${PACKAGE_ID#* }"

[[ "$PACKAGE_NAME" == "$EXPECTED_NAME" ]] ||
    fail "unexpected package name: expected '$EXPECTED_NAME', got '$PACKAGE_NAME'"
[[ -n "$EXPECTED_VERSION" ]] ||
    fail "unable to read package version from $PACKAGE_PATH"

bash scripts/build_arch_package.sh --configure-snapshot

pacman -U --noconfirm "$PACKAGE_PATH"

INSTALLED_ID="$(pacman -Q "$EXPECTED_NAME")"
[[ "$INSTALLED_ID" == "$EXPECTED_NAME $EXPECTED_VERSION" ]] ||
    fail "unexpected installed package identity: expected '$EXPECTED_NAME $EXPECTED_VERSION', got '$INSTALLED_ID'"
[[ -x "$EXPECTED_BINARY" ]] ||
    fail "installed package payload is missing executable $EXPECTED_BINARY"
[[ -f "$EXPECTED_DESKTOP" ]] ||
    fail "installed package payload is missing desktop entry $EXPECTED_DESKTOP"

BINARY_OWNER="$(pacman -Qoq "$EXPECTED_BINARY")"
DESKTOP_OWNER="$(pacman -Qoq "$EXPECTED_DESKTOP")"
[[ "$BINARY_OWNER" == "$EXPECTED_NAME" ]] ||
    fail "unexpected owner for $EXPECTED_BINARY: $BINARY_OWNER"
[[ "$DESKTOP_OWNER" == "$EXPECTED_NAME" ]] ||
    fail "unexpected owner for $EXPECTED_DESKTOP: $DESKTOP_OWNER"

pacman -Rns --noconfirm "$EXPECTED_NAME"

if pacman -Q "$EXPECTED_NAME" >/dev/null 2>&1; then
    fail "package remained registered after removal"
fi
[[ ! -e "$EXPECTED_BINARY" ]] ||
    fail "package payload binary remained after removal: $EXPECTED_BINARY"
[[ ! -e "$EXPECTED_DESKTOP" ]] ||
    fail "package desktop entry remained after removal: $EXPECTED_DESKTOP"

echo "Arch install lifecycle validation passed: $EXPECTED_NAME $EXPECTED_VERSION"
