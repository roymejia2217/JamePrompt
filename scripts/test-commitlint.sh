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

valid_message=$'feat(ui): add prompt export\n\nDocument the prompt export behavior for the supported user workflow.'
valid_body_and_footer=$'fix(ui): restore tray icon\n\nKeep the existing tray lifecycle intact across window recreation.\n\nRefs: #123'

printf '%s\n' "$valid_message" | commitlint
printf '%s\n' "$valid_body_and_footer" | commitlint
printf '%s\n' 'feat(ui): add prompt export' | npm exec --no -- commitlint --config commitlint.title.config.cjs --verbose

assert_rejected 'missing body' 'feat(ui): add prompt export'
assert_rejected 'missing type' $'Update prompt export\n\nDescribe the prompt export behavior for the supported user workflow.'
assert_rejected 'unknown type' $'unknown(ui): add prompt export\n\nDescribe the prompt export behavior for the supported user workflow.'
assert_rejected 'bracket scope' $'fix[ui]: restore tray icon\n\nKeep the existing tray lifecycle intact across window recreation.'
assert_rejected 'uppercase subject' $'feat(ui): Add prompt export\n\nDescribe the prompt export behavior for the supported user workflow.'
assert_rejected 'subject with period' $'feat(ui): add prompt export.\n\nDescribe the prompt export behavior for the supported user workflow.'
assert_rejected 'header over 100 characters' $'feat(ui): add a prompt export flow with a header that is intentionally longer than one hundred characters to verify the limit\n\nDescribe the prompt export behavior for the supported user workflow.'
assert_rejected 'body without leading blank' $'fix(ui): restore tray icon\nKeep the existing tray lifecycle intact across window recreation.'
assert_rejected 'footer without leading blank' $'fix(ui): restore tray icon\n\nKeep the existing tray lifecycle intact across window recreation.\nRefs: #123'
