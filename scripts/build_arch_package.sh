#!/usr/bin/env bash
set -euo pipefail

ARCHIVE_DATE="2026/09/13"
ARCHIVE_SERVER="https://archive.archlinux.org/repos/${ARCHIVE_DATE}/\$repo/os/\$arch"

self_test() {
    [[ "$ARCHIVE_DATE" =~ ^[0-9]{4}/[0-9]{2}/[0-9]{2}$ ]]
    [[ "$ARCHIVE_SERVER" == "https://archive.archlinux.org/repos/${ARCHIVE_DATE}/\$repo/os/\$arch" ]]
    echo "Arch snapshot builder self-test passed"
}

if [[ "${1:-}" == "--self-test" ]]; then
    self_test
    exit 0
fi

printf 'Server = %s\n' "$ARCHIVE_SERVER" > /etc/pacman.d/mirrorlist

pacman-key --init
pacman-key --populate archlinux
pacman -Syy --noconfirm --needed \
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
