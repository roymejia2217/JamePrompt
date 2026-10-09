# CI-First Build and Native GNOME Testing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move authoritative native GNOME/Wayland and packaging evidence to remote CI while purging ambiguous aggregate build behavior and preventing non-canonical magic scripts.

**Architecture:** Keep Cargo, `uv`/pytest, `dpkg-deb`, `lintian`, and the existing digest-pinned packaging containers as the canonical toolchain. Add one explicit GitHub-hosted native GNOME job using a fail-closed headless Mutter Wayland session, make it part of the CI aggregate gate, and remove the unused root aggregate builder from the supported workflow. Keep package builders narrow and direct.

**Tech Stack:** GitHub Actions, Ubuntu runner packages, Mutter headless Wayland, private D-Bus, Rust/Cargo, rustfmt, Clippy, Python/uv/pytest, Debian `dpkg-deb`/`lintian`, pinned Arch/Fedora/Debian containers, YAML/Python contract validators.

**Spec:** `docs/superpowers/specs/2026-10-09-ci-first-build-and-native-gnome-design.md`

## Global Constraints

- Remote CI is authoritative for native GNOME/Wayland, AT-SPI, packaging, and install/purge evidence.
- Local commands must not be required to run a container lifecycle, install a package, or create a native GNOME session.
- Preserve Cargo, `uv`/pytest, Debian tooling, pinned container images, and existing package entry points as canonical tools.
- Do not add a custom test runner, repair-all script, or replacement for Cargo, pytest, `dpkg-deb`, `lintian`, or distribution package managers.
- Native evidence must not use Xvfb, X11 smoke tests, screenshots, or log absence as proof of GNOME/Wayland behavior.
- Missing required tools, native-session prerequisites, or containers must fail the relevant CI job; no successful skip fallback is allowed.
- Do not install tools or change system state on the developer workstation.
- Do not weaken tests, add production fallbacks, or change product behavior outside the approved CI/build scope.
- Do not commit, push, publish, or deploy without separate explicit authorization.

## Review Focus

- **Native session false positive:** the job must prove a live Mutter Wayland display and required AT-SPI/D-Bus prerequisites before the native marker runs; test: missing display/session prerequisite fails the job instead of skipping.
- **Wrong process evidence:** native observers must continue matching the spawned application PID and exact SNI identity; test: an unrelated JamePrompt process/item cannot satisfy the observer.
- **Local build false success:** absent package tools must not make an aggregate build appear complete; test: the supported local path exposes explicit canonical targets and no skipped target is reported as full success.
- **Source-tree mutation:** Debian icon generation must not leave generated assets in the source tree; test: package build leaves the tracked icon directory unchanged.
- **CI contract drift:** the native job must remain required by the aggregate gate and its command contract must remain visible to repository validators; test: missing native job, required step, or aggregate dependency fails validation.

---

### Task 1: Add the Required Remote Native GNOME CI Job

**Files:**
- Modify: `.github/workflows/ci.yml` in the job definitions and aggregate `test` job.
- Modify: `scripts/validate_ci_release_parity.py` in the CI contract constants, validation, fixtures, and self-tests.

**Interfaces:**
- Consumes: existing `tests/desktop/test_native_gnome.py`, `tests/desktop/preflight.py`, `pyproject.toml`, `uv.lock`, Cargo feature `desktop-integration-tests`.
- Produces: required CI job `test-native-gnome`; aggregate job dependency; a validator contract that rejects missing or incomplete native evidence.

- [ ] **Step 1: Add the native job contract test first**

Extend `fixture_workflows()` and `validate_parity()` so the CI fixture contains a
`test-native-gnome` job and the validator requires these named operations in order:

1. install native GNOME/Wayland/AT-SPI dependencies;
2. create the private runtime directory and D-Bus/Mutter headless session;
3. verify the native session preflight;
4. build `jame-prompt` with `desktop-integration-tests` into `target/desktop-feature`;
5. run `pytest -m native_gnome` with `JAME_PROMPT_NATIVE_GNOME=1`,
   `JAME_PROMPT_NATIVE_ATSPI_SIGNAL=1`, and the exact feature binary path.

Require `test-native-gnome` in `CI_AGGREGATE_NEEDS`. Add a self-test that removes
the job or aggregate dependency and asserts `ParityError` identifies the missing
native CI contract.

- [ ] **Step 2: Run the fixture self-test and verify the real workflow fails before workflow changes**

Run:

```bash
python3 scripts/validate_ci_release_parity.py --self-test
python3 scripts/validate_ci_release_parity.py
```

Expected: the fixture self-test passes, while the real workflow validation fails
because `.github/workflows/ci.yml` does not yet contain the required native job.
Do not weaken the new assertion to make the real workflow pass prematurely.

- [ ] **Step 3: Add `test-native-gnome` to `.github/workflows/ci.yml`**

Use `ubuntu-latest` with a dedicated timeout and the repository's pinned checkout
and `setup-uv` actions. Install only the native packages required by the existing
preflight plus Mutter headless, Wayland, and the D-Bus launcher. Use the documented
Mutter headless mode from the upstream PyGObject testing guide:
`https://gnome.pages.gitlab.gnome.org/pygobject/guide/testing.html`.

Run the session and native test command in one private D-Bus scope so the test
process inherits the verified `WAYLAND_DISPLAY`, session bus, and cleanup path.
The bootstrap must verify the variables and commands consumed by
`missing_native_requirements()` before the test command. A missing package,
session, display, AT-SPI service, or application binary exits nonzero.

- [ ] **Step 4: Add the native job to the aggregate gate**

Add `test-native-gnome` to the `test` job's `needs` list and its explicit success
assertion. Preserve the existing Linux, Windows, Debian, Arch, and RPM gates.

- [ ] **Step 5: Run static workflow and parity validation**

Run:

```bash
python3 scripts/validate_workflow_yaml.py .github/workflows
python3 scripts/validate_ci_release_parity.py --self-test
python3 scripts/validate_ci_release_parity.py
```

Expected: all commands pass, and the validator reports the native job as part of
the required CI aggregate contract.

### Task 2: Make the Root Aggregate Builder Strict

**Files:**
- Modify: `build.sh` target availability and failure reporting.
- Test: `tests/build_orchestrator.rs` existing orchestrator contract.
- Modify: `README.md` packaging/build section.
- Modify: `packaging/linux/README.md` root orchestrator reference.
- Modify: `CONTRIBUTING.md` local verification wording if it references the removed aggregate command.

**Interfaces:**
- Consumes: explicit package entry points already documented in
  `packaging/linux/README.md` and the remote workflows.
- Produces: an existing convenience command that never reports unavailable package
  targets as a successful complete build; CI remains the authoritative package
  environment.

- [ ] **Step 1: Add the strict availability regression first**

Change the existing `build_orchestrator_reports_skipped_targets_and_continues`
case in `tests/build_orchestrator.rs` to assert that a missing target tool makes
the process fail and reports `FAIL rpm`. Run the focused test and observe the
current `SKIP rpm` behavior fail before changing `build.sh`.

- [ ] **Step 2: Make `build.sh` fail closed for unavailable targets**

Replace target-level `SKIP` recording with failure recording for missing tools or
missing RPM build dependencies. Preserve the existing canonical per-target calls,
continue running all targets to report the complete failure set, and exit nonzero
when any target is unavailable or fails. Do not delete `packaging/linux/build-deb.sh`,
`packaging/appimage/build-appimage.sh`, `scripts/build_arch_package.sh`, or
`scripts/build_rpm_fedora.sh`; those remain the narrow builders used by CI.

- [ ] **Step 3: Verify strict builder behavior and documentation boundaries**

Run:

```bash
cargo test --locked --test build_orchestrator
git grep -n 'build.sh' -- ':!docs/superpowers/*'
```

Expected: both orchestrator tests pass; any remaining `build.sh` references are
limited to the tested interface and implementation, while supported documentation
continues to state that remote CI is the complete package evidence boundary.

### Task 3: Make Debian Packaging Deterministic and Source-Tree Clean

**Files:**
- Modify: `packaging/linux/build-deb.sh` icon staging and shell strictness.
- Modify: `.github/workflows/ci.yml` Debian verification steps only if needed to assert clean staging.
- Test: existing `scripts/validate_deb_package.sh` plus a new focused shell-level assertion in the Debian CI job.

**Interfaces:**
- Consumes: `assets/icons/app_icon.png`, the feature-off release binary, and the
  existing Debian metadata files.
- Produces: the same `.deb` payload and metadata without writing generated icon
  sizes beneath `assets/icons/hicolor`.

- [ ] **Step 1: Add the CI regression assertion for source-tree cleanliness**

In the Debian CI job, record the tracked icon directory state before
`packaging/linux/build-deb.sh`, build the package, and assert that the tracked
source icon paths are unchanged afterward. Keep package payload validation through
`scripts/validate_deb_package.sh` and `lintian`.

- [ ] **Step 2: Refactor icon generation into the package staging area**

In `packaging/linux/build-deb.sh`, create a private temporary icon directory with a
cleanup trap, resize the source icon into that directory, and install those files
directly into the Debian staging root. Preserve the existing icon sizes, package
paths, permissions, and final artifact name. Use strict unset-variable handling and
retain the current locked release build behavior.

- [ ] **Step 3: Run the focused Debian package checks in an approved remote environment**

Run in CI, not on the workstation:

```bash
packaging/linux/build-deb.sh
scripts/validate_deb_package.sh target/debian/*.deb
```

Expected: package metadata, payload, desktop entry, `lintian --fail-on error`, and
source-tree cleanliness all pass. Do not run the install/purge lifecycle locally.

### Task 4: Separate Fast Local Checks from Remote Acceptance Evidence

**Files:**
- Modify: `scripts/verify_change_gate.sh` to keep only fast deterministic local contracts and explicitly label remote-only evidence.
- Modify: `package.json` script description only if needed to reflect the fast local gate.
- Modify: `CONTRIBUTING.md` local setup and delivery gate wording.
- Modify: `README.md` desktop testing and packaging sections.

**Interfaces:**
- Consumes: existing metadata self-tests, workflow/parity validators, formatter,
  lockfile checks, and focused deterministic harness checks.
- Produces: a local gate that does not launch native sessions, containers,
  package lifecycle operations, or the full feature-enabled acceptance matrix;
  required CI remains the authoritative full gate.

- [ ] **Step 1: Define the fast local gate boundary in the existing gate self-test**

Add a `--self-test` mode to `scripts/verify_change_gate.sh` before changing its
normal execution path. The self-test must inspect the gate's own command list and
fail if it contains `docker`, `apt-get`, `dpkg --root`, native GNOME environment
variables, or Debian/Arch lifecycle validators. Preserve static metadata,
YAML/parity, formatting, lockfile, and focused deterministic checks in the normal
path. Run `bash scripts/verify_change_gate.sh --self-test` before narrowing the
gate and expect the new assertion to fail while forbidden commands remain.

- [ ] **Step 2: Narrow `scripts/verify_change_gate.sh` to the approved local boundary**

Remove full package/lifecycle/native-session responsibilities from the local gate.
Do not remove the corresponding remote workflow steps. Make any remote-only
limitation explicit in output so a green local run cannot be mistaken for full
release evidence.

Run: `bash scripts/verify_change_gate.sh --self-test`

Expected: PASS, with no forbidden remote-only command or native-session variable
remaining in the local gate.

- [ ] **Step 3: Update contributor and README commands**

Document the fast local gate as feedback only and identify GitHub required checks as
the authority for native GNOME, packaging, and lifecycle evidence. Keep the native
test command documented as a CI job command, not as a required workstation step.

- [ ] **Step 4: Run the local fast gate**

Run: `npm run verify:change`

Expected: fast deterministic checks pass without starting a native GNOME session,
installing packages, or invoking container lifecycle scripts. Full acceptance is
verified by remote CI in Task 1 and the existing packaging jobs.

### Task 5: Final Repository Contract and Documentation Validation

**Files:**
- Modify: `.github/workflows/ci.yml` only for final ordering, naming, or evidence output corrections.
- Modify: `scripts/validate_ci_release_parity.py` only for contract corrections found during validation.
- Modify: `README.md`, `CONTRIBUTING.md`, and `packaging/linux/README.md` only for verified documentation drift.

**Interfaces:**
- Consumes: all outputs from Tasks 1-4.
- Produces: a reviewed diff whose supported build/test commands map one-to-one to
  canonical tools and remote evidence gates.

- [ ] **Step 1: Run repository-local static validation**

Run:

```bash
git diff --check
python3 scripts/validate_workflow_yaml.py --self-test
python3 scripts/validate_workflow_yaml.py .github/workflows
python3 scripts/validate_ci_release_parity.py --self-test
python3 scripts/validate_ci_release_parity.py
```

Expected: all checks pass with no workflow/parity drift.

- [ ] **Step 2: Review the final supported-script inventory**

For every modified or retained build/validation script, verify its owning
documentation, standard underlying tool, explicit inputs/outputs, strict failure
behavior, and CI/self-test coverage. Confirm no new aggregate runner or installer
has been introduced.

- [ ] **Step 3: Review the diff and working-tree attribution**

Run `git status --short` and `git diff --stat`, then inspect every changed file.
Do not stage, revert, or modify unrelated pre-existing changes. Record that no
commit, push, release, or deployment was performed.

- [ ] **Step 4: Remote validation gate**

Open or update the authorized CI run only through the repository's normal delivery
path after separate delivery authorization. Require the native GNOME job,
deterministic Linux job, Windows job, Debian, Arch, RPM, and aggregate gate to pass.
If the GitHub-hosted Mutter bootstrap fails, report the concrete runner limitation
and stop; do not replace it with a skip or local workaround.
