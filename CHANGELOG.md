# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- None.

### Changed

- None.

### Deprecated

- None.

### Removed

- None.

### Fixed

- None.

### Security

- None.

## [1.2.0-beta.10] - 2026-09-23

### Added

- None.

### Changed

- None.

### Deprecated

- None.

### Removed

- None.

### Fixed

- Fix native global-hotkey auto-paste to verify the prompt is on the clipboard before Ctrl+V injection and report injection failures instead of assuming success.
- Fix packaged Linux X11 hotkeys and auto-paste by declaring or bundling the libxkbcommon-x11 runtime required for X11 keyboard mapping.
- Fix the Windows MSI install lifecycle by separating machine installation state from the per-user Start Menu shortcut state and verifying both are removed on uninstall.

### Security

- None.
