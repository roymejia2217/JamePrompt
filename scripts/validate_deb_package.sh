#!/usr/bin/env bash
set -euo pipefail

PACKAGE_PATH="${1:-}"
CARGO_TOML="${2:-Cargo.toml}"
EXPECTED_NAME="jame-prompt"
EXPECTED_BINARY="usr/bin/jame-prompt"
EXPECTED_DESKTOP="usr/share/applications/io.github.roymejia2217.JamePrompt.desktop"

fail() {
    echo "Debian package validation failed: $1" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

for command_name in dpkg-deb dpkg awk grep lintian mktemp; do
    require_command "$command_name"
done

[[ -n "$PACKAGE_PATH" ]] || fail "package path is required"
[[ -s "$PACKAGE_PATH" ]] || fail "package is missing or empty: $PACKAGE_PATH"
[[ -f "$CARGO_TOML" ]] || fail "Cargo manifest is missing: $CARGO_TOML"

UPSTREAM_VERSION="$(awk -F'"' '/^version = "/ { print $2; exit }' "$CARGO_TOML")"
[[ -n "$UPSTREAM_VERSION" ]] || fail "unable to read package version from Cargo.toml"

EXPECTED_VERSION="${UPSTREAM_VERSION%%-*}"
if [[ "$UPSTREAM_VERSION" != "$EXPECTED_VERSION" ]]; then
    EXPECTED_VERSION="${EXPECTED_VERSION}~${UPSTREAM_VERSION#*-}"
fi
EXPECTED_ARCH="$(dpkg --print-architecture)"

PACKAGE_NAME="$(dpkg-deb -f "$PACKAGE_PATH" Package)"
PACKAGE_VERSION="$(dpkg-deb -f "$PACKAGE_PATH" Version)"
PACKAGE_ARCH="$(dpkg-deb -f "$PACKAGE_PATH" Architecture)"
PACKAGE_DEPENDS="$(dpkg-deb -f "$PACKAGE_PATH" Depends)"

[[ "$PACKAGE_NAME" == "$EXPECTED_NAME" ]] ||
    fail "unexpected package name: expected '$EXPECTED_NAME', got '$PACKAGE_NAME'"
[[ "$PACKAGE_VERSION" == "$EXPECTED_VERSION" ]] ||
    fail "unexpected package version: expected '$EXPECTED_VERSION', got '$PACKAGE_VERSION'"
[[ "$PACKAGE_ARCH" == "$EXPECTED_ARCH" ]] ||
    fail "unexpected package architecture: expected '$EXPECTED_ARCH', got '$PACKAGE_ARCH'"

for dependency in libxkbcommon-x11-0 xdg-desktop-portal xdg-desktop-portal-gnome; do
    grep -Fq "$dependency" <<<"$PACKAGE_DEPENDS" ||
        fail "required runtime dependency is missing: $dependency"
done

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT
ROOT_DIR="$TEMP_DIR/root"
CONTROL_DIR="$TEMP_DIR/DEBIAN"
mkdir -p "$ROOT_DIR" "$CONTROL_DIR"

dpkg-deb --extract "$PACKAGE_PATH" "$ROOT_DIR"
dpkg-deb --control "$PACKAGE_PATH" "$CONTROL_DIR"

[[ -x "$ROOT_DIR/$EXPECTED_BINARY" ]] ||
    fail "package payload is missing executable $EXPECTED_BINARY"
[[ -f "$ROOT_DIR/$EXPECTED_DESKTOP" ]] ||
    fail "package payload is missing desktop entry $EXPECTED_DESKTOP"
[[ -x "$CONTROL_DIR/postinst" ]] ||
    fail "package control payload is missing executable DEBIAN/postinst"
[[ -x "$CONTROL_DIR/postrm" ]] ||
    fail "package control payload is missing executable DEBIAN/postrm"

lintian --fail-on error "$PACKAGE_PATH"

echo "Debian package validation passed: $EXPECTED_NAME $EXPECTED_VERSION ($EXPECTED_ARCH)"
