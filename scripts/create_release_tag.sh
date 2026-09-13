#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 vMAJOR.MINOR.PATCH[-beta.N]" >&2
  exit 64
fi

tag="$1"
repository_root="$(git rev-parse --show-toplevel)"
cd "$repository_root"

git diff --quiet
git diff --cached --quiet
git fetch origin main --tags

head_commit="$(git rev-parse HEAD)"
main_commit="$(git rev-parse origin/main)"
[ "$head_commit" = "$main_commit" ] || {
  echo "release tags must be created from the current origin/main commit" >&2
  exit 2
}

git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null && {
  echo "release tag already exists: $tag" >&2
  exit 2
}

scripts/verify_change_gate.sh
python3 scripts/validate_release_gate.py \
  --tag "$tag" \
  --main-ref origin/main \
  --preflight \
  --target-ref HEAD

git tag -a "$tag" -m "$tag"
git push origin "$tag"
