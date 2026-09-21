#!/usr/bin/env python3
"""Validate provenance for workflow_runs reused by release publication recovery."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.request
from typing import Any

from prepare_release_version import parse_tag

API_ROOT = "https://api.github.com"
API_VERSION = "2022-11-28"
WORKFLOW_NAME = "Release"
WORKFLOW_PATH = ".github/workflows/release.yml"
REQUIRED_JOBS = {
    "release-gate",
    "deb",
    "arch",
    "rpm",
    "appimage",
    "windows",
}
EXPECTED_PRERELEASE_ARTIFACTS = {
    "deb-package",
    "arch-package",
    "rpm-package",
    "appimage-package",
    "windows-portable",
}
EXPECTED_STABLE_ARTIFACTS = EXPECTED_PRERELEASE_ARTIFACTS | {"windows-msi"}
REPOSITORY_PATTERN = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")


class ReusableRunError(ValueError):
    """Raised when an existing Release run cannot be safely reused."""


def api_headers() -> dict[str, str]:
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if not token:
        raise ReusableRunError("GH_TOKEN or GITHUB_TOKEN is required")
    return {
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "X-GitHub-Api-Version": API_VERSION,
        "User-Agent": "JamePrompt-reusable-release-provenance",
    }


def fetch_json(repository: str, path: str) -> dict[str, Any]:
    if not REPOSITORY_PATTERN.fullmatch(repository):
        raise ReusableRunError(f"invalid GitHub repository identifier: {repository}")
    request = urllib.request.Request(
        f"{API_ROOT}/repos/{repository}/{path}",
        headers=api_headers(),
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            payload = json.load(response)
    except (
        urllib.error.HTTPError,
        urllib.error.URLError,
        TimeoutError,
        json.JSONDecodeError,
    ) as error:
        raise ReusableRunError(f"unable to query GitHub Actions provenance: {error}") from error
    if not isinstance(payload, dict):
        raise ReusableRunError("GitHub Actions response root must be an object")
    return payload


def expected_artifacts(tag: str) -> set[str]:
    version = parse_tag(tag)
    return (
        EXPECTED_PRERELEASE_ARTIFACTS
        if version.is_prerelease
        else EXPECTED_STABLE_ARTIFACTS
    )


def validate_run(
    run: dict[str, Any],
    *,
    run_id: int,
    tag: str,
    target_sha: str,
    current_run_id: int,
) -> None:
    if run_id == current_run_id:
        raise ReusableRunError("publish_run_id must reference a different workflow run")
    if run.get("id") != run_id:
        raise ReusableRunError(
            f"workflow run id mismatch: expected {run_id}, got {run.get('id')}"
        )
    if run.get("name") != WORKFLOW_NAME:
        raise ReusableRunError(
            f"unexpected workflow name: expected '{WORKFLOW_NAME}', got {run.get('name')!r}"
        )
    if run.get("path") != WORKFLOW_PATH:
        raise ReusableRunError(
            f"unexpected workflow path: expected '{WORKFLOW_PATH}', got {run.get('path')!r}"
        )
    if run.get("event") != "push":
        raise ReusableRunError(
            f"reusable release run must be tag-triggered push, got {run.get('event')!r}"
        )
    if run.get("head_branch") != tag:
        raise ReusableRunError(
            f"reusable release run tag mismatch: expected '{tag}', got {run.get('head_branch')!r}"
        )
    if run.get("head_sha") != target_sha:
        raise ReusableRunError(
            "reusable release run SHA mismatch: "
            f"expected '{target_sha}', got {run.get('head_sha')!r}"
        )
    if run.get("status") != "completed":
        raise ReusableRunError(
            f"reusable release run must be completed, got {run.get('status')!r}"
        )


def validate_jobs(payload: dict[str, Any]) -> None:
    jobs = payload.get("jobs")
    if not isinstance(jobs, list):
        raise ReusableRunError("workflow job response is missing jobs")

    by_name: dict[str, list[dict[str, Any]]] = {}
    for job in jobs:
        if isinstance(job, dict) and isinstance(job.get("name"), str):
            by_name.setdefault(job["name"], []).append(job)

    for job_name in sorted(REQUIRED_JOBS):
        matches = by_name.get(job_name, [])
        if len(matches) != 1:
            raise ReusableRunError(
                f"required job '{job_name}' must appear exactly once, found {len(matches)}"
            )
        job = matches[0]
        if job.get("status") != "completed" or job.get("conclusion") != "success":
            raise ReusableRunError(
                f"required job '{job_name}' is not successful: "
                f"status={job.get('status')!r}, conclusion={job.get('conclusion')!r}"
            )


def validate_artifacts(payload: dict[str, Any], tag: str) -> None:
    artifacts = payload.get("artifacts")
    if not isinstance(artifacts, list):
        raise ReusableRunError("workflow artifact response is missing artifacts")

    expected = expected_artifacts(tag)
    by_name: dict[str, list[dict[str, Any]]] = {}
    for artifact in artifacts:
        if isinstance(artifact, dict) and isinstance(artifact.get("name"), str):
            by_name.setdefault(artifact["name"], []).append(artifact)

    observed = set(by_name)
    if observed != expected:
        raise ReusableRunError(
            "reusable release artifact set mismatch; "
            f"missing={sorted(expected - observed)}, "
            f"unexpected={sorted(observed - expected)}"
        )

    for name in sorted(expected):
        matches = by_name[name]
        if len(matches) != 1:
            raise ReusableRunError(
                f"artifact '{name}' must appear exactly once, found {len(matches)}"
            )
        artifact = matches[0]
        if artifact.get("expired") is not False:
            raise ReusableRunError(f"artifact '{name}' is expired or has unknown expiry state")
        size = artifact.get("size_in_bytes")
        if not isinstance(size, int) or size <= 0:
            raise ReusableRunError(
                f"artifact '{name}' must be non-empty, got size_in_bytes={size!r}"
            )


def validate_reusable_release_run(
    run: dict[str, Any],
    jobs: dict[str, Any],
    artifacts: dict[str, Any],
    *,
    run_id: int,
    tag: str,
    target_sha: str,
    current_run_id: int,
) -> None:
    validate_run(
        run,
        run_id=run_id,
        tag=tag,
        target_sha=target_sha,
        current_run_id=current_run_id,
    )
    validate_jobs(jobs)
    validate_artifacts(artifacts, tag)


def fixture(
    *,
    tag: str = "v1.2.0-beta.10",
    target_sha: str = "a" * 40,
    run_id: int = 100,
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    run = {
        "id": run_id,
        "name": WORKFLOW_NAME,
        "path": WORKFLOW_PATH,
        "event": "push",
        "head_branch": tag,
        "head_sha": target_sha,
        "status": "completed",
        "conclusion": "failure",
    }
    jobs = {
        "jobs": [
            {
                "name": name,
                "status": "completed",
                "conclusion": "success",
            }
            for name in sorted(REQUIRED_JOBS)
        ]
    }
    artifacts = {
        "artifacts": [
            {
                "name": name,
                "expired": False,
                "size_in_bytes": 1024,
            }
            for name in sorted(expected_artifacts(tag))
        ]
    }
    return run, jobs, artifacts


def expect_failure(
    run: dict[str, Any],
    jobs: dict[str, Any],
    artifacts: dict[str, Any],
    *,
    run_id: int = 100,
    tag: str = "v1.2.0-beta.10",
    target_sha: str = "a" * 40,
    current_run_id: int = 200,
) -> None:
    try:
        validate_reusable_release_run(
            run,
            jobs,
            artifacts,
            run_id=run_id,
            tag=tag,
            target_sha=target_sha,
            current_run_id=current_run_id,
        )
    except ReusableRunError:
        return
    raise AssertionError("invalid reusable release run was accepted")


def run_self_test() -> None:
    tag = "v1.2.0-beta.10"
    sha = "a" * 40
    run, jobs, artifacts = fixture(tag=tag, target_sha=sha)
    validate_reusable_release_run(
        run,
        jobs,
        artifacts,
        run_id=100,
        tag=tag,
        target_sha=sha,
        current_run_id=200,
    )

    for field, value in (
        ("path", ".github/workflows/ci.yml"),
        ("event", "workflow_dispatch"),
        ("head_branch", "v1.2.0-beta.9"),
        ("head_sha", "b" * 40),
        ("status", "in_progress"),
    ):
        broken_run, broken_jobs, broken_artifacts = fixture(tag=tag, target_sha=sha)
        broken_run[field] = value
        expect_failure(broken_run, broken_jobs, broken_artifacts, tag=tag, target_sha=sha)

    current_run, current_jobs, current_artifacts = fixture(tag=tag, target_sha=sha)
    expect_failure(
        current_run,
        current_jobs,
        current_artifacts,
        tag=tag,
        target_sha=sha,
        current_run_id=100,
    )

    failed_run, failed_jobs, failed_artifacts = fixture(tag=tag, target_sha=sha)
    failed_jobs["jobs"][0]["conclusion"] = "failure"
    expect_failure(failed_run, failed_jobs, failed_artifacts, tag=tag, target_sha=sha)

    missing_run, missing_jobs, missing_artifacts = fixture(tag=tag, target_sha=sha)
    missing_artifacts["artifacts"].pop()
    expect_failure(missing_run, missing_jobs, missing_artifacts, tag=tag, target_sha=sha)

    expired_run, expired_jobs, expired_artifacts = fixture(tag=tag, target_sha=sha)
    expired_artifacts["artifacts"][0]["expired"] = True
    expect_failure(expired_run, expired_jobs, expired_artifacts, tag=tag, target_sha=sha)

    empty_run, empty_jobs, empty_artifacts = fixture(tag=tag, target_sha=sha)
    empty_artifacts["artifacts"][0]["size_in_bytes"] = 0
    expect_failure(empty_run, empty_jobs, empty_artifacts, tag=tag, target_sha=sha)

    stable_tag = "v1.2.0"
    stable_run, stable_jobs, stable_artifacts = fixture(tag=stable_tag, target_sha=sha)
    assert any(
        artifact["name"] == "windows-msi"
        for artifact in stable_artifacts["artifacts"]
    )
    validate_reusable_release_run(
        stable_run,
        stable_jobs,
        stable_artifacts,
        run_id=100,
        tag=stable_tag,
        target_sha=sha,
        current_run_id=200,
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate provenance before reusing artifacts from an existing Release run."
    )
    parser.add_argument("--run-id", type=int)
    parser.add_argument("--tag")
    parser.add_argument("--target-sha")
    parser.add_argument("--current-run-id", type=int)
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY"))
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        run_self_test()
        print("reusable release run self-test passed")
        return 0

    if (
        args.run_id is None
        or not args.tag
        or not args.target_sha
        or args.current_run_id is None
        or not args.repository
    ):
        parser.error(
            "--run-id, --tag, --target-sha, --current-run-id, and repository are required"
        )

    try:
        parse_tag(args.tag)
        if not re.fullmatch(r"[0-9a-fA-F]{40}", args.target_sha):
            raise ReusableRunError(f"invalid target SHA: {args.target_sha}")
        run = fetch_json(args.repository, f"actions/runs/{args.run_id}")
        jobs = fetch_json(
            args.repository,
            f"actions/runs/{args.run_id}/jobs?filter=latest&per_page=100",
        )
        artifacts = fetch_json(
            args.repository,
            f"actions/runs/{args.run_id}/artifacts?per_page=100",
        )
        validate_reusable_release_run(
            run,
            jobs,
            artifacts,
            run_id=args.run_id,
            tag=args.tag,
            target_sha=args.target_sha.lower(),
            current_run_id=args.current_run_id,
        )
    except (ReusableRunError, ValueError) as error:
        print(f"reusable release run validation failed: {error}", file=sys.stderr)
        return 2

    print(
        "reusable release run accepted: "
        f"run={args.run_id} tag={args.tag} sha={args.target_sha.lower()}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
