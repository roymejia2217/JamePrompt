#!/usr/bin/env python3
"""Validate release tags before JamePrompt packages are built or published."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

from prepare_release_version import (
    ReleaseVersion,
    parse_tag,
    validate_release_version,
)
from validate_release_metadata import (
    ReleaseMetadata,
    ReleaseMetadataError,
    release_kind,
    render_tag_message,
    validate_changelog_for_tag,
)


class ReleaseGateError(ValueError):
    """Raised when a release does not satisfy the repository release contract."""


def run_git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args], text=True, capture_output=True, check=False
    )
    if result.returncode != 0:
        message = result.stderr.strip() or result.stdout.strip()
        raise ReleaseGateError(f"git {' '.join(args)} failed: {message}")
    return result.stdout.strip()


TAG_CONTENT_COMMAND = "git for-each-ref"


def validate_tag_annotation(tag: str, metadata: ReleaseMetadata) -> None:
    ref = f"refs/tags/{tag}"
    contents = run_git("for-each-ref", "--format=%(contents)", ref)
    expected = render_tag_message(metadata).strip()
    if contents.strip() != expected:
        raise ReleaseGateError(
            "annotated tag message does not match release metadata"
        )


def prerelease_numbers_for(version: ReleaseVersion, kind: str) -> list[int]:
    prefix = f"v{version.base}-{kind}."
    numbers: list[int] = []
    for tag in run_git("tag", "--list", f"{prefix}*").splitlines():
        suffix = tag.removeprefix(prefix)
        if suffix.isdecimal() and str(int(suffix)) == suffix:
            numbers.append(int(suffix))
    return numbers


def stable_exists_for(version: ReleaseVersion) -> bool:
    stable_tag = f"v{version.base}"
    return bool(run_git("tag", "--list", stable_tag).splitlines())


def validate_release_progression(
    version: ReleaseVersion,
    kind: str,
    *,
    alpha_numbers: list[int],
    beta_numbers: list[int],
    stable_exists: bool,
) -> None:
    if kind == "alpha":
        assert version.prerelease is not None
        number = int(version.prerelease.removeprefix("alpha."))
        if beta_numbers or stable_exists:
            raise ReleaseGateError(
                "alpha release is not allowed after beta or stable for the same version"
            )
        if any(existing > number for existing in alpha_numbers):
            raise ReleaseGateError(
                "alpha number must not be older than an existing alpha"
            )
        return

    if kind == "beta":
        assert version.prerelease is not None
        number = int(version.prerelease.removeprefix("beta."))
        if stable_exists:
            raise ReleaseGateError(
                "beta release is not allowed after stable for the same version"
            )
        if any(existing > number for existing in beta_numbers):
            raise ReleaseGateError(
                "beta number must not be older than an existing beta"
            )
        return

    if not beta_numbers:
        raise ReleaseGateError(
            "a stable release requires an existing beta for the same version"
        )


def validate_release(
    tag: str,
    main_ref: str,
    *,
    preflight: bool,
    target_ref: str | None,
) -> ReleaseVersion:
    version = parse_tag(tag)
    kind = release_kind(version)

    if preflight:
        if target_ref is None:
            raise ReleaseGateError("preflight validation requires --target-ref")
        target = run_git("rev-parse", "--verify", f"{target_ref}^{{commit}}")
    else:
        ref = f"refs/tags/{version.tag}"
        if run_git("cat-file", "-t", ref) != "tag":
            raise ReleaseGateError("release tags must be annotated Git tags")
        target = run_git("rev-parse", "--verify", f"{version.tag}^{{}}")

    main_commit = run_git("rev-parse", "--verify", f"{main_ref}^{{commit}}")
    ancestry = subprocess.run(
        ["git", "merge-base", "--is-ancestor", target, main_commit], check=False
    )
    if ancestry.returncode != 0:
        raise ReleaseGateError(
            "release target must be reachable from the protected main branch"
        )

    alpha_numbers = prerelease_numbers_for(version, "alpha")
    beta_numbers = prerelease_numbers_for(version, "beta")
    validate_release_progression(
        version,
        kind,
        alpha_numbers=alpha_numbers,
        beta_numbers=beta_numbers,
        stable_exists=stable_exists_for(version),
    )

    return version


def expect_progression_error(
    name: str,
    tag: str,
    *,
    alpha_numbers: list[int],
    beta_numbers: list[int],
    stable_exists: bool,
) -> None:
    version = parse_tag(tag)
    try:
        validate_release_progression(
            version,
            release_kind(version),
            alpha_numbers=alpha_numbers,
            beta_numbers=beta_numbers,
            stable_exists=stable_exists,
        )
    except ReleaseGateError:
        return
    raise AssertionError(f"{name} progression was accepted")


def self_test() -> None:
    assert release_kind(parse_tag("v1.2.0")) == "stable"
    assert release_kind(parse_tag("v1.2.0-alpha.1")) == "alpha"
    assert release_kind(parse_tag("v1.2.0-beta.9")) == "beta"

    validate_release_progression(
        parse_tag("v1.2.0-alpha.2"),
        "alpha",
        alpha_numbers=[1],
        beta_numbers=[],
        stable_exists=False,
    )
    validate_release_progression(
        parse_tag("v1.2.0-beta.10"),
        "beta",
        alpha_numbers=[1, 2],
        beta_numbers=[9],
        stable_exists=False,
    )
    validate_release_progression(
        parse_tag("v1.2.0"),
        "stable",
        alpha_numbers=[1, 2],
        beta_numbers=[9],
        stable_exists=False,
    )

    alpha_after_beta = "alpha_after_beta"
    expect_progression_error(
        alpha_after_beta,
        "v1.2.0-alpha.3",
        alpha_numbers=[1, 2],
        beta_numbers=[1],
        stable_exists=False,
    )
    alpha_after_stable = "alpha_after_stable"
    expect_progression_error(
        alpha_after_stable,
        "v1.2.0-alpha.3",
        alpha_numbers=[1, 2],
        beta_numbers=[],
        stable_exists=True,
    )
    beta_after_stable = "beta_after_stable"
    expect_progression_error(
        beta_after_stable,
        "v1.2.0-beta.10",
        alpha_numbers=[1, 2],
        beta_numbers=[9],
        stable_exists=True,
    )
    old_alpha = "old_alpha"
    expect_progression_error(
        old_alpha,
        "v1.2.0-alpha.1",
        alpha_numbers=[2],
        beta_numbers=[],
        stable_exists=False,
    )
    old_beta = "old_beta"
    expect_progression_error(
        old_beta,
        "v1.2.0-beta.8",
        alpha_numbers=[],
        beta_numbers=[9],
        stable_exists=False,
    )
    stable_without_beta = "stable_without_beta"
    expect_progression_error(
        stable_without_beta,
        "v1.2.0",
        alpha_numbers=[1],
        beta_numbers=[],
        stable_exists=False,
    )

    try:
        release_kind(parse_tag("v1.2.0-rc.1"))
    except ReleaseMetadataError:
        pass
    else:
        raise AssertionError("unsupported prerelease accepted")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate JamePrompt alpha, beta, and stable release tags."
    )
    parser.add_argument(
        "--tag",
        help="Release tag, for example v1.2.0-alpha.1, v1.2.0-beta.9, or v1.2.0",
    )
    parser.add_argument("--main-ref", default="origin/main")
    parser.add_argument("--preflight", action="store_true")
    parser.add_argument("--target-ref")
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("release gate contract: ok")
        return 0
    if not args.tag:
        parser.error("--tag is required unless --self-test is used")

    try:
        metadata = validate_changelog_for_tag(args.changelog, args.tag)
        version = validate_release(
            args.tag,
            args.main_ref,
            preflight=args.preflight,
            target_ref=args.target_ref,
        )
        validate_release_version(Path.cwd(), version)
        if not args.preflight:
            validate_tag_annotation(version.tag, metadata)
    except (ReleaseGateError, ReleaseMetadataError, ValueError) as error:
        print(f"release gate error: {error}", file=sys.stderr)
        return 2

    print(f"release gate: {version.tag} accepted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
