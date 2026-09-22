#!/usr/bin/env python3
"""Shared fail-closed transport for GitHub REST API consumers."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


API_ROOT = "https://api.github.com"
API_VERSION = "2026-03-10"
DEFAULT_TIMEOUT_SECONDS = 30
REPOSITORY_PATTERN = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")


class GitHubApiError(ValueError):
    """Raised when GitHub REST transport cannot establish a trusted response."""


@dataclass(frozen=True)
class GitHubJsonResponse:
    status: int
    payload: dict[str, Any] | None


def validate_repository(repository: str) -> str:
    if not REPOSITORY_PATTERN.fullmatch(repository):
        raise GitHubApiError(f"invalid GitHub repository identifier: {repository}")
    return repository


def repository_url(
    repository: str,
    path: str,
    *,
    api_root: str = API_ROOT,
) -> str:
    validate_repository(repository)
    normalized_path = path.lstrip("/")
    if not normalized_path:
        raise GitHubApiError("GitHub API repository path must not be empty")
    return f"{api_root.rstrip('/')}/repos/{repository}/{normalized_path}"


def github_token(
    *,
    required: bool,
    env_name: str | None = None,
) -> str | None:
    if env_name is not None:
        token = os.environ.get(env_name)
    else:
        token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")

    if required and not token:
        source = env_name or "GH_TOKEN or GITHUB_TOKEN"
        raise GitHubApiError(f"{source} is required")
    return token or None


def api_headers(user_agent: str, token: str | None = None) -> dict[str, str]:
    if not user_agent.strip():
        raise GitHubApiError("GitHub API user agent must not be blank")

    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": user_agent,
        "X-GitHub-Api-Version": API_VERSION,
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def validate_status(status: int, accepted_statuses: tuple[int, ...]) -> int:
    if not accepted_statuses:
        raise GitHubApiError("accepted GitHub API status set must not be empty")
    if status not in accepted_statuses:
        raise GitHubApiError(f"unexpected GitHub API status: {status}")
    return status


def decode_json_object(raw: str) -> dict[str, Any]:
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as error:
        raise GitHubApiError("GitHub API returned invalid JSON") from error
    if not isinstance(payload, dict):
        raise GitHubApiError("GitHub API response root must be an object")
    return payload


def request_json(
    url: str,
    *,
    user_agent: str,
    token: str | None = None,
    accepted_statuses: tuple[int, ...] = (200,),
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> GitHubJsonResponse:
    request = Request(
        url,
        headers=api_headers(user_agent, token),
        method="GET",
    )

    try:
        with urlopen(request, timeout=timeout) as response:
            status = validate_status(response.status, accepted_statuses)
            raw = response.read().decode("utf-8")
    except HTTPError as error:
        try:
            status = validate_status(error.code, accepted_statuses)
        except GitHubApiError as status_error:
            raise status_error from error
        return GitHubJsonResponse(status=status, payload=None)
    except (URLError, TimeoutError, OSError) as error:
        raise GitHubApiError(f"GitHub API request failed: {error}") from error
    except UnicodeDecodeError as error:
        raise GitHubApiError("GitHub API response was not valid UTF-8") from error

    if not raw:
        return GitHubJsonResponse(status=status, payload=None)
    return GitHubJsonResponse(status=status, payload=decode_json_object(raw))


def self_test() -> None:
    assert validate_repository("owner/repo") == "owner/repo"
    try:
        validate_repository("owner")
    except GitHubApiError:
        pass
    else:
        raise AssertionError("invalid repository identifier was accepted")

    url = repository_url("owner/repo", "actions/runs?per_page=1")
    assert url == "https://api.github.com/repos/owner/repo/actions/runs?per_page=1"

    headers = api_headers("JamePrompt-self-test", "token")
    assert headers["Accept"] == "application/vnd.github+json"
    assert headers["X-GitHub-Api-Version"] == API_VERSION
    assert headers["Authorization"] == "Bearer token"

    assert validate_status(200, (200, 404)) == 200
    assert validate_status(404, (200, 404)) == 404
    try:
        validate_status(403, (200, 404))
    except GitHubApiError:
        pass
    else:
        raise AssertionError("unsafe GitHub API status was accepted")

    assert decode_json_object('{"ok": true}') == {"ok": True}
    for invalid in ("not-json", "[]"):
        try:
            decode_json_object(invalid)
        except GitHubApiError:
            pass
        else:
            raise AssertionError(f"invalid GitHub API JSON was accepted: {invalid}")

    original_gh = os.environ.get("GH_TOKEN")
    original_github = os.environ.get("GITHUB_TOKEN")
    try:
        os.environ["GH_TOKEN"] = "gh-token"
        os.environ["GITHUB_TOKEN"] = "github-token"
        assert github_token(required=True) == "gh-token"
        assert github_token(required=True, env_name="GITHUB_TOKEN") == "github-token"

        os.environ.pop("GH_TOKEN", None)
        os.environ.pop("GITHUB_TOKEN", None)
        assert github_token(required=False) is None
        try:
            github_token(required=True)
        except GitHubApiError:
            pass
        else:
            raise AssertionError("missing required GitHub API token was accepted")
    finally:
        if original_gh is None:
            os.environ.pop("GH_TOKEN", None)
        else:
            os.environ["GH_TOKEN"] = original_gh
        if original_github is None:
            os.environ.pop("GITHUB_TOKEN", None)
        else:
            os.environ["GITHUB_TOKEN"] = original_github


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Self-test the shared JamePrompt GitHub REST transport."
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if not args.self_test:
        parser.error("--self-test is required")

    try:
        self_test()
    except GitHubApiError as error:
        print(f"GitHub REST transport self-test failed: {error}", file=sys.stderr)
        return 2

    print("GitHub REST transport self-test passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
