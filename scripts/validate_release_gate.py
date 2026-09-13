#!/usr/bin/env python3
"""Validate release tags before JamePrompt packages are built or published."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys

from prepare_release_version import ReleaseVersion, parse_tag


BETA_PATTERN = re.compile(r"beta\.(0|[1-9]\d*)$")


class ReleaseGateError(ValueError):
    """Raised when a release does not satisfy the repository release contract."""


def release_kind(version: ReleaseVersion) -> str:
    if version.prerelease is None:
        return "stable"
    if BETA_PATTERN.fullmatch(version.prerelease):
        return "beta"
    raise ReleaseGateError(
        "release prerelease identifiers must use the SemVer beta.N form"
    )


def run_git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args], text=True, capture_output=True, check=False
    )
    if result.returncode != 0:
        message = result.stderr.strip() or result.stdout.strip()
        raise ReleaseGateError(f"git {' '.join(args)} failed: {message}")
    return result.stdout.strip()


def beta_numbers_for(version: ReleaseVersion) -> list[int]:
    prefix = f"v{version.base}-beta."
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
        raise ReleaseGateError("release target must be reachable from the protected main branch")

    beta_numbers = beta_numbers_for(version)
    if kind == "beta":
        assert version.prerelease is not None
        number = int(version.prerelease.removeprefix("beta."))
        if any(existing > number for existing in beta_numbers):
            raise ReleaseGateError("beta number must not be older than an existing beta")
    elif not beta_numbers:
        raise ReleaseGateError("a stable release requires an existing beta for the same version")

    return version


def self_test() -> None:
    assert release_kind(parse_tag("v1.2.0")) == "stable"
    assert release_kind(parse_tag("v1.2.0-beta.9")) == "beta"
    try:
        release_kind(parse_tag("v1.2.0-rc.1"))
    except ReleaseGateError:
        pass
    else:
        raise AssertionError("non-beta prerelease accepted")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate JamePrompt beta and stable release tags."
    )
    parser.add_argument("--tag", help="Release tag, for example v1.2.0-beta.9")
    parser.add_argument("--main-ref", default="origin/main")
    parser.add_argument("--preflight", action="store_true")
    parser.add_argument("--target-ref")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("release gate contract: ok")
        return 0
    if not args.tag:
        parser.error("--tag is required unless --self-test is used")

    try:
        version = validate_release(
            args.tag,
            args.main_ref,
            preflight=args.preflight,
            target_ref=args.target_ref,
        )
    except ReleaseGateError as error:
        print(f"release gate error: {error}", file=sys.stderr)
        return 2

    print(f"release gate: {version.tag} accepted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
