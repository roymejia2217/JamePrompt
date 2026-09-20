#!/usr/bin/env python3
"""Validate YAML syntax for tracked GitHub Actions workflow files."""

from __future__ import annotations

import argparse
import sys
import tempfile
from pathlib import Path

import yaml


def workflow_files(target: Path) -> list[Path]:
    if target.is_file():
        return [target]
    if target.is_dir():
        return sorted(
            path
            for pattern in ("*.yml", "*.yaml")
            for path in target.rglob(pattern)
            if path.is_file()
        )
    raise ValueError(f"workflow path does not exist: {target}")


def validate_workflow(path: Path) -> None:
    try:
        parsed = yaml.safe_load(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, yaml.YAMLError) as error:
        raise ValueError(f"{path}: invalid workflow YAML: {error}") from error

    if not isinstance(parsed, dict):
        raise ValueError(f"{path}: workflow root must be a YAML mapping")


def run_self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="jameprompt-workflow-yaml-") as temp:
        root = Path(temp)
        valid = root / "valid.yml"
        invalid = root / "invalid.yml"

        valid.write_text(
            "name: CI\non:\n  push:\njobs:\n  test:\n    runs-on: ubuntu-latest\n",
            encoding="utf-8",
        )
        invalid.write_text(
            "name: CI\njobs:\n  test: [\n",
            encoding="utf-8",
        )

        validate_workflow(valid)

        try:
            validate_workflow(invalid)
        except ValueError:
            pass
        else:
            raise AssertionError("invalid workflow YAML must be rejected")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate YAML syntax for GitHub Actions workflow files."
    )
    parser.add_argument(
        "path",
        nargs="?",
        default=".github/workflows",
        help="Workflow file or directory to validate.",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run the validator's discriminating valid/invalid fixture test.",
    )
    args = parser.parse_args()

    if args.self_test:
        run_self_test()
        return 0

    try:
        files = workflow_files(Path(args.path))
        if not files:
            raise ValueError(f"no workflow YAML files found under: {args.path}")
        for path in files:
            validate_workflow(path)
    except ValueError as error:
        print(error, file=sys.stderr)
        return 2

    print(f"Validated {len(files)} workflow YAML file(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
