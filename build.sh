#!/usr/bin/env bash
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_ID="jame-prompt"
APP_NAME="JamePrompt"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)"
TARGET_DIR="$ROOT_DIR/target"
RPM_TOPDIR="$TARGET_DIR/rpmbuild"

FAILED_TARGETS=()
SKIPPED_TARGETS=()

log() {
    printf '%s\n' "$*"
}

command_exists() {
    command -v "$1" >/dev/null 2>&1
}

record_failure() {
    FAILED_TARGETS+=("$1")
    log "FAIL $1: $2"
}

record_skip() {
    SKIPPED_TARGETS+=("$1")
    log "SKIP $1: $2"
}

run_checked() {
    local target_name="$1"
    shift

    if "$@"; then
        log "DONE $target_name"
        return 0
    fi

    record_failure "$target_name" "command failed with exit code $?"
    return 1
}

clean_target() {
    log "CLEAN target"
    if rm -rf "$TARGET_DIR"; then
        log "DONE clean"
    else
        record_failure "clean" "unable to remove target directory"
    fi
}

build_release() {
    log "BUILD cargo"
    if (cd "$ROOT_DIR" && cargo build --release --locked); then
        log "DONE cargo"
    else
        record_failure "cargo" "cargo build --release --locked failed"
    fi
}

build_debian() {
    for tool in python3 dpkg dpkg-deb; do
        if ! command_exists "$tool"; then
            record_skip "debian" "missing required tool: $tool"
            return 0
        fi
    done

    log "BUILD debian"
    if (cd "$ROOT_DIR" && JAME_PROMPT_REUSE_RELEASE_BUILD=1 bash packaging/linux/build-deb.sh); then
        log "DONE debian"
    else
        record_failure "debian" "packaging/linux/build-deb.sh failed"
    fi
}

build_arch() {
    if ! command_exists makepkg; then
        record_skip "arch" "missing required tool: makepkg"
        return 0
    fi

    log "BUILD arch"
    if (cd "$ROOT_DIR/packaging/arch" && JAME_PROMPT_REUSE_RELEASE_BUILD=1 makepkg -sf --noconfirm); then
        log "DONE arch"
    else
        record_failure "arch" "makepkg -sf --noconfirm failed"
    fi
}

prepare_rpm_source() {
    local source_dir="$RPM_TOPDIR/SOURCES"
    local source_archive="$source_dir/${APP_ID}-${VERSION}.tar.gz"

    rm -rf "$RPM_TOPDIR"
    mkdir -p "$RPM_TOPDIR/BUILD" "$RPM_TOPDIR/BUILDROOT" "$RPM_TOPDIR/RPMS" "$RPM_TOPDIR/SOURCES" "$RPM_TOPDIR/SPECS" "$RPM_TOPDIR/SRPMS"
    tar -czf "$source_archive" \
        --exclude="./target" \
        --exclude="./.git" \
        --transform="s#^\./#${APP_ID}-${VERSION}/#" \
        -C "$ROOT_DIR" .
}

build_rpm() {
    for tool in tar rpmbuild; do
        if ! command_exists "$tool"; then
            record_skip "rpm" "missing required tool: $tool"
            return 0
        fi
    done

    log "BUILD rpm"
    if ! prepare_rpm_source; then
        record_failure "rpm" "failed to prepare source archive"
        return 0
    fi

    local rpm_output
    rpm_output="$(mktemp)"

    if JAME_PROMPT_REUSE_RELEASE_BUILD=1 rpmbuild \
        --define "_topdir $RPM_TOPDIR" \
        --define "_sourcedir $RPM_TOPDIR/SOURCES" \
        -ba "$ROOT_DIR/packaging/rpm/jame-prompt.spec" \
        >"$rpm_output" 2>&1; then
        rm -f "$rpm_output"
        log "DONE rpm"
    else
        if grep -q "Failed build dependencies:" "$rpm_output"; then
            rm -f "$rpm_output"
            record_skip "rpm" "missing RPM build dependencies"
            return 0
        fi

        cat "$rpm_output"
        rm -f "$rpm_output"
        record_failure "rpm" "rpmbuild failed"
    fi
}

build_appimage() {
    for tool in linuxdeploy appimagetool; do
        if ! command_exists "$tool"; then
            record_skip "appimage" "missing required tool: $tool"
            return 0
        fi
    done

    log "BUILD appimage"
    if (cd "$ROOT_DIR" && JAME_PROMPT_REUSE_RELEASE_BUILD=1 bash packaging/appimage/build-appimage.sh); then
        log "DONE appimage"
    else
        record_failure "appimage" "packaging/appimage/build-appimage.sh failed"
    fi
}

main() {
    if [ -z "$VERSION" ]; then
        record_failure "metadata" "unable to read version from Cargo.toml"
        exit 1
    fi

    clean_target
    build_release
    build_debian
    build_arch
    build_rpm
    build_appimage

    if [ "${#SKIPPED_TARGETS[@]}" -gt 0 ]; then
        log "SKIPPED: ${SKIPPED_TARGETS[*]}"
    fi

    if [ "${#FAILED_TARGETS[@]}" -gt 0 ]; then
        log "FAILED: ${FAILED_TARGETS[*]}"
        exit 1
    fi

    log "ALL BUILDS COMPLETED: ${APP_NAME} ${VERSION}"
}

main "$@"
