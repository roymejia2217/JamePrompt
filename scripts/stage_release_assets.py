#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import shutil
import sys
import tempfile
from pathlib import Path

from prepare_release_version import parse_tag

EXPECTED_PRERELEASE = {
    "appimage",
    "arch",
    "deb",
    "rpm",
    "windows-portable",
}
EXPECTED_STABLE = EXPECTED_PRERELEASE | {"windows-msi"}


def normalized_name(path: Path) -> str:
    name = path.name
    if name.endswith(".deb"):
        name = name.replace("~", "-")
    return name


def classify(name: str) -> str:
    if "-debug-" in name and name.endswith(".pkg.tar.zst"):
        raise ValueError(f"debug package must not be published: {name}")
    if name.endswith(".AppImage"):
        return "appimage"
    if name.endswith(".pkg.tar.zst"):
        return "arch"
    if name.endswith(".deb"):
        return "deb"
    if name.endswith(".rpm"):
        return "rpm"
    if name.endswith("-x64-portable.zip"):
        return "windows-portable"
    if name.endswith(".msi"):
        return "windows-msi"
    raise ValueError(f"unsupported release artifact: {name}")


def stage_release_assets(source: Path, destination: Path, *, prerelease: bool) -> list[Path]:
    source = source.resolve()
    destination = destination.resolve()

    if not source.is_dir():
        raise ValueError(f"release artifact source is not a directory: {source}")
    if source == destination:
        raise ValueError("source and destination must be different directories")

    files = sorted(
        (
            path
            for path in source.rglob("*")
            if path.is_file() and path.name != "SHA256SUMS"
        ),
        key=lambda path: path.as_posix(),
    )
    if not files:
        raise ValueError("no release artifacts were found")

    expected = EXPECTED_PRERELEASE if prerelease else EXPECTED_STABLE
    categories: dict[str, str] = {}
    staged_names: set[str] = set()

    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)

    try:
        for path in files:
            name = normalized_name(path)
            category = classify(name)

            if category in categories:
                raise ValueError(
                    f"multiple {category} artifacts: {categories[category]} and {path}"
                )
            if name in staged_names:
                raise ValueError(f"release artifact basename collision: {name}")

            categories[category] = str(path)
            staged_names.add(name)
            shutil.copy2(path, destination / name)

        observed = set(categories)
        if observed != expected:
            missing = sorted(expected - observed)
            unexpected = sorted(observed - expected)
            raise ValueError(
                f"release artifact set mismatch; missing={missing}, unexpected={unexpected}"
            )
    except Exception:
        shutil.rmtree(destination, ignore_errors=True)
        raise

    return sorted(destination.iterdir(), key=lambda path: path.name)


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temp:
        root = Path(temp)
        source = root / "source"
        stage = root / "stage"
        (source / "x86_64").mkdir(parents=True)

        fixtures = {
            "JamePrompt-1.2.0-beta.3-x64-portable.zip": b"portable",
            "jame-prompt-1.2.0-beta.3-x86_64.AppImage": b"appimage",
            "jame-prompt-1.2.0beta.3-1-x86_64.pkg.tar.zst": b"arch",
            "jame-prompt_1.2.0~beta.3_amd64.deb": b"deb",
        }
        for name, content in fixtures.items():
            (source / name).write_bytes(content)
        (source / "x86_64" / "jame-prompt-1.2.0-0.1.beta.3.fc40.x86_64.rpm").write_bytes(
            b"rpm"
        )

        staged = stage_release_assets(source, stage, prerelease=True)
        names = [path.name for path in staged]
        assert names == sorted(
            [
                "JamePrompt-1.2.0-beta.3-x64-portable.zip",
                "jame-prompt-1.2.0-0.1.beta.3.fc40.x86_64.rpm",
                "jame-prompt-1.2.0-beta.3-x86_64.AppImage",
                "jame-prompt-1.2.0beta.3-1-x86_64.pkg.tar.zst",
                "jame-prompt_1.2.0-beta.3_amd64.deb",
            ]
        )
        assert not any(path.parent != stage for path in staged)

        try:
            stage_release_assets(source, stage, prerelease=False)
        except ValueError as error:
            assert "windows-msi" in str(error)
        else:
            raise AssertionError("stable release without MSI was accepted")

        (source / "JamePrompt-1.2.0-x64.msi").write_bytes(b"msi")
        stable_staged = stage_release_assets(source, stage, prerelease=False)
        assert len(stable_staged) == 6
        (source / "JamePrompt-1.2.0-x64.msi").unlink()

        (source / "jame-prompt-debug-1.2.0beta.3-1-x86_64.pkg.tar.zst").write_bytes(
            b"debug"
        )
        try:
            stage_release_assets(source, stage, prerelease=True)
        except ValueError as error:
            assert "debug package must not be published" in str(error)
        else:
            raise AssertionError("debug package was accepted")

    stable = parse_tag("v1.2.0")
    prerelease = parse_tag("v1.2.0-beta.3")
    assert not stable.is_prerelease
    assert prerelease.is_prerelease


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Flatten, normalize, and validate JamePrompt release artifacts."
    )
    parser.add_argument("source", nargs="?", type=Path)
    parser.add_argument("destination", nargs="?", type=Path)
    parser.add_argument("--tag", help="Release tag, for example v1.2.0-beta.3")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("release staging contract: ok")
        return 0

    if args.source is None or args.destination is None or not args.tag:
        parser.error("source, destination, and --tag are required unless --self-test is used")

    try:
        version = parse_tag(args.tag)
        staged = stage_release_assets(
            args.source,
            args.destination,
            prerelease=version.is_prerelease,
        )
    except (OSError, ValueError) as error:
        print(f"release staging error: {error}", file=sys.stderr)
        return 2

    for path in staged:
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        print(f"{digest}  {path.name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
