#!/usr/bin/env bash
set -euo pipefail

repository_root="$(git rev-parse --show-toplevel)"
cd "$repository_root"

npm run test:commitlint
python3 scripts/validate_pr_description.py --self-test
python3 scripts/prepare_release_version.py --self-test
python3 scripts/validate_release_gate.py --self-test
python3 scripts/validate_release_metadata.py --self-test
python3 scripts/stage_release_assets.py --self-test
python3 scripts/validate_main_ci_evidence.py --self-test
python3 scripts/validate_reusable_release_run.py --self-test
python3 scripts/validate_ci_release_parity.py --self-test
python3 scripts/validate_ci_release_parity.py
cargo fmt --all -- --check
cargo test --locked --all-targets
