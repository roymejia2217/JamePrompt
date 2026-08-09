#!/bin/sh
set -e

APP_ID="jame-prompt"
DESKTOP_APP_ID="io.github.roymejia2217.JamePrompt"
APP_NAME="JamePrompt"
MAINTAINER="Roy Mejia <roymejia2217@gmail.com>"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
ARCH="$(dpkg --print-architecture)"
ROOT="target/debian/${APP_ID}_${VERSION}_${ARCH}"
DEB="target/debian/${APP_ID}_${VERSION}_${ARCH}.deb"

if [ -z "$VERSION" ]; then
    echo "Unable to read package version from Cargo.toml" >&2
    exit 1
fi

python3 - <<'PY'
from pathlib import Path
from PIL import Image

src = Path("assets/icons/app_icon.png")
if not src.exists():
    raise SystemExit("Missing assets/icons/app_icon.png")

img = Image.open(src).convert("RGBA")

for size in [16, 22, 24, 32, 48, 64, 128, 256, 512]:
    target = Path("assets/icons/hicolor") / f"{size}x{size}" / "apps" / "jame-prompt.png"
    target.parent.mkdir(parents=True, exist_ok=True)
    img.resize((size, size), Image.Resampling.LANCZOS).save(target, optimize=True)
PY

if [ "${JAME_PROMPT_REUSE_RELEASE_BUILD:-0}" = "1" ]; then
    echo "Reusing existing release binary..."
else
    cargo build --release --locked
fi

rm -rf "$ROOT"
install -Dm755 "target/release/${APP_ID}" "$ROOT/usr/bin/${APP_ID}"
install -Dm644 "packaging/linux/${APP_ID}.desktop" "$ROOT/usr/share/applications/${DESKTOP_APP_ID}.desktop"
install -Dm755 "packaging/linux/scripts/postinst" "$ROOT/DEBIAN/postinst"
install -Dm755 "packaging/linux/scripts/postrm" "$ROOT/DEBIAN/postrm"
install -Dm644 "packaging/linux/copyright" "$ROOT/usr/share/doc/${APP_ID}/copyright"
install -Dm644 "packaging/linux/changelog" "$ROOT/usr/share/doc/${APP_ID}/changelog"
install -Dm644 "packaging/linux/${APP_ID}.1" "$ROOT/usr/share/man/man1/${APP_ID}.1"
gzip -n -9 "$ROOT/usr/share/doc/${APP_ID}/changelog"
gzip -n -9 "$ROOT/usr/share/man/man1/${APP_ID}.1"

for icon in assets/icons/hicolor/*/apps/${APP_ID}.png; do
    size="$(basename "$(dirname "$(dirname "$icon")")")"
    install -Dm644 "$icon" "$ROOT/usr/share/icons/hicolor/${size}/apps/${APP_ID}.png"
done

installed_size="$(du -ks "$ROOT" | awk '{print $1}')"
cat > "$ROOT/DEBIAN/control" <<EOF
Package: ${APP_ID}
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Maintainer: ${MAINTAINER}
Installed-Size: ${installed_size}
Depends: libc6, libgcc-s1, libgtk-3-0, libxdo3, libayatana-appindicator3-1 | libappindicator3-1, libx11-6, libxtst6, libxkbcommon0, libfontconfig1, libfreetype6, libglib2.0-0, libgdk-pixbuf-2.0-0, hicolor-icon-theme, desktop-file-utils, xdg-desktop-portal
Description: Lightweight local prompt manager
 JamePrompt is a lightweight local prompt manager with hotkeys,
 clipboard integration, and system tray support.
EOF

dpkg-deb --build --root-owner-group "$ROOT" "$DEB"
echo "$DEB"
