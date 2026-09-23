#!/usr/bin/env bash
set -euo pipefail

read_project_rust_version() {
    local manifest="${1:-Cargo.toml}"
    local minimum
    minimum="$(awk -F'"' '/^[[:space:]]*rust-version[[:space:]]*=/ { print $2; exit }' "$manifest")"
    if [[ -z "$minimum" ]]; then
        echo "Unable to read rust-version from $manifest" >&2
        return 1
    fi
    printf '%s\n' "$minimum"
}

version_at_least() {
    candidate="$1"
    minimum="$2"
    first="$(printf '%s\n%s\n' "$minimum" "$candidate" | sort -V | sed -n '1p')"
    [[ "$first" == "$minimum" ]]
}

MINIMUM_RUST_VERSION="$(read_project_rust_version Cargo.toml)"

self_test() {
    version_at_least "$MINIMUM_RUST_VERSION" "$MINIMUM_RUST_VERSION"
    version_at_least "999.0.0" "$MINIMUM_RUST_VERSION"

    if version_at_least "0.0.0" "$MINIMUM_RUST_VERSION"; then
        echo "Version gate accepted an unsupported Rust version" >&2
        exit 1
    fi

    echo "RPM builder version gate self-test passed"
}

if [[ "${1:-}" == "--self-test" ]]; then
    self_test
    exit 0
fi

dnf -y install \
    git \
    rpm-build \
    cargo \
    rust \
    desktop-file-utils \
    gtk3-devel \
    libX11-devel \
    libXtst-devel \
    libxkbcommon-devel \
    libxdo-devel \
    dbus-daemon

RUST_VERSION="$(rustc --version | awk '{print $2}')"
CARGO_VERSION="$(cargo --version | awk '{print $2}')"

if ! version_at_least "$RUST_VERSION" "$MINIMUM_RUST_VERSION"; then
    echo "Rust toolchain is below the required minimum: found $RUST_VERSION, need >= $MINIMUM_RUST_VERSION" >&2
    exit 1
fi

echo "Using rustc $RUST_VERSION and cargo $CARGO_VERSION from Fedora packages"

RPM_VERSION="$(awk '/^Version:[[:space:]]*/ { print $2; exit }' packaging/rpm/jame-prompt.spec)"
if [[ -z "$RPM_VERSION" ]]; then
    echo "Unable to read RPM version" >&2
    exit 1
fi

RPM_TOPDIR="$PWD/target/rpmbuild"
mkdir -p "$RPM_TOPDIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}

if [[ -n "${JAME_PROMPT_RPM_CARGO_TARGET_DIR:-}" ]]; then
    mkdir -p "$JAME_PROMPT_RPM_CARGO_TARGET_DIR"
    export CARGO_TARGET_DIR="$JAME_PROMPT_RPM_CARGO_TARGET_DIR"
fi

tar -czf "$RPM_TOPDIR/SOURCES/jame-prompt-${RPM_VERSION}.tar.gz" \
    --exclude="./target" \
    --exclude="./.git" \
    --transform="s#^\./#jame-prompt-${RPM_VERSION}/#" \
    -C "$PWD" .

rpmbuild \
    --define "_topdir $RPM_TOPDIR" \
    --define "_sourcedir $RPM_TOPDIR/SOURCES" \
    -ba packaging/rpm/jame-prompt.spec
