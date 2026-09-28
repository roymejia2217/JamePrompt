# JamePrompt

![JamePrompt banner](docs/banner.webp)

[![Rust](https://img.shields.io/badge/Rust-2021-b7410e?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![SQLite](https://img.shields.io/badge/SQLite-rusqlite-044a64?style=flat&logo=sqlite&logoColor=white)](https://www.sqlite.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Lightweight local prompt manager with SQLite storage, global hotkeys, clipboard
integration, autostart, and desktop tray support on Linux and Windows.

JamePrompt keeps prompts local, exposes fast desktop workflows for reusable
text, and supports native packaging for the Linux and Windows distributions
maintained by this repository.

## Table of Contents

- [Install](#install)
- [Usage](#usage)
- [Screenshots](#screenshots)
- [Features](#features)
- [Configuration](#configuration)
- [Architecture](#architecture)
- [Development](#development)
- [Distribution](#distribution)
- [Releases](#releases)
- [Contributing](#contributing)
- [License](#license)

## Install

JamePrompt requires Rust 1.88 or newer. Iced is pinned to 0.14.0, so source
builds should use the locked dependency graph.

Clone and build the application:

```sh
git clone https://github.com/roymejia2217/JamePrompt.git
cd JamePrompt
cargo build --release --locked
./target/release/jame-prompt
```

### Linux source-build requirements

On Debian or Ubuntu, install the native dependencies used by the GUI, tray,
hotkey, and paste-injection integrations:

```sh
sudo apt-get install \
  pkg-config \
  libgtk-3-dev \
  libx11-dev \
  libxtst-dev \
  libxkbcommon-dev \
  libxkbcommon-x11-dev \
  libayatana-appindicator3-dev \
  libxdo-dev \
  python3-pil
```

On GNOME Wayland, tray support uses destroy/recreate behavior when hidden state
is unavailable. Global hotkeys use the XDG GlobalShortcuts portal, and
auto-paste can require RemoteDesktop keyboard permission. Linux X11 also
requires the xkbcommon X11 runtime: `libxkbcommon-x11-0` on Debian/Ubuntu or
`libxkbcommon-x11` on Arch and Fedora-family systems.

Prebuilt artifacts are published through
[GitHub Releases](https://github.com/roymejia2217/JamePrompt/releases).

## Usage

Launch the desktop application with:

```sh
./target/release/jame-prompt [--start-minimized]
```

Typical workflow:

1. Create a prompt with a unique name, content, and optional hotkey.
2. Search, filter, sort, favorite, edit, or delete prompts from the main window.
3. Select a prompt to copy its content to the clipboard.
4. Close the window to keep JamePrompt running in the system tray.
5. Restore the window from the tray icon or tray menu.
6. Quit from the tray menu when background operation should stop.

`--start-minimized` launches the application hidden. Prompts are stored in
`prompts.db`, while persistent settings are stored in `settings.json` inside
the application data directory. Existing data from the older `prompt-manager`
directory is migrated automatically when present.

### Prompt backup

Use **Export prompts** to write a schema-versioned JSON backup and **Import
prompts** to review and restore a backup with merge, replace, and duplicate
handling.

Prompt backups contain prompt records only; settings remain in `settings.json` and are not included in the JSON backup.

## Screenshots

| Screenshot | Description |
| --- | --- |
| <img src="docs/screenshots/main_window.webp" alt="Main window" width="220"> | Main window with prompt search and actions. |
| <img src="docs/screenshots/main_window_min.webp" alt="Main window minimized" width="220"> | Minimized state while the app remains active in the tray. |
| <img src="docs/screenshots/prompt_editor.webp" alt="Prompt editor" width="220"> | Editor for creating and updating prompt content. |
| <img src="docs/screenshots/settings_window.webp" alt="Settings window" width="220"> | Theme, hotkey, and autostart settings. |
| <img src="docs/screenshots/favorites_filter.webp" alt="Favorites filter" width="220"> | Favorites-filtered prompt list. |
| <img src="docs/screenshots/system_tray.webp" alt="System tray" width="220"> | Tray controls for restoring or quitting the app. |
| <img src="docs/screenshots/about_window.webp" alt="About window" width="220"> | Application identity and version information. |

## Features

| Feature | Description |
| --- | --- |
| **Prompt storage** | Stores prompts locally in SQLite through `rusqlite`. |
| **Search and filtering** | Filters prompts by name or content and supports favorites-based views. |
| **Prompt management** | Creates, edits, deletes, and favorites prompts from the main window. |
| **Clipboard workflow** | Copies prompt content to the clipboard for reuse in other applications. |
| **Global hotkeys** | Registers optional per-prompt shortcuts and triggers actions from anywhere. |
| **System tray** | Keeps the application available in the background on Linux and Windows. |
| **Theme settings** | Persists Light and Dark theme selection in `settings.json`. |
| **Autostart** | Synchronizes desktop autostart from the settings screen. |
| **Data migration** | Migrates data from the previous `prompt-manager` data directory. |
| **Prompt backup** | Exports and imports schema-versioned JSON prompt backups. |

## Configuration

The `settings.json` file stores persistent application preferences.

| Setting or variable | Purpose |
| --- | --- |
| `hotkeys_enabled` | Enables per-prompt global hotkeys. Defaults to `true`. |
| `autostart_enabled` | Synchronizes desktop autostart. Defaults to `false`. |
| `theme` | Selects `Dark` or `Light`. Defaults to `Dark`. |
| `JAME_PROMPT_PERF` | Enables performance sampling and report generation. |
| `JAME_PROMPT_PERF_REPORT_PATH` | Selects the performance report path. |
| `JAME_PROMPT_PERF_SLOW_MS` | Sets the slow-operation threshold. Defaults to `25`. |
| `JAME_PROMPT_UI_SMOKE_DURATION_MS` | Sets the UI smoke soak duration. Defaults to `15000`. |

Example:

```json
{
  "hotkeys_enabled": true,
  "autostart_enabled": false,
  "theme": "Dark"
}
```

## Architecture

JamePrompt separates application orchestration, domain policy, platform
integration, persistence, and UI concerns.

- `src/application/` contains application-level orchestration and stores.
- `src/domain/` contains domain notification types and policies.
- `src/platform.rs` isolates platform-specific behavior.
- `src/hotkeys/` and `src/hotkeys.rs` implement hotkey adapters and coordination.
- `src/prompt_backup.rs` owns the prompt backup serialization contract.
- `src/prompt_repository.rs` and `src/prompt_service.rs` own prompt persistence
  and service behavior.
- `src/window_lifecycle.rs` coordinates desktop window lifecycle behavior.
- `src/tray.rs`, `src/autostart.rs`, and `src/ui.rs` contain desktop
  integration and presentation behavior.

The repository also contains `packaging/` for platform packages,
`scripts/` for governed build and verification tooling, and `tests/` for
product, packaging, governance, and release contracts.

## Development

Install the pinned repository tooling:

```sh
npm ci
```

Run focused metadata/governance harnesses or the complete local change gate:

```sh
npm run test:commitlint
npm run test:pr-governance
npm run test:release-metadata
npm run test:release-gate
npm run verify:change
```

Core Rust checks are:

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
```

The packaging orchestrator is available through:

```sh
./build.sh
```

Required GitHub checks remain the authoritative merge boundary. See
[CONTRIBUTING.md](CONTRIBUTING.md) for the exact commit, pull-request, release,
and merge contracts.

## Distribution

### Linux

Supported Linux package targets are:

- Debian
- Arch Linux
- Fedora/RHEL RPM
- AppImage

Flatpak is not supported. Native packages preserve the application's global
hotkey, paste simulation, tray, autostart, and desktop integration behavior.

### Windows

JamePrompt publishes:

- an MSI installer for standard installation under Program Files; and
- a portable ZIP that can be extracted and run without installation.

The raw application executable is not published as the installer. The MSI and
portable ZIP are the supported Windows user-facing artifacts.

## Releases

Release metadata is governed by Semantic Versioning and the checked-in
`CHANGELOG.md`. Release tags are created only after the prepared version and
release metadata have passed the repository gates on protected `main`.

Published artifacts are available from
[GitHub Releases](https://github.com/roymejia2217/JamePrompt/releases).

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before proposing changes. JamePrompt
uses Conventional Commits, a machine-validated pull-request schema, required CI
checks, and explicit reviewed rebase merges.

## License

MIT © 2026 Roy Mejia. See [LICENSE](LICENSE).
