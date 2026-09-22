#!/usr/bin/env python3
"""Require successful post-merge CI evidence for the exact protected-main commit."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import urllib.parse
from typing import Any

from github_api import (
    GitHubApiError,
    REPOSITORY_PATTERN,
    github_token,
    repository_url,
    request_json,
)


DEFAULT_BRANCH = "main"
DEFAULT_WORKFLOW_NAME = "CI"
DEFAULT_WORKFLOW_PATH = ".github/workflows/ci.yml"
class EvidenceError(ValueError):
    """Raised when protected-main CI evidence is absent, stale, or unsuccessful."""


def run_git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        message = result.stderr.strip() or result.stdout.strip()
        raise EvidenceError(f"git {' '.join(args)} failed: {message}")
    return result.stdout.strip()


def repository_from_remote(remote_url: str) -> str:
    value = remote_url.strip()
    prefixes = (
        "git@github.com:",
        "ssh://git@github.com/",
        "https://github.com/",
        "http://github.com/",
    )
    for prefix in prefixes:
        if value.startswith(prefix):
            repository = value[len(prefix) :]
            if repository.endswith(".git"):
                repository = repository[:-4]
            if REPOSITORY_PATTERN.fullmatch(repository):
                return repository
            break
    raise EvidenceError(f"origin is not a supported GitHub repository URL: {remote_url}")


def infer_repository() -> str:
    return repository_from_remote(run_git("remote", "get-url", "origin"))


def build_api_url(repository: str, sha: str) -> str:
    if not REPOSITORY_PATTERN.fullmatch(repository):
        raise EvidenceError(f"invalid GitHub repository identifier: {repository}")
    if not re.fullmatch(r"[0-9a-fA-F]{40}", sha):
        raise EvidenceError(f"invalid commit SHA: {sha}")
    query = urllib.parse.urlencode(
        {
            "head_sha": sha.lower(),
            "event": "push",
            "status": "success",
            "per_page": 100,
        }
    )
    try:
        return repository_url(repository, f"actions/runs?{query}")
    except GitHubApiError as error:
        raise EvidenceError(str(error)) from error


def fetch_workflow_runs(repository: str, sha: str) -> dict[str, Any]:
    url = build_api_url(repository, sha)
    try:
        response = request_json(
            url,
            user_agent="JamePrompt-release-evidence",
            token=github_token(required=False),
        )
    except GitHubApiError as error:
        raise EvidenceError(
            f"unable to query GitHub Actions evidence: {error}"
        ) from error

    if response.payload is None:
        raise EvidenceError("GitHub Actions response body is missing")
    return response.payload


def validate_evidence(
    payload: dict[str, Any],
    sha: str,
    *,
    branch: str = DEFAULT_BRANCH,
    workflow_name: str = DEFAULT_WORKFLOW_NAME,
    workflow_path: str = DEFAULT_WORKFLOW_PATH,
) -> dict[str, Any]:
    runs = payload.get("workflow_runs")
    if not isinstance(runs, list):
        raise EvidenceError("GitHub Actions response is missing workflow_runs")

    matches: list[dict[str, Any]] = []
    for item in runs:
        if not isinstance(item, dict):
            continue
        path = item.get("path")
        if (
            item.get("name") == workflow_name
            and item.get("head_sha") == sha
            and item.get("head_branch") == branch
            and item.get("event") == "push"
            and item.get("status") == "completed"
            and item.get("conclusion") == "success"
            and isinstance(path, str)
            and path.startswith(f"{workflow_path}@")
        ):
            matches.append(item)

    if not matches:
        observed = [
            {
                "name": item.get("name"),
                "head_sha": item.get("head_sha"),
                "head_branch": item.get("head_branch"),
                "event": item.get("event"),
                "status": item.get("status"),
                "conclusion": item.get("conclusion"),
                "path": item.get("path"),
            }
            for item in runs
            if isinstance(item, dict)
        ]
        raise EvidenceError(
            "no successful completed post-merge main CI run matches the exact target SHA; "
            f"observed={observed}"
        )

    return max(matches, key=lambda item: int(item.get("id", 0)))


def run_self_test() -> None:
    sha = "a" * 40
    valid_run = {
        "id": 101,
        "run_number": 42,
        "name": "CI",
        "path": ".github/workflows/ci.yml@refs/heads/main",
        "head_sha": sha,
        "head_branch": "main",
        "event": "push",
        "status": "completed",
        "conclusion": "success",
    }
    payload = {"workflow_runs": [valid_run]}
    accepted = validate_evidence(payload, sha)
    assert accepted["id"] == 101

    failure_cases = (
        ("head_sha", "b" * 40),
        ("head_branch", "feature"),
        ("event", "pull_request"),
        ("status", "in_progress"),
        ("conclusion", "failure"),
        ("path", ".github/workflows/other.yml@refs/heads/main"),
        ("name", "Other"),
    )
    for field, value in failure_cases:
        broken = dict(valid_run)
        broken[field] = value
        try:
            validate_evidence({"workflow_runs": [broken]}, sha)
        except EvidenceError:
            pass
        else:
            raise AssertionError(f"invalid CI evidence was accepted: {field}={value}")

    assert repository_from_remote("git@github.com:owner/repo.git") == "owner/repo"
    assert repository_from_remote("https://github.com/owner/repo.git") == "owner/repo"
    expected_url = build_api_url("owner/repo", sha)
    assert "actions/runs" in expected_url
    assert "head_sha=" in expected_url
    assert "event=push" in expected_url
    assert "status=success" in expected_url


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate successful post-merge CI evidence for protected main."
    )
    parser.add_argument("--sha", help="Exact protected-main commit SHA to validate.")
    parser.add_argument(
        "--repository",
        help="GitHub owner/repository. Defaults to the origin remote.",
    )
    parser.add_argument("--branch", default=DEFAULT_BRANCH)
    parser.add_argument("--workflow-name", default=DEFAULT_WORKFLOW_NAME)
    parser.add_argument("--workflow-path", default=DEFAULT_WORKFLOW_PATH)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        run_self_test()
        print("main CI evidence self-test passed")
        return 0
    if not args.sha:
        parser.error("--sha is required unless --self-test is used")

    try:
        repository = args.repository or infer_repository()
        payload = fetch_workflow_runs(repository, args.sha)
        run = validate_evidence(
            payload,
            args.sha.lower(),
            branch=args.branch,
            workflow_name=args.workflow_name,
            workflow_path=args.workflow_path,
        )
    except EvidenceError as error:
        print(f"main CI evidence validation failed: {error}", file=sys.stderr)
        return 2

    print(
        "main CI evidence accepted: "
        f"{repository} {args.sha.lower()} "
        f"run={run.get('id')} number={run.get('run_number')}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
