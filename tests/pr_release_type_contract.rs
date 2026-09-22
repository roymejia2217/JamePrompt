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
fn pull_request_release_impact_is_machine_readable_and_closed_set() {
    let template = read_file(".github/pull_request_template.md");
    let validator = read_file("scripts/validate_pr_description.py");

    for required in [
        "Release-Type: <none|alpha|beta|stable>",
        "Release-Reason: <explain why this PR does or does not require a release>",
    ] {
        assert!(
            template.contains(required),
            "PR template must expose release metadata field: {}",
            required
        );
    }

    for required in [
        "RELEASE_TYPES = {\"none\", \"alpha\", \"beta\", \"stable\"}",
        "Release-Type:",
        "Release-Reason:",
        "invalid Release-Type",
        "Release-Reason must contain at least 20 characters",
        "release impact must contain exactly Release-Type and Release-Reason",
    ] {
        assert!(
            validator.contains(required),
            "PR validator must enforce release-impact contract: {}",
            required
        );
    }
}

#[test]
fn pull_request_release_type_contract_rejects_placeholder_and_unknown_values() {
    let validator = read_file("scripts/validate_pr_description.py");

    for required in [
        "Release-Type: beta",
        "Release-Type: alpha",
        "Release-Type: stable",
        "Release-Type: none",
        "Release-Type: rc",
        "Release-Type: <none|alpha|beta|stable>",
    ] {
        assert!(
            validator.contains(required),
            "PR validator self-test must exercise release type case: {}",
            required
        );
    }
}

#[test]
fn governance_and_local_pr_creation_reuse_the_same_description_validator() {
    let governance = read_file(".github/workflows/pr-governance.yml");
    let local = read_file("scripts/open_pull_request.sh");

    let command = "python3 scripts/validate_pr_description.py --body-file";
    assert!(
        governance.contains(command),
        "trusted PR governance must execute the canonical body validator"
    );
    assert!(
        local.contains(command),
        "local PR creation must execute the same canonical body validator"
    );
}
