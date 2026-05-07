# Linux Packaging

JamePrompt supports native Linux package formats that preserve the full product
surface: global hotkeys, clipboard copy, paste simulation, system tray behavior,
desktop launch integration, and autostart.

The root `build.sh` orchestrator drives all supported package builds from one
place after cleaning `target/` and building the Rust release binary once.

Flatpak is not supported at this time. The application requires global hotkeys
and paste simulation into other applications, and those behaviors must not be
reduced in any distributed version.

## Supported Targets

| Target | Artifact | Status | Build Entry Point |
|---|---|---|---|
| Debian and Ubuntu | `.deb` | Supported | `packaging/linux/build-deb.sh` |
| Arch Linux | `pkg.tar.zst` through `makepkg` | Supported recipe | `packaging/arch/PKGBUILD` |
| Fedora | `.rpm` | Supported recipe | `packaging/rpm/jame-prompt.spec` |
| RHEL-compatible distributions | `.rpm` | Supported recipe | `packaging/rpm/jame-prompt.spec` |
| AppImage | `.AppImage` | Supported portable build | `packaging/appimage/build-appimage.sh` |

## Runtime Requirements

| Capability | Debian/Ubuntu | Arch | Fedora/RHEL | AppImage |
|---|---|---|---|---|
| GTK 3 UI and tray event loop | `libgtk-3-0` | `gtk3` | `gtk3` | Bundled or provided by the host image build |
| AppIndicator tray support | `libayatana-appindicator3-1` or `libappindicator3-1` | `libappindicator-gtk3` or Ayatana equivalent | `libappindicator-gtk3` or Ayatana equivalent | Bundled when available |
| X11 global hotkeys | `libx11-6`, `libxtst6`, `libxkbcommon0` | `libx11`, `libxtst`, `libxkbcommon` | `libX11`, `libXtst`, `libxkbcommon` | Bundled or host-provided |
| Paste simulation | `libxdo3` | `xdotool` | `libxdo` | Bundled or host-provided |
| Font and image rendering | `libfontconfig1`, `libfreetype6`, `libgdk-pixbuf-2.0-0` | `fontconfig`, `freetype2`, `gdk-pixbuf2` | `fontconfig`, `freetype`, `gdk-pixbuf2` | Bundled or host-provided |
| Desktop integration | `desktop-file-utils`, `hicolor-icon-theme` | `desktop-file-utils`, `hicolor-icon-theme` | `desktop-file-utils`, `hicolor-icon-theme` | AppImage desktop integration tools |

## Validation Checklist

Validate every release artifact before publishing:

1. Build the release binary with `cargo build --release --locked`.
2. Run `cargo test --locked`.
3. Install the package on a clean target system.
4. Launch `jame-prompt` from a terminal.
5. Launch JamePrompt from the desktop menu.
6. Confirm the application icon appears in the menu and window switcher.
7. Create, edit, delete, favorite, search, and copy a prompt.
8. Enable global hotkeys and verify shortcut-triggered clipboard copy.
9. Verify paste simulation into another X11 application.
10. Close the window and confirm JamePrompt stays available from the system tray.
11. Restore the window from the tray icon and tray menu.
12. Enable autostart, log out, log in, and confirm the app starts minimized.
13. Disable autostart and confirm the autostart desktop entry is removed.
14. Uninstall the package and confirm system files are removed.

Wayland sessions may restrict global hotkeys and paste simulation. A package is
release-ready only for environments where all required features pass validation.
