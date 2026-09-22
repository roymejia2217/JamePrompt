#!/usr/bin/env python3
"""Validate JamePrompt pull request descriptions without network access."""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from pathlib import Path


REQUIRED_HEADINGS = (
    "Summary",
    "Motivation",
    "Changes",
    "Verification",
    "Risk and rollback",
    "Release impact",
)
RELEASE_TYPES = {"none", "alpha", "beta", "stable"}
HEADING_PATTERN = re.compile(r"^##\s+(.+?)\s*$", re.MULTILINE)
COMMENT_PATTERN = re.compile(r"<!--.*?-->", re.DOTALL)
PLACEHOLDER_PATTERN = re.compile(r"(?:todo|tbd|n/?a|[-–—])", re.IGNORECASE)
RELEASE_TYPE_PATTERN = re.compile(r"^Release-Type:\s*(\S+)\s*$")
RELEASE_REASON_PATTERN = re.compile(r"^Release-Reason:\s*(.+?)\s*$")


class DescriptionError(ValueError):
    """Raised when a pull request body does not meet the repository contract."""


def sections(body: str) -> dict[str, str]:
    matches = list(HEADING_PATTERN.finditer(body))
    headings = [match.group(1).strip() for match in matches]

    unexpected = [heading for heading in headings if heading not in REQUIRED_HEADINGS]
    if unexpected:
        raise DescriptionError(
            f"unexpected level-2 section(s): {', '.join(unexpected)}"
        )

    duplicates = [
        heading
        for heading in REQUIRED_HEADINGS
        if headings.count(heading) > 1
    ]
    if duplicates:
        raise DescriptionError(
            f"duplicate required section(s): {', '.join(duplicates)}"
        )

    missing = [heading for heading in REQUIRED_HEADINGS if heading not in headings]
    if missing:
        raise DescriptionError(f"missing required section(s): {', '.join(missing)}")

    if tuple(headings) != REQUIRED_HEADINGS:
        raise DescriptionError("required sections are out of order")

    found: dict[str, str] = {}
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(body)
        found[headings[index]] = body[match.end() : end]

    return found


def normalized_content(value: str) -> str:
    return COMMENT_PATTERN.sub("", value).strip()


def parse_release_impact(value: str) -> str:
    content = normalized_content(value)
    lines = [line.strip() for line in content.splitlines() if line.strip()]
    if len(lines) != 2:
        raise DescriptionError(
            "release impact must contain exactly Release-Type and Release-Reason"
        )

    type_match = RELEASE_TYPE_PATTERN.fullmatch(lines[0])
    if type_match is None:
        raise DescriptionError("release impact must start with Release-Type")
    release_type = type_match.group(1)
    if release_type not in RELEASE_TYPES:
        raise DescriptionError(f"invalid Release-Type: {release_type}")

    reason_match = RELEASE_REASON_PATTERN.fullmatch(lines[1])
    if reason_match is None:
        raise DescriptionError("release impact must include Release-Reason")
    reason = reason_match.group(1).strip()
    if len(reason) < 20:
        raise DescriptionError("Release-Reason must contain at least 20 characters")
    if reason.startswith("<") and reason.endswith(">"):
        raise DescriptionError("Release-Reason must not be a template placeholder")

    return release_type


def validate_description(body: str) -> str:
    found = sections(body)
    for heading in REQUIRED_HEADINGS:
        content = normalized_content(found[heading])
        if not content:
            raise DescriptionError(f"required section is empty: {heading}")
        if PLACEHOLDER_PATTERN.fullmatch(content):
            raise DescriptionError(
                f"required section contains only a placeholder: {heading}"
            )

    return parse_release_impact(found["Release impact"])


def self_test() -> None:
    valid = """## Summary

Modernize the editor behavior.

## Motivation

The existing behavior can lose focus during prompt edits.

## Changes

- Preserve focus while applying the editor update.

## Verification

- `cargo test --locked --all-targets`

## Risk and rollback

Low risk. Revert the focused editor commit if regression evidence appears.

## Release impact

Release-Type: beta
Release-Reason: This PR prepares the next beta validation candidate.
"""
    assert validate_description(valid) == "beta"

    for release_type in ("alpha", "stable", "none"):
        candidate = valid.replace("Release-Type: beta", f"Release-Type: {release_type}")
        assert validate_description(candidate) == release_type

    invalid_cases = [
        ("## Summary\n\nPresent", "missing required section"),
        (
            valid.replace("Modernize the editor behavior.", "<!-- fill this -->"),
            "required section is empty: Summary",
        ),
        (
            valid.replace("- `cargo test --locked --all-targets`", "TBD"),
            "placeholder: Verification",
        ),
        (
            valid.replace(
                "## Motivation\n\nThe existing behavior can lose focus during prompt edits.\n\n"
                "## Changes\n\n- Preserve focus while applying the editor update.",
                "## Changes\n\n- Preserve focus while applying the editor update.\n\n"
                "## Motivation\n\nThe existing behavior can lose focus during prompt edits.",
            ),
            "required sections are out of order",
        ),
        (
            valid.replace(
                "## Motivation",
                "## Unreviewed\n\nSomething.\n\n## Motivation",
            ),
            "unexpected level-2 section",
        ),
        (
            valid.replace(
                "## Verification",
                "## Summary\n\nDuplicate.\n\n## Verification",
            ),
            "duplicate required section",
        ),
        (
            valid.replace("Release-Type: beta", "Release-Type: rc"),
            "invalid Release-Type",
        ),
        (
            valid.replace(
                "Release-Type: beta",
                "Release-Type: <none|alpha|beta|stable>",
            ),
            "invalid Release-Type",
        ),
        (
            valid.replace(
                "Release-Reason: This PR prepares the next beta validation candidate.",
                "Release-Reason: too short",
            ),
            "Release-Reason must contain at least 20 characters",
        ),
        (
            valid.replace(
                "Release-Reason: This PR prepares the next beta validation candidate.",
                "Release-Reason: <explain why this PR does or does not require a release>",
            ),
            "Release-Reason must not be a template placeholder",
        ),
        (
            valid.replace(
                "Release-Reason: This PR prepares the next beta validation candidate.",
                "Release-Reason: This reason is long enough to be valid.\nExtra: forbidden",
            ),
            "release impact must contain exactly Release-Type and Release-Reason",
        ),
    ]

    for invalid, expected in invalid_cases:
        try:
            validate_description(invalid)
        except DescriptionError as error:
            if expected not in str(error):
                raise AssertionError(
                    f"expected {expected!r}, got {str(error)!r}"
                ) from error
        else:
            raise AssertionError(f"invalid description accepted: {invalid!r}")

    with tempfile.TemporaryDirectory() as directory:
        body_path = Path(directory) / "pr.md"
        body_path.write_text(valid, encoding="utf-8")
        assert validate_description(body_path.read_text(encoding="utf-8")) == "beta"


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate the required structure of a JamePrompt pull request description."
    )
    parser.add_argument("--body-file", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("pull request description contract: ok")
        return 0
    if args.body_file is None:
        parser.error("--body-file is required unless --self-test is used")

    try:
        release_type = validate_description(
            args.body_file.read_text(encoding="utf-8")
        )
    except (OSError, DescriptionError) as error:
        print(f"pull request description error: {error}", file=sys.stderr)
        return 2

    print(f"pull request description contract: ok (release-type={release_type})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
