#!/usr/bin/env bash
set -euo pipefail

PACKAGE_PATH="${1:-}"
SPEC_PATH="${2:-packaging/rpm/jame-prompt.spec}"
EXPECTED_BINARY="/usr/bin/jame-prompt"
EXPECTED_DESKTOP="/usr/share/applications/io.github.roymejia2217.JamePrompt.desktop"

fail() {
    echo "RPM package validation failed: $1" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

for command_name in rpm awk grep; do
    require_command "$command_name"
done

[[ -n "$PACKAGE_PATH" ]] || fail "package path is required"
[[ -s "$PACKAGE_PATH" ]] || fail "package is missing or empty: $PACKAGE_PATH"
[[ -f "$SPEC_PATH" ]] || fail "RPM spec is missing: $SPEC_PATH"

spec_value() {
    local field="$1"
    awk -v field="$field" '
        $0 ~ "^" field ":[[:space:]]*" {
            value = $0
            sub("^[^:]+:[[:space:]]*", "", value)
            print value
            exit
        }
    ' "$SPEC_PATH"
}

EXPECTED_NAME="$(spec_value Name)"
EXPECTED_VERSION="$(spec_value Version)"
SPEC_RELEASE="$(spec_value Release)"
EXPECTED_RELEASE="$(rpm --eval "$SPEC_RELEASE")"
EXPECTED_ARCH="$(rpm --eval '%{_arch}')"
EXPECTED_SUMMARY="$(spec_value Summary)"
EXPECTED_LICENSE="$(spec_value License)"
EXPECTED_URL="$(spec_value URL)"

for metadata in \
    "$EXPECTED_NAME" \
    "$EXPECTED_VERSION" \
    "$SPEC_RELEASE" \
    "$EXPECTED_RELEASE" \
    "$EXPECTED_ARCH" \
    "$EXPECTED_SUMMARY" \
    "$EXPECTED_LICENSE" \
    "$EXPECTED_URL"; do
    [[ -n "$metadata" ]] || fail "required metadata is missing from $SPEC_PATH"
done

PACKAGE_NAME="$(rpm -qp --queryformat '%{NAME}' "$PACKAGE_PATH")"
PACKAGE_VERSION="$(rpm -qp --queryformat '%{VERSION}' "$PACKAGE_PATH")"
PACKAGE_RELEASE="$(rpm -qp --queryformat '%{RELEASE}' "$PACKAGE_PATH")"
PACKAGE_ARCH="$(rpm -qp --queryformat '%{ARCH}' "$PACKAGE_PATH")"
PACKAGE_SUMMARY="$(rpm -qp --queryformat '%{SUMMARY}' "$PACKAGE_PATH")"
PACKAGE_LICENSE="$(rpm -qp --queryformat '%{LICENSE}' "$PACKAGE_PATH")"
PACKAGE_URL="$(rpm -qp --queryformat '%{URL}' "$PACKAGE_PATH")"

[[ "$PACKAGE_NAME" == "$EXPECTED_NAME" ]] ||
    fail "unexpected package name: expected '$EXPECTED_NAME', got '$PACKAGE_NAME'"
[[ "$PACKAGE_VERSION" == "$EXPECTED_VERSION" ]] ||
    fail "unexpected package version: expected '$EXPECTED_VERSION', got '$PACKAGE_VERSION'"
[[ "$PACKAGE_RELEASE" == "$EXPECTED_RELEASE" ]] ||
    fail "unexpected package release: expected '$EXPECTED_RELEASE', got '$PACKAGE_RELEASE'"
[[ "$PACKAGE_ARCH" == "$EXPECTED_ARCH" ]] ||
    fail "unexpected package architecture: expected '$EXPECTED_ARCH', got '$PACKAGE_ARCH'"
[[ "$PACKAGE_SUMMARY" == "$EXPECTED_SUMMARY" ]] ||
    fail "unexpected package summary: expected '$EXPECTED_SUMMARY', got '$PACKAGE_SUMMARY'"
[[ "$PACKAGE_LICENSE" == "$EXPECTED_LICENSE" ]] ||
    fail "unexpected package license: expected '$EXPECTED_LICENSE', got '$PACKAGE_LICENSE'"
[[ "$PACKAGE_URL" == "$EXPECTED_URL" ]] ||
    fail "unexpected package URL: expected '$EXPECTED_URL', got '$PACKAGE_URL'"

PACKAGE_REQUIRES="$(rpm -qp --queryformat '[%{REQUIRENAME}\n]' "$PACKAGE_PATH")"
SPEC_REQUIRES="$(awk '$1 == "Requires:" { print $2 }' "$SPEC_PATH")"
[[ -n "$SPEC_REQUIRES" ]] || fail "no runtime Requires: entries found in $SPEC_PATH"

while IFS= read -r dependency; do
    [[ -n "$dependency" ]] || continue
    grep -Fxq "$dependency" <<<"$PACKAGE_REQUIRES" ||
        fail "required runtime dependency is missing: $dependency"
done <<<"$SPEC_REQUIRES"

PACKAGE_FILES="$(rpm -qp --queryformat '[%{FILEMODES:perms}\t%{FILENAMES}\n]' "$PACKAGE_PATH")"
BINARY_MODE="$(awk -F '\t' -v path="$EXPECTED_BINARY" '$2 == path { print $1; exit }' <<<"$PACKAGE_FILES")"

[[ -n "$BINARY_MODE" ]] ||
    fail "package payload is missing executable $EXPECTED_BINARY"
[[ "$BINARY_MODE" == "-rwxr-xr-x" ]] ||
    fail "package payload binary is not executable: expected '-rwxr-xr-x', got '$BINARY_MODE'"
grep -Fq $'\t'"$EXPECTED_DESKTOP" <<<"$PACKAGE_FILES" ||
    fail "package payload is missing desktop entry $EXPECTED_DESKTOP"

echo "RPM package validation passed: $EXPECTED_NAME $EXPECTED_VERSION-$EXPECTED_RELEASE ($EXPECTED_ARCH)"
