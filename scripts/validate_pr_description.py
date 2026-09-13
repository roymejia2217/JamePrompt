#!/usr/bin/env python3
"""Validate JamePrompt pull request descriptions without network access."""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from pathlib import Path


REQUIRED_HEADINGS = ("Summary", "Verification", "Release impact")
HEADING_PATTERN = re.compile(r"^##\s+(.+?)\s*$", re.MULTILINE)
COMMENT_PATTERN = re.compile(r"<!--.*?-->", re.DOTALL)
PLACEHOLDER_PATTERN = re.compile(r"(?:todo|tbd|n/?a|[-–—])", re.IGNORECASE)


class DescriptionError(ValueError):
    """Raised when a pull request body does not meet the repository contract."""


def sections(body: str) -> dict[str, str]:
    matches = list(HEADING_PATTERN.finditer(body))
    found: dict[str, str] = {}

    for index, match in enumerate(matches):
        heading = match.group(1).strip()
        if heading not in REQUIRED_HEADINGS:
            continue
        if heading in found:
            raise DescriptionError(f"duplicate required section: {heading}")
        end = matches[index + 1].start() if index + 1 < len(matches) else len(body)
        found[heading] = body[match.end() : end]

    return found


def normalized_content(value: str) -> str:
    return COMMENT_PATTERN.sub("", value).strip()


def validate_description(body: str) -> None:
    found = sections(body)
    missing = [heading for heading in REQUIRED_HEADINGS if heading not in found]
    if missing:
        raise DescriptionError(f"missing required section(s): {', '.join(missing)}")

    for heading, value in found.items():
        content = normalized_content(value)
        if not content:
            raise DescriptionError(f"required section is empty: {heading}")
        if heading != "Release impact" and PLACEHOLDER_PATTERN.fullmatch(content):
            raise DescriptionError(f"required section contains only a placeholder: {heading}")


def self_test() -> None:
    valid = """## Summary

Modernize the editor behavior.

## Verification

- `cargo test --locked --all-targets`

## Release impact

Beta candidate only.
"""
    validate_description(valid)

    for invalid, expected in (
        ("## Summary\n\nPresent", "missing required section"),
        (
            "## Summary\n\n<!-- fill this -->\n\n## Verification\n\n`cargo test`\n\n## Release impact\n\nNone",
            "required section is empty: Summary",
        ),
        (
            "## Summary\n\nDone\n\n## Verification\n\nTBD\n\n## Release impact\n\nNone",
            "placeholder: Verification",
        ),
        (
            "## Summary\n\nOne\n\n## Summary\n\nTwo\n\n## Verification\n\n`cargo test`\n\n## Release impact\n\nNone",
            "duplicate required section",
        ),
    ):
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
        validate_description(body_path.read_text(encoding="utf-8"))


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
        validate_description(args.body_file.read_text(encoding="utf-8"))
    except (OSError, DescriptionError) as error:
        print(f"pull request description error: {error}", file=sys.stderr)
        return 2

    print("pull request description contract: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
