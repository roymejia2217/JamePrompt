#!/usr/bin/env python3
"""Fail closed when release-facing CI and Release contracts drift apart."""

from __future__ import annotations

import argparse
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml


class ParityError(ValueError):
    """Raised when CI and Release no longer enforce equivalent release-facing gates."""


@dataclass(frozen=True)
class ParityContract:
    ci_job: str
    release_job: str
    shared_tokens: tuple[str, ...]
    release_order: tuple[str, ...]


PARITY_CONTRACTS: dict[str, ParityContract] = {
    "debian": ParityContract(
        ci_job="test_deb",
        release_job="deb",
        shared_tokens=(
            "packaging/linux/build-deb.sh",
            "scripts/validate_deb_package.sh",
            "debian:trixie-20260713-slim@sha256:020c0d20b9880058cbe785a9db107156c3c75c2ac944a6aa7ab59f2add76a7bd",
            "scripts/validate_deb_install_lifecycle.sh",
        ),
        release_order=(
            "Build Debian package",
            "Validate Debian package artifact",
            "Validate Debian install lifecycle",
            "Upload Debian package",
        ),
    ),
    "arch": ParityContract(
        ci_job="test_arch",
        release_job="arch",
        shared_tokens=(
            "archlinux:base-devel-20260913.0.592969@sha256:70d777aaeb45befc04150df137c4d7c1b5042be442b4c904c38c6f6880bb7844",
            "scripts/build_arch_package.sh",
            "scripts/validate_arch_package.sh",
            "scripts/validate_arch_install_lifecycle.sh",
        ),
        release_order=(
            "Build Arch package in pinned snapshot container",
            "Validate Arch package artifact",
            "Validate Arch install lifecycle",
            "Upload Arch package",
        ),
    ),
    "rpm": ParityContract(
        ci_job="test_rpm",
        release_job="rpm",
        shared_tokens=(
            "fedora:44@sha256:43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80",
            "scripts/build_rpm_fedora.sh",
            "scripts/validate_rpm_package.sh",
        ),
        release_order=(
            "Build RPM package in Fedora container",
            "Validate RPM package artifact",
            "Upload RPM package",
        ),
    ),
    "appimage": ParityContract(
        ci_job="test-linux",
        release_job="appimage",
        shared_tokens=(
            "scripts/install_appimage_tools.sh",
            "packaging/appimage/build-appimage.sh",
            "scripts/smoke/native-hotkey-x11.sh",
            "APPIMAGE_EXTRACT_AND_RUN",
        ),
        release_order=(
            "Install verified AppImage tools",
            "Build AppImage",
            "Run AppImage X11 native hotkey auto-paste smoke",
            "Upload AppImage",
        ),
    ),
    "windows": ParityContract(
        ci_job="test-windows",
        release_job="windows",
        shared_tokens=(
            "cargo build --release --locked",
            "scripts/validate_windows_runtime.ps1",
            "scripts/smoke/native-hotkey-windows.ps1",
            "scripts/build_windows_msi.ps1",
            "scripts/validate_windows_msi.ps1",
        ),
        release_order=(
            "Build Windows binaries",
            "Validate Windows runtime dependencies",
            "Run Windows release native hotkey auto-paste smoke",
            "Create portable archive",
            "Build MSI",
            "Validate MSI install and uninstall",
            "Upload portable archive",
            "Upload MSI",
        ),
    ),
}

CI_AGGREGATE_NEEDS = {
    "test-linux",
    "test-windows",
    "test_deb",
    "test_arch",
    "test_rpm",
}
RELEASE_REQUIRED_NEEDS = {
    "release-gate",
    "deb",
    "arch",
    "rpm",
    "appimage",
    "windows",
}
UNSAFE_RELEASE_REF_CHECKOUT = "ref: ${{ env.RELEASE_REF }}"


def load_workflow(path: Path) -> dict[str, Any]:
    try:
        parsed = yaml.safe_load(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, yaml.YAMLError) as error:
        raise ParityError(f"{path}: unable to read workflow: {error}") from error
    if not isinstance(parsed, dict):
        raise ParityError(f"{path}: workflow root must be a mapping")
    jobs = parsed.get("jobs")
    if not isinstance(jobs, dict):
        raise ParityError(f"{path}: jobs must be a mapping")
    return parsed


def get_job(workflow: dict[str, Any], job_name: str, source: str) -> dict[str, Any]:
    jobs = workflow["jobs"]
    job = jobs.get(job_name)
    if not isinstance(job, dict):
        raise ParityError(f"{source}: missing job '{job_name}'")
    return job


def flatten_strings(value: Any) -> list[str]:
    strings: list[str] = []
    if isinstance(value, str):
        strings.append(value)
    elif isinstance(value, dict):
        for key, item in value.items():
            strings.extend(flatten_strings(key))
            strings.extend(flatten_strings(item))
    elif isinstance(value, list):
        for item in value:
            strings.extend(flatten_strings(item))
    return strings


def job_text(job: dict[str, Any]) -> str:
    return "\n".join(flatten_strings(job))


def step_by_name(job: dict[str, Any], name: str, source: str) -> dict[str, Any]:
    steps = job.get("steps")
    if not isinstance(steps, list):
        raise ParityError(f"{source}: job has no steps list")
    matches = [
        step
        for step in steps
        if isinstance(step, dict) and step.get("name") == name
    ]
    if len(matches) != 1:
        raise ParityError(
            f"{source}: step '{name}' must appear exactly once, found {len(matches)}"
        )
    return matches[0]


def step_names(job: dict[str, Any], source: str) -> list[str]:
    steps = job.get("steps")
    if not isinstance(steps, list):
        raise ParityError(f"{source}: job has no steps list")
    names: list[str] = []
    for step in steps:
        if not isinstance(step, dict):
            raise ParityError(f"{source}: workflow step must be a mapping")
        name = step.get("name")
        if isinstance(name, str):
            names.append(name)
    return names


def require_tokens(text: str, tokens: tuple[str, ...], source: str) -> None:
    for token in tokens:
        if token not in text:
            raise ParityError(f"{source}: missing required parity token: {token}")


def require_order(names: list[str], expected: tuple[str, ...], source: str) -> None:
    positions: list[int] = []
    for name in expected:
        try:
            positions.append(names.index(name))
        except ValueError as error:
            raise ParityError(f"{source}: missing required release step: {name}") from error
    if positions != sorted(positions) or len(set(positions)) != len(positions):
        raise ParityError(
            f"{source}: release-facing steps are not in the required order: {expected}"
        )


def normalized_needs(job: dict[str, Any], source: str) -> set[str]:
    needs = job.get("needs", [])
    if isinstance(needs, str):
        return {needs}
    if isinstance(needs, list) and all(isinstance(item, str) for item in needs):
        return set(needs)
    raise ParityError(f"{source}: needs must be a string or list of strings")


def validate_parity(ci: dict[str, Any], release: dict[str, Any]) -> None:
    release_text = "\n".join(flatten_strings(release))
    if "--apply" in release_text:
        raise ParityError(
            "Release workflow must not mutate tracked version metadata"
        )

    for platform, contract in PARITY_CONTRACTS.items():
        ci_job = get_job(ci, contract.ci_job, f"CI/{platform}")
        release_job = get_job(release, contract.release_job, f"Release/{platform}")
        builder_checkout = step_by_name(
            release_job,
            "Checkout",
            f"Release/{platform}",
        )
        builder_checkout_with = builder_checkout.get("with")
        if not isinstance(builder_checkout_with, dict):
            raise ParityError(
                f"Release/{platform}: builder checkout must use release tag data"
            )
        if builder_checkout_with.get("ref") != "${{ env.RELEASE_REF }}":
            raise ParityError(
                f"Release/{platform}: builder checkout must use release tag data"
            )
        if builder_checkout_with.get("persist-credentials") is not False:
            raise ParityError(
                f"Release/{platform}: builder checkout must disable persisted credentials"
            )
        if platform == "appimage":
            for source, appimage_job in (
                ("CI/appimage", ci_job),
                ("Release/appimage", release_job),
            ):
                tool_step = step_by_name(
                    appimage_job,
                    "Install verified AppImage tools",
                    source,
                )
                tool_step_text = "\n".join(flatten_strings(tool_step))
                if any(
                    token in tool_step_text
                    for token in ("GH_TOKEN", "GITHUB_TOKEN", "github.token")
                ):
                    raise ParityError(
                        f"{source}: AppImage tool installer must not receive GitHub token"
                    )
        require_tokens(
            job_text(ci_job),
            contract.shared_tokens,
            f"CI/{platform}",
        )
        require_tokens(
            job_text(release_job),
            contract.shared_tokens,
            f"Release/{platform}",
        )
        require_tokens(
            job_text(release_job),
            ("scripts/prepare_release_version.py", "--check"),
            f"Release/{platform}",
        )
        require_order(
            step_names(release_job, f"Release/{platform}"),
            contract.release_order,
            f"Release/{platform}",
        )

    release_gate_job = get_job(release, "release-gate", "Release/release-gate")
    release_gate_checkout = step_by_name(
        release_gate_job,
        "Checkout trusted release gate tooling",
        "Release/release-gate",
    )
    release_gate_checkout_with = release_gate_checkout.get("with")
    if not isinstance(release_gate_checkout_with, dict):
        raise ParityError(
            "Release/release-gate: trusted tooling must use workflow identity"
        )
    if release_gate_checkout_with.get("ref") != "${{ github.workflow_sha }}":
        raise ParityError(
            "Release/release-gate: trusted tooling must use workflow identity"
        )
    if release_gate_checkout_with.get("fetch-depth") != 0:
        raise ParityError(
            "Release/release-gate: trusted checkout requires fetch-depth: 0"
        )
    if release_gate_checkout_with.get("persist-credentials") is not False:
        raise ParityError(
            "Release/release-gate: trusted checkout requires persist-credentials: false"
        )

    release_gate_text = job_text(release_gate_job)
    require_tokens(
        release_gate_text,
        (
            "scripts/validate_release_gate.py",
            '--source-ref "$RELEASE_REF"',
        ),
        "Release/release-gate",
    )
    require_order(
        step_names(release_gate_job, "Release/release-gate"),
        (
            "Checkout trusted release gate tooling",
            "Verify trusted release gate tooling identity",
            "Fetch protected main and tags",
            "Validate release provenance and version",
        ),
        "Release/release-gate",
    )

    ci_aggregate = get_job(ci, "test", "CI/test")
    missing_ci_needs = CI_AGGREGATE_NEEDS - normalized_needs(ci_aggregate, "CI/test")
    if missing_ci_needs:
        raise ParityError(
            f"CI/test: aggregate gate is missing required jobs: {sorted(missing_ci_needs)}"
        )

    release_job = get_job(release, "release", "Release/release")
    trusted_checkout = step_by_name(
        release_job,
        "Checkout trusted release tooling",
        "Release/release",
    )
    checkout_with = trusted_checkout.get("with")
    if not isinstance(checkout_with, dict):
        raise ParityError(
            "Release/release: write-capable tooling must use workflow identity"
        )
    if checkout_with.get("ref") != "${{ github.workflow_sha }}":
        raise ParityError(
            "Release/release: write-capable tooling must use workflow identity"
        )
    if checkout_with.get("fetch-depth") != 0:
        raise ParityError(
            "Release/release: trusted checkout requires fetch-depth: 0"
        )
    if checkout_with.get("persist-credentials") is not False:
        raise ParityError(
            "Release/release: trusted checkout requires persist-credentials: false"
        )

    publication_text = job_text(release_job)
    if "${{ env.RELEASE_REF }}" in str(checkout_with.get("ref", "")):
        raise ParityError(
            f"Release/release: write-capable tooling must use workflow identity; "
            f"forbidden={UNSAFE_RELEASE_REF_CHECKOUT}"
        )
    require_tokens(
        publication_text,
        (
            "TAG_CHANGELOG_FILE",
            'git show "${TAG_NAME}:CHANGELOG.md"',
            '--changelog "$TAG_CHANGELOG_FILE"',
        ),
        "Release/release",
    )
    if "--clobber" in publication_text:
        raise ParityError("Release workflow must not clobber published assets")
    if "gh release view" in publication_text or "2>/dev/null" in publication_text:
        raise ParityError(
            "Release workflow must use deterministic release existence probe"
        )
    require_tokens(
        publication_text,
        (
            "scripts/probe_github_release.py",
            "RELEASE_STATE",
            "scripts/validate_existing_release_assets.py",
            "MISSING_RELEASE_FILES",
        ),
        "Release/release",
    )
    for token in (
        "FINAL_RELEASE_STATE",
        "--require-complete",
        "Published release attestation passed.",
    ):
        if token not in publication_text:
            raise ParityError(
                "Release workflow must attest published state after mutation: "
                f"missing {token}"
            )

    missing_release_needs = RELEASE_REQUIRED_NEEDS - normalized_needs(
        release_job, "Release/release"
    )
    if missing_release_needs:
        raise ParityError(
            "Release/release: final publication gate is missing required jobs: "
            f"{sorted(missing_release_needs)}"
        )

    release_step_names = step_names(release_job, "Release/release")
    require_tokens(
        job_text(release_job),
        ("scripts/validate_reusable_release_run.py",),
        "Release/release",
    )
    require_order(
        release_step_names,
        (
            "Checkout trusted release tooling",
            "Verify trusted release tooling identity",
            "Validate reusable release run provenance",
            "Download artifacts from existing run",
            "Create GitHub release",
        ),
        "Release/release",
    )

    release_condition = str(release_job.get("if", ""))
    for job_name in ("deb", "arch", "rpm", "appimage", "windows"):
        token = f"needs.{job_name}.result == 'success'"
        if token not in release_condition:
            raise ParityError(
                f"Release/release: publication condition is missing success gate: {token}"
            )


def fixture_workflows() -> tuple[dict[str, Any], dict[str, Any]]:
    ci_jobs: dict[str, Any] = {}
    release_jobs: dict[str, Any] = {}

    for contract in PARITY_CONTRACTS.values():
        ci_steps = [
            {
                "name": "Parity fixture",
                "run": "\n".join(contract.shared_tokens),
            }
        ]
        if contract.release_job == "appimage":
            ci_steps.append(
                {
                    "name": "Install verified AppImage tools",
                    "run": "scripts/install_appimage_tools.sh",
                }
            )
        ci_jobs[contract.ci_job] = {"steps": ci_steps}
        release_steps = [
            {
                "name": "Checkout",
                "uses": "actions/checkout@pinned",
                "with": {
                    "ref": "${{ env.RELEASE_REF }}",
                    "persist-credentials": False,
                },
            },
            {
                "name": "Validate release source version",
                "run": "python3 scripts/prepare_release_version.py --tag \"$RELEASE_REF\" --check",
            },
            *[
                {
                    "name": name,
                    "run": "\n".join(contract.shared_tokens) if index == 0 else "true",
                }
                for index, name in enumerate(contract.release_order)
            ],
        ]
        release_jobs[contract.release_job] = {"steps": release_steps}

    ci_jobs["test"] = {"needs": sorted(CI_AGGREGATE_NEEDS), "steps": []}
    release_jobs["release-gate"] = {
        "steps": [
            {
                "name": "Checkout trusted release gate tooling",
                "uses": "actions/checkout@pinned",
                "with": {
                    "ref": "${{ github.workflow_sha }}",
                    "fetch-depth": 0,
                    "persist-credentials": False,
                },
            },
            {
                "name": "Verify trusted release gate tooling identity",
                "run": (
                    "EXPECTED_WORKFLOW_SHA=${{ github.workflow_sha }}\n"
                    "echo 'Trusted release gate tooling identity mismatch' >/dev/null"
                ),
            },
            {
                "name": "Fetch protected main and tags",
                "run": "git fetch origin main --tags --force",
            },
            {
                "name": "Validate release provenance and version",
                "run": (
                    'python3 scripts/validate_release_gate.py --tag "$RELEASE_REF" '
                    '--main-ref origin/main --source-ref "$RELEASE_REF"'
                ),
            },
        ]
    }
    release_jobs["release"] = {
        "needs": sorted(RELEASE_REQUIRED_NEEDS),
        "if": " && ".join(
            f"needs.{job_name}.result == 'success'"
            for job_name in ("deb", "arch", "rpm", "appimage", "windows")
        ),
        "steps": [
            {
                "name": "Checkout trusted release tooling",
                "uses": "actions/checkout@pinned",
                "with": {
                    "ref": "${{ github.workflow_sha }}",
                    "fetch-depth": 0,
                    "persist-credentials": False,
                },
            },
            {
                "name": "Verify trusted release tooling identity",
                "run": (
                    "EXPECTED_WORKFLOW_SHA=${{ github.workflow_sha }}\n"
                    "echo 'Trusted release tooling identity mismatch' >/dev/null"
                ),
            },
            {
                "name": "Validate reusable release run provenance",
                "run": "python3 scripts/validate_reusable_release_run.py",
            },
            {"name": "Download artifacts from existing run", "run": "true"},
            {
                "name": "Create GitHub release",
                "run": (
                    "TAG_CHANGELOG_FILE=release-changelog.md\n"
                    "git show \"${TAG_NAME}:CHANGELOG.md\" > \"$TAG_CHANGELOG_FILE\"\n"
                    "python3 scripts/validate_release_metadata.py "
                    "--changelog \"$TAG_CHANGELOG_FILE\"\n"
                    "python3 scripts/probe_github_release.py "
                    "--repository owner/repo --tag v1.2.3 "
                    "--write-status release-state.txt --write-json existing.json\n"
                    "RELEASE_STATE=existing\n"
                    "python3 scripts/validate_existing_release_assets.py "
                    "--release-json existing.json --asset-dir release-artifacts "
                    "--write-missing missing.txt\n"
                    "MISSING_RELEASE_FILES=()\n"
                    "FINAL_RELEASE_STATE=existing\n"
                    "python3 scripts/validate_existing_release_assets.py "
                    "--release-json final.json --asset-dir release-artifacts "
                    "--write-missing final-missing.txt --require-complete\n"
                    "echo 'Published release attestation passed.'"
                ),
            },
        ],
    }
    return {"jobs": ci_jobs}, {"jobs": release_jobs}


def run_self_test() -> None:
    ci, release = fixture_workflows()
    validate_parity(ci, release)

    broken_ci, broken_release = fixture_workflows()
    rpm_steps = broken_release["jobs"]["rpm"]["steps"]
    rpm_validator_step = next(
        step
        for step in rpm_steps
        if "scripts/validate_rpm_package.sh" in str(step.get("run", ""))
    )
    rpm_validator_step["run"] = rpm_validator_step["run"].replace(
        "scripts/validate_rpm_package.sh", "missing-rpm-validator"
    )
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "Release/rpm" not in str(error):
            raise AssertionError("RPM parity failure was not attributed correctly") from error
    else:
        raise AssertionError("missing RPM validator must fail parity validation")

    broken_ci, broken_release = fixture_workflows()
    arch_steps = broken_release["jobs"]["arch"]["steps"]
    arch_steps[2], arch_steps[3] = arch_steps[3], arch_steps[2]
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "Release/arch" not in str(error):
            raise AssertionError("Arch ordering failure was not attributed correctly") from error
    else:
        raise AssertionError("reordered Arch validation steps must fail parity validation")

    broken_ci, broken_release = fixture_workflows()
    deb_version_step = step_by_name(
        broken_release["jobs"]["deb"],
        "Validate release source version",
        "Release/debian",
    )
    deb_version_step["run"] = deb_version_step["run"].replace("--check", "--apply")
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "must not mutate tracked version metadata" not in str(error):
            raise AssertionError("Release mutation failure was not attributed correctly") from error
    else:
        raise AssertionError("Release workflow version mutation must fail parity validation")

    broken_ci, broken_release = fixture_workflows()
    create_step = next(
        step
        for step in broken_release["jobs"]["release"]["steps"]
        if step.get("name") == "Create GitHub release"
    )
    create_step["run"] += "\ngh release upload v1.2.3 artifact --clobber"
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "Release workflow must not clobber published assets" not in str(error):
            raise AssertionError("Release clobber failure was not attributed correctly") from error
    else:
        raise AssertionError("published asset clobber must fail parity validation")

    broken_ci, broken_release = fixture_workflows()
    create_step = next(
        step
        for step in broken_release["jobs"]["release"]["steps"]
        if step.get("name") == "Create GitHub release"
    )
    create_step["run"] += "\ngh release view v1.2.3 2>/dev/null"
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "deterministic release existence probe" not in str(error):
            raise AssertionError("ambiguous existence probe failure was not attributed correctly") from error
    else:
        raise AssertionError("ambiguous release existence probe must fail parity validation")

    broken_ci, broken_release = fixture_workflows()
    create_step = next(
        step
        for step in broken_release["jobs"]["release"]["steps"]
        if step.get("name") == "Create GitHub release"
    )
    create_step["run"] = create_step["run"].replace(
        "FINAL_RELEASE_STATE=existing",
        "FINAL_STATE_REMOVED=existing",
    )
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "must attest published state after mutation" not in str(error):
            raise AssertionError("publication attestation failure was not attributed correctly") from error
    else:
        raise AssertionError("missing publication attestation must fail parity validation")


    broken_ci, broken_release = fixture_workflows()
    appimage_tool_step = step_by_name(
        broken_release["jobs"]["appimage"],
        "Install verified AppImage tools",
        "Release/appimage",
    )
    appimage_tool_step["env"] = {"GH_TOKEN": "${{ github.token }}"}
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "AppImage tool installer must not receive GitHub token" not in str(error):
            raise AssertionError(
                "AppImage token exposure failure was not attributed correctly"
            ) from error
    else:
        raise AssertionError(
            "AppImage tool installer with GitHub token must fail parity validation"
        )

    broken_ci, broken_release = fixture_workflows()
    appimage_checkout = step_by_name(
        broken_release["jobs"]["appimage"],
        "Checkout",
        "Release/appimage",
    )
    appimage_checkout["with"]["persist-credentials"] = True
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "builder checkout must disable persisted credentials" not in str(error):
            raise AssertionError(
                "builder credential failure was not attributed correctly"
            ) from error
    else:
        raise AssertionError(
            "release builder with persisted credentials must fail parity validation"
        )

    broken_ci, broken_release = fixture_workflows()
    release_gate_checkout = step_by_name(
        broken_release["jobs"]["release-gate"],
        "Checkout trusted release gate tooling",
        "Release/release-gate",
    )
    release_gate_checkout["with"]["ref"] = "${{ env.RELEASE_REF }}"
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "Release/release-gate: trusted tooling must use workflow identity" not in str(error):
            raise AssertionError(
                "release-gate trusted tooling failure was not attributed correctly"
            ) from error
    else:
        raise AssertionError(
            "release-ref checkout in release-gate must fail parity validation"
        )

    broken_ci, broken_release = fixture_workflows()
    release_gate_validation = step_by_name(
        broken_release["jobs"]["release-gate"],
        "Validate release provenance and version",
        "Release/release-gate",
    )
    release_gate_validation["run"] = release_gate_validation["run"].replace(
        ' --source-ref "$RELEASE_REF"',
        "",
    )
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "Release/release-gate" not in str(error):
            raise AssertionError(
                "release-gate source-data boundary failure was not attributed correctly"
            ) from error
    else:
        raise AssertionError(
            "release-gate without explicit tagged source data must fail parity validation"
        )

    broken_ci, broken_release = fixture_workflows()
    checkout_step = step_by_name(
        broken_release["jobs"]["release"],
        "Checkout trusted release tooling",
        "Release/release",
    )
    checkout_step["with"]["ref"] = "${{ env.RELEASE_REF }}"
    try:
        validate_parity(broken_ci, broken_release)
    except ParityError as error:
        if "write-capable tooling must use workflow identity" not in str(error):
            raise AssertionError(
                "trusted tooling boundary failure was not attributed correctly"
            ) from error
    else:
        raise AssertionError(
            "release-ref checkout in write-capable publication job must fail parity validation"
        )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate release-facing parity between CI and Release workflows."
    )
    parser.add_argument(
        "--ci",
        type=Path,
        default=Path(".github/workflows/ci.yml"),
        help="CI workflow path.",
    )
    parser.add_argument(
        "--release",
        type=Path,
        default=Path(".github/workflows/release.yml"),
        help="Release workflow path.",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run discriminating parity fixtures.",
    )
    args = parser.parse_args()

    try:
        if args.self_test:
            run_self_test()
            print("CI/Release parity self-test passed")
            return 0

        validate_parity(load_workflow(args.ci), load_workflow(args.release))
    except ParityError as error:
        print(f"CI/Release parity validation failed: {error}", file=sys.stderr)
        return 2

    print("CI/Release parity validation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
