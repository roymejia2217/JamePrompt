#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 vMAJOR.MINOR.PATCH[-alpha.N|-beta.N]" >&2
  exit 64
fi

tag="$1"
repository_root="$(git rev-parse --show-toplevel)"
cd "$repository_root"

tag_message_file="$(mktemp)"
tag_created=false
tag_pushed=false

cleanup_local_tag() {
  rm -f "$tag_message_file"
  if [ "$tag_created" = true ] && [ "$tag_pushed" = false ]; then
    git tag -d "$tag" >/dev/null
  fi
}
trap cleanup_local_tag EXIT

assert_release_state_current() {
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
}

git diff --quiet
git diff --cached --quiet
git fetch origin main --tags
assert_release_state_current

python3 scripts/validate_release_metadata.py \
  --tag "$tag" \
  --changelog CHANGELOG.md \
  --write-tag-message "$tag_message_file"
python3 scripts/validate_main_ci_evidence.py --sha "$head_commit"
scripts/verify_change_gate.sh

git fetch origin main --tags
assert_release_state_current

python3 scripts/validate_release_gate.py \
  --tag "$tag" \
  --main-ref origin/main \
  --preflight \
  --target-ref HEAD

git tag -a "$tag" -F "$tag_message_file"
tag_created=true

python3 scripts/validate_release_gate.py \
  --tag "$tag" \
  --main-ref origin/main

git push origin "$tag"
tag_pushed=true
