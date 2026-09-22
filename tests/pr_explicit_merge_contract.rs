use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_file(relative: &str) -> String {
    let path = repo_path(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {}", path.display(), error))
        .replace("\r\n", "\n")
}

#[test]
fn pull_request_helper_opens_draft_without_any_merge_side_effect() {
    let helper = read_file("scripts/open_pull_request.sh");

    for required in [
        "gh pr create",
        "--draft",
        "--base main",
        "--head \"$branch\"",
        "python3 scripts/validate_pr_description.py --body-file",
        "scripts/verify_change_gate.sh",
    ] {
        assert!(
            helper.contains(required),
            "PR helper must preserve explicit opening contract: {}",
            required
        );
    }

    for forbidden in [
        "gh pr merge",
        "--auto",
        "--squash",
        "--merge",
        "gh pr ready",
    ] {
        assert!(
            !helper.contains(forbidden),
            "PR helper must not cross the explicit merge boundary: {}",
            forbidden
        );
    }
}

#[test]
fn pull_request_helper_reports_existing_or_created_pr_without_mutating_readiness() {
    let helper = read_file("scripts/open_pull_request.sh");

    for required in [
        "gh pr list --base main --head \"$branch\" --state open",
        "gh pr view \"$existing\"",
        "--json number,url,isDraft,headRefName,baseRefName",
        "pull request prepared:",
    ] {
        assert!(
            helper.contains(required),
            "PR helper must expose review state without merging: {}",
            required
        );
    }
}

#[test]
fn contributor_contract_requires_explicit_review_and_rebase_merge() {
    let contributing = read_file("CONTRIBUTING.md");

    for required in [
        "creates a draft pull request",
        "does not enable auto-merge",
        "required checks",
        "exact head SHA",
        "rebase",
    ] {
        assert!(
            contributing.contains(required),
            "contributor contract must document explicit merge boundary: {}",
            required
        );
    }
}
