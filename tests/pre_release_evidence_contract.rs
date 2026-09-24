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
fn release_evidence_validator_requires_successful_post_merge_main_ci() {
    let validator = read_file("scripts/validate_main_ci_evidence.py");
    let transport = read_file("scripts/github_api.py");

    assert!(
        transport.contains("API_ROOT = \"https://api.github.com\""),
        "shared GitHub REST transport must own the API root"
    );

    for required in [
        "actions/runs",
        "head_sha",
        "event",
        "push",
        "status",
        "success",
        "head_branch",
        "main",
        ".github/workflows/ci.yml",
        "completed",
        "conclusion",
        "--self-test",
        "main CI evidence accepted",
    ] {
        assert!(
            validator.contains(required),
            "main CI evidence validator must include contract: {}",
            required
        );
    }

    for forbidden in [
        "except Exception:",
        "return True",
        "|| true",
        "failure accepted",
    ] {
        assert!(
            !validator.contains(forbidden),
            "main CI evidence validator must remain fail-closed: {}",
            forbidden
        );
    }
}

#[test]
fn release_tag_creation_requires_main_ci_evidence_before_tagging() {
    let tagger = read_file("scripts/create_release_tag.sh");

    let evidence = tagger
        .find("python3 scripts/validate_main_ci_evidence.py")
        .expect("release tag creation must validate post-merge main CI evidence");
    let change_gate = tagger
        .find("scripts/verify_change_gate.sh")
        .expect("release tag creation must retain the local change gate");
    let tag = tagger
        .find("git tag -a")
        .expect("release tag creation must create an annotated tag");

    assert!(
        evidence < change_gate && change_gate < tag,
        "main CI evidence and local change gate must both pass before tag creation"
    );
    assert!(
        tagger.contains("--sha \"$head_commit\""),
        "release evidence must target the exact origin/main commit being tagged"
    );
}

#[test]
fn protected_and_local_gates_self_test_release_evidence_contract() {
    let ci = read_file(".github/workflows/ci.yml");
    let local_gate = read_file("scripts/verify_change_gate.sh");

    assert!(
        ci.contains("python3 scripts/validate_main_ci_evidence.py --self-test"),
        "protected CI must self-test the main CI evidence validator"
    );
    assert!(
        local_gate.contains("python3 scripts/validate_main_ci_evidence.py --self-test"),
        "local release change gate must self-test the main CI evidence validator"
    );
    assert!(
        local_gate.contains("python3 scripts/validate_ci_release_parity.py"),
        "local release change gate must enforce the same CI/Release parity contract"
    );
}


#[test]
fn release_evidence_matches_the_workflow_path_shape_returned_by_github_actions() {
    let validator = read_file("scripts/validate_main_ci_evidence.py");

    assert!(
        validator.contains("path == workflow_path"),
        "main CI evidence must match the exact workflow path returned by GitHub Actions"
    );
    assert!(
        !validator.contains("path.startswith(f\"{workflow_path}@\")"),
        "main CI evidence must not require a synthetic @ref suffix absent from workflow runs"
    );
}
