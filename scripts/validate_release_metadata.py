#!/usr/bin/env python3
"""Validate and render JamePrompt release metadata from CHANGELOG.md."""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from dataclasses import dataclass
from datetime import date
from pathlib import Path
from typing import Any

from prepare_release_version import ReleaseVersion, parse_tag


CATEGORIES = ("Added", "Changed", "Deprecated", "Removed", "Fixed", "Security")
ALPHA_PATTERN = re.compile(r"alpha\.(0|[1-9]\d*)$")
BETA_PATTERN = re.compile(r"beta\.(0|[1-9]\d*)$")
VERSION_HEADING_PATTERN = re.compile(
    r"^## \[(?P<version>[^\]]+)\] - (?P<date>\d{4}-\d{2}-\d{2})\s*$",
    re.MULTILINE,
)
LEVEL_TWO_PATTERN = re.compile(r"^##\s+.+$", re.MULTILINE)
CATEGORY_PATTERN = re.compile(r"^###\s+(.+?)\s*$", re.MULTILINE)


class ReleaseMetadataError(ValueError):
    """Raised when changelog or GitHub Release metadata violates the contract."""


@dataclass(frozen=True)
class ReleaseMetadata:
    tag: str
    title: str
    kind: str
    notes: str

    @property
    def is_prerelease(self) -> bool:
        return self.kind != "stable"


def release_kind(version: ReleaseVersion) -> str:
    if version.prerelease is None:
        return "stable"
    if ALPHA_PATTERN.fullmatch(version.prerelease):
        return "alpha"
    if BETA_PATTERN.fullmatch(version.prerelease):
        return "beta"
    raise ReleaseMetadataError(
        "release prerelease identifiers must use the SemVer alpha.N or beta.N form"
    )


def validate_changelog_preamble(text: str) -> None:
    for required in (
        "# Changelog",
        "Keep a Changelog",
        "Semantic Versioning",
        "## [Unreleased]",
    ):
        if required not in text:
            raise ReleaseMetadataError(
                f"CHANGELOG.md is missing required declaration: {required}"
            )

    unreleased = text.find("## [Unreleased]")
    first_release = VERSION_HEADING_PATTERN.search(text)
    if first_release is not None and unreleased > first_release.start():
        raise ReleaseMetadataError("Unreleased must precede released versions")


def release_block(text: str, canonical_version: str) -> str:
    matches = [
        match
        for match in VERSION_HEADING_PATTERN.finditer(text)
        if match.group("version") == canonical_version
    ]
    if len(matches) != 1:
        raise ReleaseMetadataError(
            f"CHANGELOG.md must contain exactly one release entry for {canonical_version}"
        )

    match = matches[0]
    try:
        date.fromisoformat(match.group("date"))
    except ValueError as error:
        raise ReleaseMetadataError(
            f"release date must use ISO YYYY-MM-DD: {match.group('date')}"
        ) from error

    next_heading = LEVEL_TWO_PATTERN.search(text, match.end())
    end = next_heading.start() if next_heading is not None else len(text)
    body = text[match.end() : end].strip()

    category_matches = list(CATEGORY_PATTERN.finditer(body))
    categories = [category.group(1).strip() for category in category_matches]
    if tuple(categories) != CATEGORIES:
        raise ReleaseMetadataError(
            "release entry categories must appear exactly in this order: "
            + ", ".join(CATEGORIES)
        )

    meaningful_entries = 0
    for index, category in enumerate(category_matches):
        section_end = (
            category_matches[index + 1].start()
            if index + 1 < len(category_matches)
            else len(body)
        )
        content = body[category.end() : section_end].strip()
        lines = [line.strip() for line in content.splitlines() if line.strip()]
        if not lines:
            raise ReleaseMetadataError(
                f"release category is empty: {category.group(1).strip()}"
            )
        for line in lines:
            if not line.startswith("- "):
                raise ReleaseMetadataError(
                    f"release category entries must be Markdown bullets: {line}"
                )
            value = line[2:].strip()
            if not value:
                raise ReleaseMetadataError("release changelog bullets must not be empty")
            if value.casefold() not in {"none.", "none"}:
                meaningful_entries += 1

    if meaningful_entries == 0:
        raise ReleaseMetadataError(
            "release changelog must contain at least one notable change"
        )

    return body + "\n"


def validate_changelog_for_tag(path: Path, tag: str) -> ReleaseMetadata:
    version = parse_tag(tag)
    kind = release_kind(version)
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        raise ReleaseMetadataError(f"unable to read {path}: {error}") from error

    validate_changelog_preamble(text)
    notes = release_block(text, version.canonical)
    return ReleaseMetadata(
        tag=version.tag,
        title=version.tag,
        kind=kind,
        notes=notes,
    )


def validate_existing_release(payload: dict[str, Any], metadata: ReleaseMetadata) -> None:
    expected = {
        "tagName": metadata.tag,
        "name": metadata.title,
        "isPrerelease": metadata.is_prerelease,
    }
    for key, value in expected.items():
        if payload.get(key) != value:
            raise ReleaseMetadataError(
                f"existing GitHub Release {key} mismatch: expected {value!r}, "
                f"got {payload.get(key)!r}"
            )

    body = payload.get("body")
    if not isinstance(body, str):
        raise ReleaseMetadataError("existing GitHub Release body must be text")
    if body.strip() != metadata.notes.strip():
        raise ReleaseMetadataError(
            "existing GitHub Release body does not match CHANGELOG.md"
        )


def fixture_changelog(version: str) -> str:
    return f"""# Changelog

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

## [{version}] - 2026-09-21

### Added

- Add deterministic repository metadata gates.

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
"""


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="jameprompt-release-metadata-") as temp:
        root = Path(temp)
        for tag in ("v1.2.0-alpha.1", "v1.2.0-beta.10", "v1.2.0"):
            version = parse_tag(tag)
            changelog_path = root / f"{version.canonical}.md"
            changelog_path.write_text(
                fixture_changelog(version.canonical),
                encoding="utf-8",
            )
            metadata = validate_changelog_for_tag(changelog_path, tag)
            assert metadata.title == tag
            assert metadata.kind in {"alpha", "beta", "stable"}
            assert metadata.is_prerelease == (metadata.kind != "stable")

            payload = {
                "tagName": tag,
                "name": tag,
                "body": metadata.notes,
                "isPrerelease": metadata.is_prerelease,
            }
            validate_existing_release(payload, metadata)

        invalid_kind = root / "invalid-kind.md"
        invalid_kind.write_text(fixture_changelog("1.2.0-rc.1"), encoding="utf-8")
        try:
            validate_changelog_for_tag(invalid_kind, "v1.2.0-rc.1")
        except ReleaseMetadataError:
            pass
        else:
            raise AssertionError("unsupported rc prerelease was accepted")

        empty_release = root / "empty.md"
        empty_release.write_text(
            fixture_changelog("1.2.0-alpha.2").replace(
                "- Add deterministic repository metadata gates.",
                "- None.",
            ),
            encoding="utf-8",
        )
        try:
            validate_changelog_for_tag(empty_release, "v1.2.0-alpha.2")
        except ReleaseMetadataError:
            pass
        else:
            raise AssertionError("release with no notable changes was accepted")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate JamePrompt release metadata from CHANGELOG.md."
    )
    parser.add_argument("--tag")
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    parser.add_argument("--write-notes", type=Path)
    parser.add_argument("--existing-release-json", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("release metadata contract: ok")
        return 0
    if not args.tag:
        parser.error("--tag is required unless --self-test is used")

    try:
        metadata = validate_changelog_for_tag(args.changelog, args.tag)
        if args.write_notes is not None:
            args.write_notes.write_text(metadata.notes, encoding="utf-8")
        if args.existing_release_json is not None:
            payload = json.loads(
                args.existing_release_json.read_text(encoding="utf-8")
            )
            if not isinstance(payload, dict):
                raise ReleaseMetadataError(
                    "existing GitHub Release JSON root must be an object"
                )
            validate_existing_release(payload, metadata)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"release metadata error: {error}", file=sys.stderr)
        return 2

    print(f"release metadata contract: ok ({metadata.tag}, kind={metadata.kind})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
