#!/usr/bin/env python3
"""Probe GitHub for one published release without conflating 404 with API failure."""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


API_VERSION = "2026-03-10"
EXISTING = "existing"
ABSENT = "absent"


class ReleaseProbeError(ValueError):
    """Raised when published release existence cannot be determined safely."""


def classify_status(status: int) -> str:
    if status == 200:
        return EXISTING
    if status == 404:
        return ABSENT
    raise ReleaseProbeError(f"unexpected GitHub Release API status: {status}")


def normalize_release_payload(payload: dict[str, Any]) -> dict[str, Any]:
    if payload.get("draft") is not False:
        raise ReleaseProbeError("draft GitHub Release is not a published release")
    assets = payload.get("assets")
    if not isinstance(assets, list):
        raise ReleaseProbeError("published GitHub Release assets must be a list")

    return {
        "tagName": payload.get("tag_name"),
        "name": payload.get("name"),
        "body": payload.get("body"),
        "isDraft": payload.get("draft"),
        "isPrerelease": payload.get("prerelease"),
        "isImmutable": payload.get("immutable"),
        "publishedAt": payload.get("published_at"),
        "assets": assets,
    }


def release_url(api_url: str, repository: str, tag: str) -> str:
    parts = repository.split("/")
    if len(parts) != 2 or not all(parts):
        raise ReleaseProbeError("repository must use OWNER/REPO format")
    owner, name = parts
    return (
        f"{api_url.rstrip('/')}/repos/{quote(owner, safe='')}/"
        f"{quote(name, safe='')}/releases/tags/{quote(tag, safe='')}"
    )


def fetch_published_release(
    api_url: str,
    repository: str,
    tag: str,
    token: str,
) -> tuple[str, dict[str, Any] | None]:
    if not token:
        raise ReleaseProbeError("GitHub API token is required")

    request = Request(
        release_url(api_url, repository, tag),
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "User-Agent": "JamePrompt-release-probe",
            "X-GitHub-Api-Version": API_VERSION,
        },
        method="GET",
    )

    try:
        with urlopen(request, timeout=30) as response:
            state = classify_status(response.status)
            raw = response.read().decode("utf-8")
    except HTTPError as error:
        state = classify_status(error.code)
        if state == ABSENT:
            return ABSENT, None
        raise AssertionError("unreachable HTTP status classification") from error
    except URLError as error:
        raise ReleaseProbeError(
            f"GitHub Release API request failed: {error.reason}"
        ) from error
    except OSError as error:
        raise ReleaseProbeError(
            f"GitHub Release API request failed: {error}"
        ) from error

    if state != EXISTING:
        raise ReleaseProbeError(f"unexpected successful probe state: {state}")

    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as error:
        raise ReleaseProbeError("GitHub Release API returned invalid JSON") from error
    if not isinstance(payload, dict):
        raise ReleaseProbeError("GitHub Release API response root must be an object")

    return EXISTING, normalize_release_payload(payload)


def self_test() -> None:
    assert classify_status(200) == EXISTING
    assert classify_status(404) == ABSENT

    for status in (0, 401, 403, 429, 500, 503):
        try:
            classify_status(status)
        except ReleaseProbeError as error:
            assert "unexpected GitHub Release API status" in str(error)
        else:
            raise AssertionError(f"unsafe API status was accepted: {status}")

    payload = {
        "tag_name": "v1.2.0-beta.10",
        "name": "v1.2.0-beta.10",
        "body": "notes",
        "draft": False,
        "prerelease": True,
        "immutable": False,
        "published_at": "2026-09-22T12:00:00Z",
        "assets": [
            {
                "name": "artifact.bin",
                "state": "uploaded",
                "size": 4,
                "digest": "sha256:" + ("0" * 64),
            }
        ],
    }
    normalized = normalize_release_payload(payload)
    assert normalized["tagName"] == payload["tag_name"]
    assert normalized["isDraft"] is False
    assert normalized["isPrerelease"] is True
    assert normalized["assets"] == payload["assets"]

    draft = dict(payload)
    draft["draft"] = True
    try:
        normalize_release_payload(draft)
    except ReleaseProbeError as error:
        assert "draft GitHub Release is not a published release" in str(error)
    else:
        raise AssertionError("draft GitHub Release was accepted as published")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Probe a published GitHub Release by exact tag."
    )
    parser.add_argument("--repository")
    parser.add_argument("--tag")
    parser.add_argument("--write-status", type=Path)
    parser.add_argument("--write-json", type=Path)
    parser.add_argument(
        "--api-url",
        default=os.environ.get("GITHUB_API_URL", "https://api.github.com"),
    )
    parser.add_argument("--token-env", default="GH_TOKEN")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("GitHub Release existence probe self-test passed")
        return 0

    if (
        not args.repository
        or not args.tag
        or args.write_status is None
        or args.write_json is None
    ):
        parser.error(
            "--repository, --tag, --write-status, and --write-json are required"
        )

    try:
        state, payload = fetch_published_release(
            args.api_url,
            args.repository,
            args.tag,
            os.environ.get(args.token_env, ""),
        )
        if state == EXISTING:
            assert payload is not None
            args.write_json.write_text(
                json.dumps(payload, sort_keys=True) + "\n",
                encoding="utf-8",
            )
        else:
            if args.write_json.exists():
                args.write_json.unlink()
        args.write_status.write_text(state + "\n", encoding="utf-8")
    except (OSError, ReleaseProbeError) as error:
        print(f"release existence probe error: {error}", file=sys.stderr)
        return 2

    print(f"GitHub Release existence probe: {state}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
