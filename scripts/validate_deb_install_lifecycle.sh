#!/usr/bin/env bash
set -euo pipefail

PACKAGE_PATH="${1:-}"
EXPECTED_NAME="jame-prompt"
EXPECTED_BINARY="/usr/bin/jame-prompt"
EXPECTED_DESKTOP="/usr/share/applications/io.github.roymejia2217.JamePrompt.desktop"

fail() {
    echo "Debian install lifecycle validation failed: $1" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

for command_name in apt-get dpkg-deb dpkg-query readlink dirname basename grep; do
    require_command "$command_name"
done

[[ "$(id -u)" == "0" ]] || fail "lifecycle validation must run as root"
[[ -n "$PACKAGE_PATH" ]] || fail "package path is required"
[[ -s "$PACKAGE_PATH" ]] || fail "package is missing or empty: $PACKAGE_PATH"

PACKAGE_PATH="$(readlink -f "$PACKAGE_PATH")"
PACKAGE_NAME="$(dpkg-deb -f "$PACKAGE_PATH" Package)"
[[ "$PACKAGE_NAME" == "$EXPECTED_NAME" ]] ||
    fail "unexpected package name: expected '$EXPECTED_NAME', got '$PACKAGE_NAME'"

apt-get update

PACKAGE_DIR="$(dirname "$PACKAGE_PATH")"
PACKAGE_FILE="./$(basename "$PACKAGE_PATH")"
(
    cd "$PACKAGE_DIR"
    DEBIAN_FRONTEND=noninteractive apt-get install -y "$PACKAGE_FILE"
)

PACKAGE_STATUS="$(dpkg-query -W -f='${Status}' "$EXPECTED_NAME")"
[[ "$PACKAGE_STATUS" == "install ok installed" ]] ||
    fail "unexpected installed package status: $PACKAGE_STATUS"
[[ -x "$EXPECTED_BINARY" ]] ||
    fail "installed package payload is missing executable $EXPECTED_BINARY"
[[ -f "$EXPECTED_DESKTOP" ]] ||
    fail "installed package payload is missing desktop entry $EXPECTED_DESKTOP"

dpkg-query -L "$EXPECTED_NAME" | grep -Fxq "$EXPECTED_BINARY" ||
    fail "dpkg database does not own expected binary $EXPECTED_BINARY"
dpkg-query -L "$EXPECTED_NAME" | grep -Fxq "$EXPECTED_DESKTOP" ||
    fail "dpkg database does not own expected desktop entry $EXPECTED_DESKTOP"

DEBIAN_FRONTEND=noninteractive apt-get purge -y "$EXPECTED_NAME"

if dpkg-query -W "$EXPECTED_NAME" >/dev/null 2>&1; then
    fail "package remained registered after purge"
fi
[[ ! -e "$EXPECTED_BINARY" ]] ||
    fail "package payload binary remained after purge: $EXPECTED_BINARY"
[[ ! -e "$EXPECTED_DESKTOP" ]] ||
    fail "package desktop entry remained after purge: $EXPECTED_DESKTOP"

echo "Debian install lifecycle validation passed: $EXPECTED_NAME"
