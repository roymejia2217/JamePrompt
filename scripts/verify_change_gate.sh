#!/usr/bin/env bash
set -euo pipefail

repository_root="$(git rev-parse --show-toplevel)"
cd "$repository_root"

run_local_gate() {
    npm run test:commitlint
    python3 scripts/github_api.py --self-test
    python3 scripts/validate_pr_description.py --self-test
    python3 scripts/prepare_release_version.py --self-test
    python3 scripts/validate_release_gate.py --self-test
    python3 scripts/validate_release_metadata.py --self-test
    python3 scripts/validate_existing_release_assets.py --self-test
    python3 scripts/probe_github_release.py --self-test
    python3 scripts/stage_release_assets.py --self-test
    python3 scripts/validate_main_ci_evidence.py --self-test
    python3 scripts/validate_reusable_release_run.py --self-test
    python3 scripts/validate_ci_release_parity.py --self-test
    python3 scripts/validate_ci_release_parity.py
    python3 scripts/validate_workflow_yaml.py --self-test
    python3 scripts/validate_workflow_yaml.py .github/workflows
    cargo fmt --all -- --check
    uv lock --check
    uv venv --python /usr/bin/python3 --system-site-packages --allow-existing
    uv sync --locked
    uv run --locked pytest tests/desktop/test_harness_contract.py tests/desktop/test_adapters.py -q
    cargo check --locked --bin jame-prompt
}

run_self_test() {
    local gate_body
    gate_body="$(sed -n '/^run_local_gate() {/,/^}$/p' "${BASH_SOURCE[0]}")"
    local forbidden_token
    for forbidden_token in \
        docker \
        apt-get \
        'dpkg --root' \
        JAME_PROMPT_NATIVE_GNOME \
        validate_deb_install_lifecycle.sh \
        validate_arch_install_lifecycle.sh \
        '--features desktop-integration-tests'; do
        if grep -Fq -- "$forbidden_token" <<<"$gate_body"; then
            printf 'Local gate self-test failed: forbidden remote-only command: %s\n' "$forbidden_token" >&2
            return 1
        fi
    done
    printf '%s\n' 'Local gate self-test passed'
}

if [[ "${1:-}" == "--self-test" ]]; then
    run_self_test
    exit 0
fi

run_local_gate
printf '%s\n' 'Remote-only evidence remains required: native GNOME, packaging, and install/purge CI jobs.'
