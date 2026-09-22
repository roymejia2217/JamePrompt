#!/usr/bin/env python3
"""Validate release tags before JamePrompt packages are built or published."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

from prepare_release_version import ReleaseVersion, parse_tag
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

    if kind in {"alpha", "beta"}:
        assert version.prerelease is not None
        number = int(version.prerelease.removeprefix(f"{kind}."))
        existing_numbers = prerelease_numbers_for(version, kind)
        if any(existing > number for existing in existing_numbers):
            raise ReleaseGateError(
                f"{kind} number must not be older than an existing {kind}"
            )
    else:
        beta_numbers = prerelease_numbers_for(version, "beta")
        if not beta_numbers:
            raise ReleaseGateError(
                "a stable release requires an existing beta for the same version"
            )

    return version


def self_test() -> None:
    assert release_kind(parse_tag("v1.2.0")) == "stable"
    assert release_kind(parse_tag("v1.2.0-alpha.1")) == "alpha"
    assert release_kind(parse_tag("v1.2.0-beta.9")) == "beta"
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
        if not args.preflight:
            validate_tag_annotation(version.tag, metadata)
    except (ReleaseGateError, ReleaseMetadataError, ValueError) as error:
        print(f"release gate error: {error}", file=sys.stderr)
        return 2

    print(f"release gate: {version.tag} accepted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
