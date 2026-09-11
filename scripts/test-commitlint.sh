#!/usr/bin/env bash
set -euo pipefail

commitlint() {
  npm exec --no -- commitlint --verbose
}

assert_rejected() {
  local name="$1"
  local message="$2"

  if printf '%s\n' "$message" | commitlint; then
    echo "Expected Commitlint to reject: $name" >&2
    exit 1
  fi
}

valid_message='feat(ui): add prompt export'
valid_body_and_footer=$'fix(ui): restore tray icon\n\nKeep the existing tray lifecycle intact.\n\nRefs: #123'

printf '%s\n' "$valid_message" | commitlint
printf '%s\n' "$valid_body_and_footer" | commitlint

assert_rejected 'missing type' 'Update prompt export'
assert_rejected 'unknown type' 'unknown(ui): add prompt export'
assert_rejected 'bracket scope' 'fix[ui]: restore tray icon'
assert_rejected 'uppercase subject' 'feat(ui): Add prompt export'
assert_rejected 'subject with period' 'feat(ui): add prompt export.'
assert_rejected 'header over 100 characters' 'feat(ui): add a prompt export flow with a header that is intentionally longer than one hundred characters to verify the limit'
assert_rejected 'body without leading blank' $'fix(ui): restore tray icon\nKeep the existing tray lifecycle intact.'
assert_rejected 'footer without leading blank' $'fix(ui): restore tray icon\n\nKeep the existing tray lifecycle intact.\nRefs: #123'
