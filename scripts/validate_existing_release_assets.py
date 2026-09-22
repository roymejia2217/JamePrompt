#!/usr/bin/env python3
"""Fail closed when recovery would replace published GitHub Release assets."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tempfile
from pathlib import Path
from typing import Any


class ReleaseAssetError(ValueError):
    """Raised when published assets differ from staged release files."""


def sha256_digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def local_assets(asset_dir: Path) -> dict[str, dict[str, Any]]:
    try:
        paths = sorted(path for path in asset_dir.iterdir() if path.is_file())
    except OSError as error:
        raise ReleaseAssetError(f"unable to inspect asset directory: {error}") from error

    if not paths:
        raise ReleaseAssetError("release asset directory contains no files")

    assets: dict[str, dict[str, Any]] = {}
    for path in paths:
        if "\n" in path.name or "\r" in path.name:
            raise ReleaseAssetError("release asset filenames must not contain newlines")
        if path.name in assets:
            raise ReleaseAssetError(f"duplicate staged release asset: {path.name}")
        assets[path.name] = {
            "path": path,
            "size": path.stat().st_size,
            "digest": sha256_digest(path),
        }
    return assets


def validate_existing_assets(
    payload: dict[str, Any],
    asset_dir: Path,
) -> list[Path]:
    raw_assets = payload.get("assets")
    if not isinstance(raw_assets, list):
        raise ReleaseAssetError("GitHub Release JSON assets must be a list")

    staged = local_assets(asset_dir)
    published: dict[str, dict[str, Any]] = {}

    for raw in raw_assets:
        if not isinstance(raw, dict):
            raise ReleaseAssetError("GitHub Release asset entry must be an object")
        name = raw.get("name")
        if not isinstance(name, str) or not name:
            raise ReleaseAssetError("GitHub Release asset name must be non-empty text")
        if name in published:
            raise ReleaseAssetError(f"duplicate published release asset: {name}")
        if name not in staged:
            raise ReleaseAssetError(f"unexpected published release asset: {name}")
        published[name] = raw

        expected = staged[name]
        state = raw.get("state")
        size = raw.get("size")
        digest = raw.get("digest")
        if (
            state != "uploaded"
            or size != expected["size"]
            or digest != expected["digest"]
        ):
            raise ReleaseAssetError(
                "published release asset mismatch: "
                f"{name}: expected state='uploaded', size={expected['size']}, "
                f"digest={expected['digest']}; got state={state!r}, "
                f"size={size!r}, digest={digest!r}"
            )

    return [
        staged[name]["path"]
        for name in sorted(staged)
        if name not in published
    ]


def expect_error(
    name: str,
    payload: dict[str, Any],
    asset_dir: Path,
    expected: str,
) -> None:
    try:
        validate_existing_assets(payload, asset_dir)
    except ReleaseAssetError as error:
        if expected not in str(error):
            raise AssertionError(
                f"{name}: expected {expected!r}, got {str(error)!r}"
            ) from error
        return
    raise AssertionError(f"{name}: invalid published assets were accepted")


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="jameprompt-release-assets-") as temp:
        root = Path(temp)
        asset_dir = root / "assets"
        asset_dir.mkdir()
        first = asset_dir / "jame-prompt.bin"
        second = asset_dir / "SHA256SUMS"
        first.write_bytes(b"artifact-bytes")
        second.write_text("checksum manifest\n", encoding="utf-8")

        first_asset = {
            "name": first.name,
            "state": "uploaded",
            "size": first.stat().st_size,
            "digest": sha256_digest(first),
        }
        second_asset = {
            "name": second.name,
            "state": "uploaded",
            "size": second.stat().st_size,
            "digest": sha256_digest(second),
        }

        missing = validate_existing_assets({"assets": [first_asset]}, asset_dir)
        assert missing == [second]

        complete = validate_existing_assets(
            {"assets": [first_asset, second_asset]},
            asset_dir,
        )
        assert complete == []

        mismatch = dict(first_asset)
        mismatch["digest"] = "sha256:" + ("0" * 64)
        expect_error(
            "digest mismatch",
            {"assets": [mismatch]},
            asset_dir,
            "published release asset mismatch",
        )

        wrong_size = dict(first_asset)
        wrong_size["size"] = first.stat().st_size + 1
        expect_error(
            "size mismatch",
            {"assets": [wrong_size]},
            asset_dir,
            "published release asset mismatch",
        )

        incomplete = dict(first_asset)
        incomplete["state"] = "new"
        expect_error(
            "state mismatch",
            {"assets": [incomplete]},
            asset_dir,
            "published release asset mismatch",
        )

        unexpected = dict(first_asset)
        unexpected["name"] = "unexpected.bin"
        expect_error(
            "unexpected asset",
            {"assets": [unexpected]},
            asset_dir,
            "unexpected published release asset",
        )

        expect_error(
            "duplicate asset",
            {"assets": [first_asset, first_asset]},
            asset_dir,
            "duplicate published release asset",
        )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate immutable assets for an existing GitHub Release."
    )
    parser.add_argument("--release-json", type=Path)
    parser.add_argument("--asset-dir", type=Path)
    parser.add_argument("--write-missing", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("published release asset immutability self-test passed")
        return 0

    if args.release_json is None or args.asset_dir is None or args.write_missing is None:
        parser.error(
            "--release-json, --asset-dir, and --write-missing are required"
        )

    try:
        payload = json.loads(args.release_json.read_text(encoding="utf-8"))
        if not isinstance(payload, dict):
            raise ReleaseAssetError("GitHub Release JSON root must be an object")
        missing = validate_existing_assets(payload, args.asset_dir)
        args.write_missing.write_text(
            "".join(f"{path.as_posix()}\n" for path in missing),
            encoding="utf-8",
        )
    except (OSError, json.JSONDecodeError, ReleaseAssetError) as error:
        print(f"release asset immutability error: {error}", file=sys.stderr)
        return 2

    print(
        "published release assets validated: "
        f"{len(missing)} missing asset(s) may be uploaded"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
