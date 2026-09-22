# Contributing to JamePrompt

## Repository metadata standards

JamePrompt treats repository metadata as executable acceptance input, not as prose-only policy.
The authoritative standards and repository profiles are:

- **Commits:** [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) through pinned Commitlint.
- **Pull requests:** GitHub pull request templates plus the checked-in JamePrompt body schema.
- **Versions and tags:** [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).
- **Release notes:** [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) categories and ordering.

Local hooks provide fast feedback. GitHub required checks remain the authoritative merge boundary.

## Commit messages

Commit messages use the Conventional Commits structure:

```text
type(scope): subject

A meaningful body explaining the change.

Optional-Trailer: value
```

The scope is optional. The configured conventional type vocabulary is `build`, `chore`,
`ci`, `docs`, `feat`, `fix`, `perf`, `refactor`, `revert`, `style`, and
`test`. Breaking changes use `!` before the colon and/or the standard
`BREAKING CHANGE:` footer.

JamePrompt deliberately uses a stricter Conventional Commits profile: the body is required,
must be separated from the header by a blank line, and must contain at least 20 characters.
Trailers remain optional and, when present, must be separated from the body by a blank line.
PR titles use the same Conventional Commit header grammar but do not require a body.

Examples:

```text
feat(ui): add prompt export

Document the prompt export behavior for the supported user workflow.
```

```text
fix(hotkeys): restore portal registration

Keep the existing portal registration active during window recreation.

Refs: #123
```

## Pull request body

GitHub automatically supplies `.github/pull_request_template.md`. The body must contain
exactly these level-two sections, in this order:

```text
## Summary
## Motivation
## Changes
## Verification
## Risk and rollback
## Release impact
```

All six sections must contain non-placeholder content. Unknown, duplicated, missing, or
reordered level-two sections are rejected by `scripts/validate_pr_description.py`.
The PR title is independently checked against the Conventional Commit header profile.

Open a reviewed pull request through the checked-in path:

```bash
scripts/open_pull_request.sh \
  --title 'type(scope): subject' \
  --body-file path/to/pull-request.md
```

## Release metadata

`CHANGELOG.md` is the source of truth for GitHub Release titles and bodies. It retains an
`[Unreleased]` section and uses the Keep a Changelog change categories in this exact order:

```text
Added
Changed
Deprecated
Removed
Fixed
Security
```

A release entry must use an ISO date and a SemVer version heading, for example:

```markdown
## [1.2.0-beta.10] - 2026-09-21
```

Every category must contain at least one Markdown bullet, and at least one category must
describe a notable change rather than `None.`.

The supported release profile is:

- `vMAJOR.MINOR.PATCH-alpha.N` — alpha prerelease.
- `vMAJOR.MINOR.PATCH-beta.N` — beta prerelease.
- `vMAJOR.MINOR.PATCH` — stable release.

Other prerelease labels such as `rc.N` are rejected by the repository profile. Stable
release creation retains the existing requirement that a beta for the same base version
already exists.

The GitHub Release title is exactly the tag. The GitHub Release body is rendered from the
matching validated `CHANGELOG.md` entry. If a GitHub Release already exists, its tag,
title, prerelease flag, and body must match the changelog-derived metadata before artifacts
can be replaced.

Create a tag only after the changelog entry has been merged to protected `main`:

```bash
scripts/create_release_tag.sh v1.2.0-alpha.1
scripts/create_release_tag.sh v1.2.0-beta.10
scripts/create_release_tag.sh v1.2.0
```

## Local setup

Install the pinned development dependencies from the repository root:

```bash
npm ci
```

The `prepare` script installs the versioned Husky hooks. The `commit-msg` hook executes
the local Commitlint binary; the `pre-push` hook runs the repository change gate.

Run focused metadata harnesses with:

```bash
npm run test:commitlint
npm run test:pr-governance
npm run test:release-metadata
npm run test:release-gate
```

Run the complete local acceptance gate with:

```bash
npm run verify:change
```

## Delivery gates

| Boundary | Enforced contract |
| --- | --- |
| Commit | Husky `commit-msg` and protected CI enforce Conventional Commits plus the required body profile. |
| Push | Husky `pre-push` runs the fail-closed repository change gate. |
| Pull request | GitHub validates the conventional title and exact six-section body from the trusted base revision. |
| Merge | Protected `main` accepts only PRs whose required checks pass; linear rebase history is preserved. |
| Tag | Tag creation requires current protected `main`, successful post-merge CI, valid changelog metadata, SemVer alpha/beta/stable profile, and an annotated tag. |
| Release | Release provenance, package/lifecycle gates, remote tag identity, changelog-derived metadata, artifact set, and publication concurrency must all pass. |

CI cancels only superseded pull-request runs. Post-merge pushes to `main` retain independent
validation runs. Protected jobs use explicit fail-closed timeouts.

## Workflow supply-chain updates

External GitHub Actions in `.github/workflows` must use full 40-character commit SHAs.
Keep the human-readable release or channel as an inline comment, but never replace the
immutable SHA with a moving tag or branch.

Release helper binaries follow the same rule. `linuxdeploy` and `appimagetool` are
resolved from exact release asset IDs and their bytes must match checked-in SHA-256 values
before execution.

## Bypass and authoritative enforcement

Local Git hooks can technically be bypassed with Git's `--no-verify`; that is not an
approved merge path. Required GitHub checks re-run the contracts against PR commits and
metadata from the trusted base revision.

Protected `main` requires the repository's configured required checks and linear history.
Remote branch settings remain GitHub configuration and must not be inferred from this file.
