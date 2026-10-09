# CI-First Build and Native GNOME Testing Design

## Status

Design prepared after repository audit and explicit selection of the GitHub-hosted
runner bootstrap for native GNOME evidence. This document is not an implementation
authorization by itself; it records the agreed architectural direction.

## Problem

The repository already has remote Debian, Arch, and RPM packaging jobs, but the
native GNOME/Wayland evidence was executed manually on a developer workstation.
The root `build.sh` was also a broad local orchestrator that silently skipped
targets when host tools were absent. That made local success ambiguous and
encouraged workstation-specific execution instead of a reproducible CI contract.

The project must use established build and test tools, keep CI as the authoritative
merge boundary, and prevent invented all-in-one installer or repair scripts from
becoming part of the architecture.

## Goals

- Make remote CI authoritative for native GNOME/Wayland, AT-SPI, packaging, and
  install/purge evidence.
- Run native GNOME tests in a GitHub-hosted job with a fail-closed headless Mutter
  Wayland session rather than relying on a developer workstation.
- Preserve Cargo, `uv`/pytest, Debian tooling, pinned container images, and existing
  package entry points as the canonical tools.
- Remove ambiguity from local build orchestration and prevent skipped targets from
  being reported as a successful complete build.
- Retain only transparent, narrow wrappers that call canonical tools and are
  exercised by CI or a documented developer workflow.
- Keep the change modular, deterministic, auditable, and free of hidden fallback
  behavior.

## Non-Goals

- Rewriting the Rust application, desktop test protocol, or packaging formats.
- Adding a custom test runner, repair-all script, or replacement for Cargo, pytest,
  `dpkg-deb`, `lintian`, or the distribution package managers.
- Treating Xvfb, X11 hotkey smoke tests, screenshots, or absence of log errors as
  proof of native GNOME/Wayland behavior.
- Running package installation, purge, or native session setup on the developer's
  workstation.
- Installing tools or changing system state locally to force the gates to pass.

## Current Canonical Boundaries

| Responsibility | Canonical owner |
| --- | --- |
| Rust build and tests | Cargo with `Cargo.lock`, rustfmt, Clippy, and Cargo test |
| Python desktop harness | `pyproject.toml`, `uv.lock`, and pytest |
| Debian build | `packaging/linux/build-deb.sh` and `dpkg-deb` |
| Debian package policy | `scripts/validate_deb_package.sh` and `lintian` |
| Debian lifecycle | `scripts/validate_deb_install_lifecycle.sh` inside the pinned Debian container |
| Arch build and lifecycle | `scripts/build_arch_package.sh` and existing Arch validators inside the pinned container |
| RPM build and validation | `scripts/build_rpm_fedora.sh` and existing RPM validator inside the pinned Fedora container |
| AppImage build and X11 smoke | `packaging/appimage/build-appimage.sh` and the existing smoke script |
| CI/release parity | `.github/workflows/ci.yml`, `.github/workflows/release.yml`, and `scripts/validate_ci_release_parity.py` |

No `crear_instalador_debian.sh` exists in the repository. No replacement with that
name or equivalent behavior is to be introduced.

## CI Architecture

### Deterministic Linux job

The existing deterministic Linux job remains responsible for ordinary Rust tests,
desktop harness contract tests, formatting, metadata validators, AppImage build,
and X11 smoke evidence. It must not imply native GNOME evidence when the native
marker is skipped.

### Native GNOME job

Add a dedicated remote job with an explicit responsibility and a separate timeout.
The job will:

1. Use the pinned repository Rust and Python toolchain contracts.
2. Install the native GTK, GObject, AT-SPI, D-Bus, GNOME/Mutter, and Wayland
   runtime dependencies through the runner's package manager.
3. Start a private D-Bus session and a headless Mutter Wayland session using
   documented upstream behavior.
4. Verify `WAYLAND_DISPLAY`, GNOME session identity, AT-SPI availability, and the
   required D-Bus tools before running tests.
5. Build the feature-enabled binary in a dedicated target directory.
6. Run `pytest -m native_gnome` serially with the explicit native environment
   variables.
7. Fail if the session cannot be created or any required native prerequisite is
   missing; no skip or local fallback is allowed.
8. Collect only bounded diagnostic output and test evidence, excluding prompt,
   clipboard, token, and raw handle contents.

The upstream PyGObject testing guide documents Mutter's headless mode for running
GTK applications on a Wayland display without physical display hardware:
https://gnome.pages.gitlab.gnome.org/pygobject/guide/testing.html

The implementation must verify the exact runner package names and session command
in CI before treating the job as a required gate. A failed bootstrap is a failed
job, not a reason to weaken the native assertions.

### Packaging jobs

The existing Debian, Arch, and RPM jobs remain remote and containerized. Their
digest-pinned images, package validators, lifecycle checks, and parity contract are
the authoritative packaging path. The developer workstation is not a supported
place to run the Debian install/purge lifecycle.

### Release jobs

Release jobs continue to use the same canonical builders and validators. Any
change to a shared package step must update the existing CI/release parity contract
and its self-tests rather than duplicating a new builder.

## Build Script Policy

Every retained repository script must satisfy all of these conditions:

- It has one narrow responsibility and calls a standard underlying tool.
- It has strict failure behavior and does not convert missing required tools into a
  successful complete build.
- Its inputs, outputs, environment variables, and side effects are explicit.
- It is exercised by CI, a focused self-test, or a documented supported workflow.
- It does not silently install dependencies, repair arbitrary state, mutate
  production data, or bypass the canonical toolchain.
- Its existence is documented at its owning boundary.

The root `build.sh` is not a CI authority. Its target-by-target skip-success
behavior is removed while its existing tested developer interface remains a strict
convenience wrapper. README and packaging documentation refer to the explicit
canonical entry points and remote CI jobs instead of presenting a workstation
aggregate as complete release proof.

Generated packaging assets must not create unexplained source-tree state. Any
generated icon or staging output must be confined to a build output directory or be
clearly documented as generated and cleaned deterministically.

## Failure and Evidence Contract

- Missing native-session dependencies fail the native job.
- Missing `lintian`, package-manager tools, or container runtime fail the relevant
  CI job; they are not represented as a clean scan or successful package gate.
- The local developer gate may run fast checks, but it must clearly identify remote
  evidence that it cannot establish.
- Evidence records the commit, dirty/untracked inventory, tool versions, exact
  command, feature set, artifact hashes, exit status, and known limitations.
- No test is weakened, skipped by default, or changed to match an existing
  implementation defect.

## Acceptance Criteria

- A required GitHub-hosted native GNOME job establishes and verifies a headless
  Mutter Wayland session, AT-SPI, and the application process identity before
  running the native suite.
- The native job runs the three native tests with the explicit native environment
  and fails closed when the session cannot be established.
- CI and release packaging continue to invoke the existing canonical builders and
  validators through pinned environments.
- No local command is required to run a container lifecycle, install a package, or
  create a native GNOME session.
- The root aggregate build path no longer reports skipped package targets as a
  complete successful build and is not documented as production evidence.
- No `crear_instalador_debian.sh`, repair-all script, or opaque replacement is
  added.
- Static workflow/parity/self-tests and the relevant CI validation pass.
- The diff contains no unrelated cleanup or changes to product behavior.

## Risks and Controls

| Risk | Control |
| --- | --- |
| GitHub runner package drift | Pin action versions, record package/session versions, and fail closed on contract mismatch. |
| Headless Mutter session differs from a physical GNOME desktop | Label evidence precisely; retain a separate manual installed-session gate for claims requiring physical desktop behavior. |
| CI and release workflow drift | Extend the existing parity validator rather than introducing parallel release logic. |
| Local aggregate command hides missing targets | Remove skip-success semantics from the supported path and make explicit entry points authoritative. |
| A new wrapper becomes another magic script | Apply the script policy above and require CI/self-test coverage before admission. |

## Decisions

- The root `build.sh` is retained as an existing tested developer convenience
  wrapper and now fails when a target tool or build dependency is unavailable. It
  remains outside the authoritative CI package evidence path.
- Exact package names and the final headless Mutter invocation must be validated
  on the GitHub-hosted runner before the workflow change is considered complete.
- Existing dependency advisories remain a separate remediation decision and are not
  silently upgraded as part of this architecture change.
