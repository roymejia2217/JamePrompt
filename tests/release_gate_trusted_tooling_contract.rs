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

fn release_gate_job(workflow: &str) -> &str {
    workflow
        .split("\n  release-gate:\n")
        .nth(1)
        .expect("release workflow must define release-gate job")
        .split("\n  deb:\n")
        .next()
        .expect("release-gate job must precede deb job")
}

#[test]
fn release_gate_executes_only_workflow_identity_tooling() {
    let workflow = read_file(".github/workflows/release.yml");
    let gate = release_gate_job(&workflow);

    for required in [
        "- name: Checkout trusted release gate tooling",
        "ref: ${{ github.workflow_sha }}",
        "fetch-depth: 0",
        "persist-credentials: false",
        "- name: Verify trusted release gate tooling identity",
        "EXPECTED_WORKFLOW_SHA: ${{ github.workflow_sha }}",
        "Trusted release gate tooling identity mismatch",
        "python3 scripts/validate_release_gate.py",
        "--source-ref \"$RELEASE_REF\"",
    ] {
        assert!(
            gate.contains(required),
            "release-gate missing trusted-tooling contract: {}",
            required
        );
    }

    assert!(
        !gate.contains("ref: ${{ env.RELEASE_REF }}"),
        "release-gate must never execute validator tooling from RELEASE_REF"
    );
}

#[test]
fn release_gate_validator_treats_release_ref_as_fixed_source_data() {
    let validator = read_file("scripts/validate_release_gate.py");

    for required in [
        "RELEASE_SOURCE_FILES",
        "materialize_release_source",
        "--source-ref",
        "source ref must match release tag",
        "validate_release_version(source_root, version)",
    ] {
        assert!(
            validator.contains(required),
            "release gate validator missing source-data boundary: {}",
            required
        );
    }
}

#[test]
fn parity_validator_guards_release_gate_tooling_identity() {
    let parity = read_file("scripts/validate_ci_release_parity.py");

    for required in [
        "Release/release-gate: trusted tooling must use workflow identity",
        "Checkout trusted release gate tooling",
        "--source-ref \"$RELEASE_REF\"",
    ] {
        assert!(
            parity.contains(required),
            "CI/Release parity must guard release-gate trust boundary: {}",
            required
        );
    }
}
