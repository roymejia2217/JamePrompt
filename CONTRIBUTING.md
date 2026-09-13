# Contributing to JamePrompt

## Commit messages

JamePrompt uses the maintained Conventional Commits implementation provided by
`@commitlint/cli` and `@commitlint/config-conventional`. Commit messages must
use the standard format:

```text
type(scope): subject
```

The scope is optional. The conventional configuration supplies the closed
standard type vocabulary (`build`, `chore`, `ci`, `docs`, `feat`, `fix`,
`perf`, `refactor`, `revert`, `style`, and `test`) and the associated
Conventional Commits rules. Breaking changes use `!` before the colon and/or
the standard `BREAKING CHANGE:` footer.

Examples:

```text
feat(ui): add prompt export
fix(hotkeys): restore portal registration
refactor(db)!: replace the repository interface
```

The body and footer, when present, must be separated from the subject by a
blank line. The same rule applies between the body and footer. These are
configured as errors, not warnings.

## Local setup

Install the pinned development dependencies from the repository root:

```bash
npm ci
```

The `prepare` script installs the versioned Husky `commit-msg` hook. The hook
executes the local Commitlint binary, so it does not depend on a globally
installed Node package.

Run the acceptance checks with:

```bash
npm run test:commitlint
```

To validate the current commit range manually:

```bash
npm run lint:commit
```

## Delivery gates

The repository enforces delivery controls at each boundary, using Git hooks for
fast local feedback and GitHub checks as the authoritative merge protection:

| Boundary | Enforced contract |
| --- | --- |
| Commit | The Husky `commit-msg` hook runs Commitlint. |
| Push | The Husky `pre-push` hook runs `npm run verify:change`: message, policy-script, formatting, and locked test checks. |
| Pull request | The template requires **Summary**, **Verification**, and **Release impact**. GitHub validates that structure and the Conventional Commit PR title from the trusted `main` revision. |
| Merge | Protected `main` accepts a PR only after its required GitHub checks pass; GitHub native auto-merge uses rebase. |
| Release | `scripts/create_release_tag.sh` creates an annotated tag only from the current `origin/main`; the workflow accepts only `vMAJOR.MINOR.PATCH` or `vMAJOR.MINOR.PATCH-beta.N`. A stable tag additionally requires a beta for the same version. |

Open a reviewed, auto-merge-enabled pull request through the checked-in path:

```bash
scripts/open_pull_request.sh \
  --title 'type(scope): subject' \
  --body-file path/to/pull-request.md
```

Create a beta or stable release tag only after the pull request has merged:

```bash
scripts/create_release_tag.sh v1.2.0-beta.10
```

Push-to-PR creation is deliberately not automated in this repository yet. A
workflow that creates a PR and triggers its checks needs a GitHub App
installation token (or a user-managed PAT); `GITHUB_TOKEN`-created PRs do not
run their `pull_request` workflows normally. The checked-in command above is
the deterministic path until that external credential is installed.

## Bypass and authoritative enforcement

Git permits bypassing local hooks with `git commit --no-verify`. This is a
local-development escape hatch, not an approved way to merge code. The
authoritative control is the `validate-commit-messages` CI job, which runs
Commitlint against only the commits introduced by a pull request. It is
intentionally skipped for non-pull-request events so a push or other event
cannot produce a false commit-range result.

The protected `main` branch requires a pull request, the required CI checks,
resolved conversations, and a linear history; its rules also apply to
administrators and prohibit force pushes and deletions. Review count is a
GitHub repository setting and must not be claimed here unless it is configured
there. Those remote settings are managed in GitHub rather than in this
repository.
