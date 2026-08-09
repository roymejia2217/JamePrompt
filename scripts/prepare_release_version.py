#!/usr/bin/env python3
from __future__ import annotations

import argparse
import dataclasses
import re
import sys
import tempfile
from pathlib import Path

SEMVER_TAG = re.compile(
    r"^v(?P<major>0|[1-9]\d*)\."
    r"(?P<minor>0|[1-9]\d*)\."
    r"(?P<patch>0|[1-9]\d*)"
    r"(?:-(?P<pre>[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)


@dataclasses.dataclass(frozen=True)
class ReleaseVersion:
    tag: str
    major: int
    minor: int
    patch: int
    prerelease: str | None = None

    @property
    def base(self) -> str:
        return f"{self.major}.{self.minor}.{self.patch}"

    @property
    def canonical(self) -> str:
        return self.base if self.prerelease is None else f"{self.base}-{self.prerelease}"

    @property
    def is_prerelease(self) -> bool:
        return self.prerelease is not None

    @property
    def debian(self) -> str:
        return self.base if self.prerelease is None else f"{self.base}~{self.prerelease}"

    @property
    def arch(self) -> str:
        if self.prerelease is None:
            return self.base
        normalized = re.sub(r"[^0-9A-Za-z.]+", ".", self.prerelease).strip(".")
        return f"{self.base}{normalized}"

    @property
    def rpm_version(self) -> str:
        return self.base

    @property
    def rpm_release(self) -> str:
        if self.prerelease is None:
            return "1"
        normalized = re.sub(r"[^0-9A-Za-z.]+", ".", self.prerelease).strip(".")
        return f"0.1.{normalized}"


def normalize_release_ref(value: str) -> str:
    value = value.strip()
    prefix = "release-candidate/"
    return value[len(prefix):] if value.startswith(prefix) else value


def parse_tag(tag: str) -> ReleaseVersion:
    tag = normalize_release_ref(tag)
    match = SEMVER_TAG.fullmatch(tag)
    if not match:
        raise ValueError(
            "release tag must use vMAJOR.MINOR.PATCH or "
            "vMAJOR.MINOR.PATCH-prerelease without build metadata"
        )
    pre = match.group("pre")
    return ReleaseVersion(
        tag=tag,
        major=int(match.group("major")),
        minor=int(match.group("minor")),
        patch=int(match.group("patch")),
        prerelease=pre,
    )


def replace_once(path: Path, pattern: str, replacement: str, *, flags: int = 0) -> None:
    original = path.read_text(encoding="utf-8")
    updated, count = re.subn(pattern, replacement, original, count=1, flags=flags)
    if count != 1:
        raise RuntimeError(f"expected exactly one version field in {path}, found {count}")
    path.write_text(updated, encoding="utf-8")


def apply_release_version(root: Path, version: ReleaseVersion) -> None:
    replace_once(
        root / "Cargo.toml",
        r'(?m)^version = "[^"]+"$',
        f'version = "{version.canonical}"',
    )
    replace_once(
        root / "Cargo.lock",
        r'(?m)(^\[\[package\]\]\nname = "jame-prompt"\nversion = ")[^"]+(")',
        rf'\g<1>{version.canonical}\2',
    )
    replace_once(
        root / "packaging/arch/PKGBUILD",
        r"(?m)^pkgver=.*$",
        f"pkgver={version.arch}",
    )
    replace_once(
        root / "packaging/arch/PKGBUILD",
        r"(?m)^pkgrel=.*$",
        "pkgrel=1",
    )
    replace_once(
        root / "packaging/rpm/jame-prompt.spec",
        r"(?m)^Version:\s+.*$",
        f"Version:        {version.rpm_version}",
    )
    replace_once(
        root / "packaging/rpm/jame-prompt.spec",
        r"(?m)^Release:\s+.*$",
        f"Release:        {version.rpm_release}%{{?dist}}",
    )


def write_github_output(path: Path, version: ReleaseVersion) -> None:
    values = {
        "tag": version.tag,
        "version": version.canonical,
        "prerelease": str(version.is_prerelease).lower(),
        "debian_version": version.debian,
        "arch_version": version.arch,
        "rpm_version": version.rpm_version,
        "rpm_release": version.rpm_release,
    }
    with path.open("a", encoding="utf-8") as handle:
        for key, value in values.items():
            handle.write(f"{key}={value}\n")


def self_test() -> None:
    stable = parse_tag("v1.2.0")
    assert stable.canonical == "1.2.0"
    assert stable.debian == "1.2.0"
    assert stable.arch == "1.2.0"
    assert stable.rpm_version == "1.2.0"
    assert stable.rpm_release == "1"
    assert not stable.is_prerelease

    beta = parse_tag("release-candidate/v1.2.0-beta.1")
    assert beta.canonical == "1.2.0-beta.1"
    assert beta.debian == "1.2.0~beta.1"
    assert beta.arch == "1.2.0beta.1"
    assert beta.rpm_version == "1.2.0"
    assert beta.rpm_release == "0.1.beta.1"
    assert beta.is_prerelease

    for invalid in (
        "1.2.0",
        "v1.2",
        "v01.2.0",
        "v1.2.0+build.1",
        "v1.2.0-",
        "release-v1.2.0",
    ):
        try:
            parse_tag(invalid)
        except ValueError:
            pass
        else:
            raise AssertionError(f"invalid tag was accepted: {invalid}")

    with tempfile.TemporaryDirectory() as temp:
        root = Path(temp)
        (root / "packaging/arch").mkdir(parents=True)
        (root / "packaging/rpm").mkdir(parents=True)
        (root / "Cargo.toml").write_text(
            '[package]\nname = "jame-prompt"\nversion = "1.1.0"\n',
            encoding="utf-8",
        )
        (root / "Cargo.lock").write_text(
            'version = 4\n\n[[package]]\nname = "jame-prompt"\nversion = "1.1.0"\n',
            encoding="utf-8",
        )
        (root / "packaging/arch/PKGBUILD").write_text(
            "pkgname=jame-prompt\npkgver=1.1.0\npkgrel=4\n",
            encoding="utf-8",
        )
        (root / "packaging/rpm/jame-prompt.spec").write_text(
            "Name: jame-prompt\nVersion: 1.1.0\nRelease: 1%{?dist}\n",
            encoding="utf-8",
        )
        apply_release_version(root, beta)
        assert 'version = "1.2.0-beta.1"' in (root / "Cargo.toml").read_text()
        assert 'version = "1.2.0-beta.1"' in (root / "Cargo.lock").read_text()
        assert "pkgver=1.2.0beta.1" in (root / "packaging/arch/PKGBUILD").read_text()
        rpm = (root / "packaging/rpm/jame-prompt.spec").read_text()
        assert "Version:        1.2.0" in rpm
        assert "Release:        0.1.beta.1%{?dist}" in rpm


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate and apply a JamePrompt release tag to package metadata."
    )
    parser.add_argument("--tag", help="Release tag, for example v1.2.0-beta.1")
    parser.add_argument("--apply", action="store_true", help="Patch the current workspace")
    parser.add_argument("--github-output", type=Path, help="Append release metadata for Actions")
    parser.add_argument("--self-test", action="store_true", help="Run deterministic contract tests")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("release version contract: ok")
        return 0

    if not args.tag:
        parser.error("--tag is required unless --self-test is used")

    try:
        version = parse_tag(args.tag)
        if args.apply:
            apply_release_version(args.root, version)
        if args.github_output:
            write_github_output(args.github_output, version)
    except (OSError, RuntimeError, ValueError) as error:
        print(f"release version error: {error}", file=sys.stderr)
        return 2

    print(
        f"{version.tag}: cargo={version.canonical} deb={version.debian} "
        f"arch={version.arch} rpm={version.rpm_version}-{version.rpm_release}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
