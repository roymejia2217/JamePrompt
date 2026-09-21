#!/usr/bin/env bash
set -euo pipefail

ARCHIVE_DATE="2026/09/13"
ARCHIVE_SERVER="https://archive.archlinux.org/repos/${ARCHIVE_DATE}/\$repo/os/\$arch"
MAX_DOWNLOAD_ATTEMPTS=3
RETRY_DELAY_SECONDS=5

retry_with_backoff() {
    local max_attempts="$1"
    local base_delay="$2"
    shift 2

    local attempt=1
    local status=0

    while true; do
        if "$@"; then
            return 0
        else
            status=$?
        fi

        if (( attempt >= max_attempts )); then
            echo "Command failed after ${attempt} attempt(s): $*" >&2
            return "$status"
        fi

        local sleep_seconds=$((base_delay * attempt))
        echo "Command failed on attempt ${attempt}/${max_attempts}; retrying in ${sleep_seconds}s: $*" >&2
        sleep "$sleep_seconds"
        attempt=$((attempt + 1))
    done
}

self_test() {
    [[ "$ARCHIVE_DATE" =~ ^[0-9]{4}/[0-9]{2}/[0-9]{2}$ ]]
    [[ "$ARCHIVE_SERVER" == "https://archive.archlinux.org/repos/${ARCHIVE_DATE}/\$repo/os/\$arch" ]]

    local transient_attempts=0
    transient_failure_then_success() {
        transient_attempts=$((transient_attempts + 1))
        (( transient_attempts >= 2 ))
    }

    permanent_failure() {
        return 17
    }

    retry_with_backoff 3 0 transient_failure_then_success
    [[ "$transient_attempts" -eq 2 ]]

    if retry_with_backoff 2 0 permanent_failure; then
        echo "Permanent failure unexpectedly succeeded" >&2
        return 1
    fi

    echo "Arch snapshot builder retry self-test passed"
}

if [[ "${1:-}" == "--self-test" ]]; then
    self_test
    exit 0
fi

printf 'Server = %s\n' "$ARCHIVE_SERVER" > /etc/pacman.d/mirrorlist

pacman-key --init
pacman-key --populate archlinux
retry_with_backoff "$MAX_DOWNLOAD_ATTEMPTS" "$RETRY_DELAY_SECONDS" pacman -Syy --noconfirm --needed \
    archlinux-keyring \
    git \
    rust \
    cargo \
    desktop-file-utils \
    gtk3 \
    libappindicator-gtk3 \
    libx11 \
    libxtst \
    libxkbcommon \
    xdotool \
    fontconfig \
    freetype2 \
    gdk-pixbuf2 \
    hicolor-icon-theme \
    pkgconf \
    sqlite

useradd -m builder
mkdir -p /tmp/arch-build
tar \
    --exclude=./.git \
    --exclude=./target \
    --exclude=./packaging/arch/pkg \
    --exclude=./packaging/arch/src \
    --exclude=./packaging/arch/target \
    -cf - -C /workspace . | tar -xf - -C /tmp/arch-build

chown -R builder:builder /tmp/arch-build
su builder -c "cd /tmp/arch-build/packaging/arch && LIBSQLITE3_SYS_USE_PKG_CONFIG=1 makepkg --nodeps --noconfirm --cleanbuild --clean"
cp /tmp/arch-build/packaging/arch/*.pkg.tar.zst /workspace/packaging/arch/
