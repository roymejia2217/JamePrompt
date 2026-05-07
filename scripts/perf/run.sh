#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPORT_PATH="${JAME_PROMPT_PERF_REPORT_PATH:-$ROOT_DIR/target/perf-report.json}"

export JAME_PROMPT_PERF=1
export JAME_PROMPT_PERF_REPORT_PATH="$REPORT_PATH"

cd "$ROOT_DIR"
cargo run --release -- --perf-smoke "$@"
