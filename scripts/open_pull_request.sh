#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --title <conventional-title> --body-file <markdown-file>" >&2
  exit 64
}

title=""
body_file=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --title)
      title="${2:-}"
      shift 2
      ;;
    --body-file)
      body_file="${2:-}"
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done

[ -n "$title" ] && [ -n "$body_file" ] && [ -f "$body_file" ] || usage

repository_root="$(git rev-parse --show-toplevel)"
cd "$repository_root"
branch="$(git branch --show-current)"
[ "$branch" != "main" ] || {
  echo "open a pull request from a work branch, never main" >&2
  exit 2
}

printf '%s\n' "$title" | npm exec --no -- commitlint --verbose
python3 scripts/validate_pr_description.py --body-file "$body_file"
scripts/verify_change_gate.sh

existing="$(gh pr list --base main --head "$branch" --state open --json number --jq '.[0].number')"
if [ -z "$existing" ]; then
  gh pr create --base main --head "$branch" --title "$title" --body-file "$body_file"
  existing="$(gh pr list --base main --head "$branch" --state open --json number --jq '.[0].number')"
fi

gh pr merge "$existing" --auto --rebase
