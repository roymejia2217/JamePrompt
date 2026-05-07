<p align="center">
  <img src="docs/banner.webp" alt="JamePrompt banner">
</p>

<h1 align="center">JamePrompt</h1>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-b7410e?logo=rust&logoColor=white">
  <img alt="Iced" src="https://img.shields.io/badge/Iced-0.13.1-4b7bec">
  <img alt="SQLite" src="https://img.shields.io/badge/SQLite-rusqlite-044a64?logo=sqlite&logoColor=white">
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/License-MIT-blue.svg"></a>
</p>

<p align="center">A lightweight local prompt manager with SQLite storage, global hotkeys, clipboard integration, and Linux system tray support.</p>

---

## Quick Start

```bash
git clone https://github.com/roymejia2217/JamePrompt
cd JamePrompt
cargo run
```

---

## Features

| **Feature** | **Description** |
|---|---|
| Local prompt database | Stores prompts in a local SQLite database through `rusqlite`. |
| Search | Filters prompts by name or content with escaped SQLite `LIKE` queries. |
| Prompt management | Creates, edits, deletes, selects, and copies prompt content. |
| Global hotkeys | Registers optional per-prompt shortcuts with `global-hotkey`. |
| Clipboard workflow | Copies prompt content to the clipboard and can trigger paste simulation. |
| System tray | Hides the window to tray, restores it from tray, and provides a Quit action. |
| Theme settings | Supports Light and Dark themes persisted in `settings.json`. |
| Auto-start | Can launch automatically on login through a settings toggle and desktop entry sync. |
| Data migration | Migrates existing data from the previous project data directory when available. |

---

## Prerequisites

| **Dependency** | **Purpose** | **Installation** |
|---|---|---|
| Rust toolchain | Builds and runs the application from source. | `rustup` or distribution package manager |
| `pkg-config` | Detects native libraries required by Rust crates. | `sudo apt install pkg-config` |
| X11 development libraries | Required by global hotkeys and input simulation on Linux. | `sudo apt install libx11-dev libxtst-dev libxkbcommon-dev` |
| GTK 3 development libraries | Required for Linux GUI and tray integration during development. | `sudo apt install libgtk-3-dev` |
| AppIndicator or Ayatana AppIndicator | Provides Linux system tray indicator support. | `sudo apt install libayatana-appindicator3-dev` |
| `libxdo` development library | Supports simulated paste actions through `rdev`/X11 integration. | `sudo apt install libxdo-dev` |
| Python Pillow | Generates hicolor launcher icon sizes from `app_icon.png` during Debian packaging. | `sudo apt install python3-pil` |
| Debian packaging tools | Builds and validates the `.deb` package. | `sudo apt install dpkg-dev desktop-file-utils lintian` |
| Arch packaging tools | Builds the Arch package from the local source checkout. | `sudo pacman -S base-devel rust cargo desktop-file-utils` |
| RPM packaging tools | Builds Fedora and RHEL-compatible RPM packages. | `sudo dnf install rpm-build rust cargo desktop-file-utils gtk3-devel libX11-devel libXtst-devel libxkbcommon-devel` |
| AppImage tools | Builds portable AppImage artifacts. | Install `linuxdeploy` and `appimagetool` from their upstream releases |

Note: global hotkeys and paste simulation are X11-oriented. Tray visibility also depends on the desktop environment having an active AppIndicator or status notifier applet.

---

## Linux Packaging

JamePrompt supports the following Linux packaging targets:

- Debian
- Arch
- Fedora
- RHEL
- AppImage

Flatpak is not supported.

The native package builds cover the app's Linux integration features, including global hotkeys, paste simulation, system tray support, and autostart.

---

## Installation

```bash
# 1. Clone the project
git clone https://github.com/roymejia2217/JamePrompt
cd JamePrompt

# 2. Install Linux build dependencies
sudo apt install pkg-config libx11-dev libxtst-dev libxkbcommon-dev libgtk-3-dev libayatana-appindicator3-dev libxdo-dev python3-pil

# 3. Build the optimized binary
cargo build --release

# 4. Run the release binary
./target/release/jame-prompt

# 5. Orchestrate the full build pipeline
./build.sh
```

---

## Usage

```bash
jame-prompt
```

1. Create a prompt with the plus button.
2. Enter a name, content, and optional hotkey.
3. Select a prompt to copy its content to the clipboard.
4. Enable global hotkeys in settings when shortcut triggering is needed.
5. Optionally enable auto-start in settings so the app opens on login.
6. Close the window to keep the application running in the system tray.
7. Restore the window from the tray icon or use the tray menu.
8. Quit explicitly from the tray menu when the background process should stop.

Prompts are persistent local records with a unique name, content, optional hotkey, and per-prompt hotkey enabled state. They are stored in `prompts.db` inside the application data directory. User settings are stored separately in `settings.json`, including hotkeys, theme, and auto-start.

---

## Project Structure

```text
assets/
  icons/
    app_icon.png
    tray_icon.png
    hicolor/
docs/
  banner.webp
  screenshots/
    about_window.webp
    favorites_filter.webp
    main_window.webp
    main_window_min.webp
    prompt_editor.webp
    settings_window.webp
    system_tray.webp
fonts/
  icons.toml
  lucide.ttf
packaging/
  linux/
    build-deb.sh
    changelog
    copyright
    jame-prompt.1
    jame-prompt.desktop
    scripts/
  arch/
    PKGBUILD
  rpm/
    jame-prompt.spec
  appimage/
    build-appimage.sh
src/
  autostart.rs
  config.rs
  db.rs
  hotkeys.rs
  icon.rs
  launch.rs
  main.rs
  migrations.rs
  models.rs
  perf.rs
  perf_smoke.rs
  prompt_repository.rs
  prompt_service.rs
  settings_service.rs
  tray.rs
  ui.rs
tests/
  build_orchestrator.rs
  packaging_metadata.rs
  perf_smoke.rs
  ui_smoke.rs
build.rs
build.sh
Cargo.toml
Cargo.lock
LICENSE
README.md
```

---

## Credits

| **Project** | **Description** | **License** |
|---|---|---|
| Rust | Systems programming language used for the application. | MIT or Apache-2.0 |
| Iced | GUI framework used for the desktop interface. | MIT |
| SQLite | Embedded database engine used through `rusqlite`. | Public Domain |
| rusqlite | Rust SQLite bindings used for local prompt storage. | MIT |
| tray-icon | System tray integration used for background operation. | Apache-2.0 or MIT |
| iced_lucide | Lucide icon integration used by the Iced UI. | MIT |
| global-hotkey | Global keyboard shortcut registration. | Apache-2.0 or MIT |
| rdev | Keyboard input simulation used for paste workflow support. | MIT |
| serde | Serialization framework used for settings persistence. | MIT or Apache-2.0 |
| tokio | Async runtime feature used by Iced. | MIT |

---

## Screenshots

<p align="center">
  <img src="docs/screenshots/main_window.webp" alt="Main window">
</p>

<p align="center">
  <img src="docs/screenshots/main_window_min.webp" alt="Main window minimized">
</p>

<p align="center">
  <img src="docs/screenshots/prompt_editor.webp" alt="Prompt editor">
</p>

<p align="center">
  <img src="docs/screenshots/settings_window.webp" alt="Settings window">
</p>

<p align="center">
  <img src="docs/screenshots/favorites_filter.webp" alt="Favorites filter">
</p>

<p align="center">
  <img src="docs/screenshots/system_tray.webp" alt="System tray">
</p>

<p align="center">
  <img src="docs/screenshots/about_window.webp" alt="About window">
</p>

---

## License

This project is licensed under the [MIT License](LICENSE).
