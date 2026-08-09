#!/usr/bin/env bash
set -euo pipefail

APP_ID="jame-prompt"
DESKTOP_APP_ID="io.github.roymejia2217.JamePrompt"
APP_NAME="JamePrompt"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
ARCH="${ARCH:-x86_64}"
APPDIR="target/appimage/${APP_NAME}.AppDir"
DIST_DIR="dist"

require_command() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "Missing required command: $1" >&2
        exit 1
    }
}

require_file() {
    [ -f "$1" ] || {
        echo "Missing required file: $1" >&2
        exit 1
    }
}

if [ -z "$VERSION" ]; then
    echo "Unable to read package version from Cargo.toml" >&2
    exit 1
fi

require_command cargo
require_command install
require_command linuxdeploy
require_command appimagetool
require_file "assets/icons/app_icon.png"
require_file "packaging/linux/${APP_ID}.desktop"

if [ "${JAME_PROMPT_REUSE_RELEASE_BUILD:-0}" != "1" ]; then
    cargo build --release --locked
fi

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/512x512/apps" "$DIST_DIR"

install -Dm755 "target/release/${APP_ID}" "$APPDIR/usr/bin/${APP_ID}"
install -Dm644 "packaging/linux/${APP_ID}.desktop" "$APPDIR/usr/share/applications/${DESKTOP_APP_ID}.desktop"
install -Dm644 "assets/icons/app_icon.png" "$APPDIR/usr/share/icons/hicolor/512x512/apps/${APP_ID}.png"
install -Dm644 "assets/icons/app_icon.png" "$APPDIR/${APP_ID}.png"
install -Dm644 "packaging/linux/${APP_ID}.desktop" "$APPDIR/${DESKTOP_APP_ID}.desktop"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/usr/bin/env bash
set -euo pipefail

HERE="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH:-}"
exec "${HERE}/usr/bin/jame-prompt" "$@"
APPRUN
chmod 755 "$APPDIR/AppRun"

linuxdeploy --appdir "$APPDIR" --desktop-file "$APPDIR/${DESKTOP_APP_ID}.desktop" --icon-file "$APPDIR/${APP_ID}.png"
ARCH="$ARCH" VERSION="$VERSION" appimagetool "$APPDIR" "${DIST_DIR}/${APP_ID}-${VERSION}-${ARCH}.AppImage"
