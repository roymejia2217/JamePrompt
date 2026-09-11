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

## Bypass and authoritative enforcement

Git permits bypassing local hooks with `git commit --no-verify`. This is a
local-development escape hatch, not an approved way to merge code. The
authoritative control is the `validate-commit-messages` CI job, which runs
Commitlint against only the commits introduced by a pull request. It is
intentionally skipped for non-pull-request events so a push or other event
cannot produce a false commit-range result.

The protected `main` branch requires this CI check, a pull request, one
approval, resolved conversations, and a linear history; its rules also apply
to administrators and prohibit force pushes and deletions. Those remote
settings are managed in GitHub rather than in this repository.
