#!/usr/bin/env bash
set -euo pipefail

PACKAGE_PATH="${1:-}"
PKGBUILD_PATH="${2:-packaging/arch/PKGBUILD}"
EXPECTED_NAME="jame-prompt"
EXPECTED_ARCH="x86_64"
EXPECTED_BINARY="usr/bin/jame-prompt"

fail() {
    echo "Arch package validation failed: $1" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

for command_name in pacman bsdtar awk grep; do
    require_command "$command_name"
done

[[ -n "$PACKAGE_PATH" ]] || fail "package path is required"
[[ -s "$PACKAGE_PATH" ]] || fail "package is missing or empty: $PACKAGE_PATH"
[[ -f "$PKGBUILD_PATH" ]] || fail "PKGBUILD is missing: $PKGBUILD_PATH"

pkgver="$(awk -F= '$1 == "pkgver" { print $2; exit }' "$PKGBUILD_PATH")"
pkgrel="$(awk -F= '$1 == "pkgrel" { print $2; exit }' "$PKGBUILD_PATH")"
[[ -n "$pkgver" ]] || fail "pkgver is missing from PKGBUILD"
[[ -n "$pkgrel" ]] || fail "pkgrel is missing from PKGBUILD"
EXPECTED_VERSION="${pkgver}-${pkgrel}"

identity="$(pacman -Qp "$PACKAGE_PATH")"
[[ "$identity" == "$EXPECTED_NAME $EXPECTED_VERSION" ]] ||     fail "unexpected package identity: expected '$EXPECTED_NAME $EXPECTED_VERSION', got '$identity'"

info="$(pacman -Qip "$PACKAGE_PATH")"
if ! grep -Eq '^Architecture[[:space:]]*:[[:space:]]*x86_64[[:space:]]*$' <<<"$info"; then
    fail "package architecture is not $EXPECTED_ARCH"
fi

contents="$(bsdtar -tf "$PACKAGE_PATH")"
if ! grep -Fxq "$EXPECTED_BINARY" <<<"$contents"; then
    fail "package payload is missing $EXPECTED_BINARY"
fi

echo "Arch package validation passed: $EXPECTED_NAME $EXPECTED_VERSION ($EXPECTED_ARCH)"
