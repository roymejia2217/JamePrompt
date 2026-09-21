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
fn reusable_release_run_validator_binds_tag_sha_jobs_and_artifacts() {
    let validator = read_file("scripts/validate_reusable_release_run.py");

    for required in [
        "actions/runs",
        "workflow_runs",
        ".github/workflows/release.yml",
        "event",
        "push",
        "head_branch",
        "head_sha",
        "release-gate",
        "deb",
        "arch",
        "rpm",
        "appimage",
        "windows",
        "deb-package",
        "arch-package",
        "rpm-package",
        "appimage-package",
        "windows-portable",
        "windows-msi",
        "expired",
        "size_in_bytes",
        "--current-run-id",
        "--self-test",
        "reusable release run accepted",
    ] {
        assert!(
            validator.contains(required),
            "reusable release validator must include contract: {}",
            required
        );
    }

    for forbidden in ["|| true", "except Exception:", "return True"] {
        assert!(
            !validator.contains(forbidden),
            "reusable release validator must remain fail-closed: {}",
            forbidden
        );
    }
}

#[test]
fn release_validates_reusable_run_before_downloading_any_existing_artifact() {
    let release = read_file(".github/workflows/release.yml");

    let validate = release
        .find("- name: Validate reusable release run provenance")
        .expect("release workflow must validate reusable run provenance");
    let download = release
        .find("- name: Download artifacts from existing run")
        .expect("release workflow must retain existing-run download");
    let publish = release
        .find("- name: Create GitHub release")
        .expect("release workflow must retain publication step");

    assert!(
        validate < download && download < publish,
        "reusable run provenance must pass before artifact download and publication"
    );

    for required in [
        "inputs.publish_run_id != ''",
        "scripts/validate_reusable_release_run.py",
        "--run-id \"${{ inputs.publish_run_id }}\"",
        "--tag \"$RELEASE_REF\"",
        "--target-sha \"$TARGET_SHA\"",
        "--current-run-id \"$GITHUB_RUN_ID\"",
        "git rev-parse \"${RELEASE_REF}^{}\"",
    ] {
        assert!(
            release.contains(required),
            "release reusable-run gate must include contract: {}",
            required
        );
    }
}

#[test]
fn protected_and_local_gates_self_test_reusable_release_provenance() {
    let ci = read_file(".github/workflows/ci.yml");
    let local_gate = read_file("scripts/verify_change_gate.sh");
    let parity = read_file("scripts/validate_ci_release_parity.py");

    assert!(
        ci.contains("python3 scripts/validate_reusable_release_run.py --self-test"),
        "protected CI must self-test reusable release provenance"
    );
    assert!(
        local_gate.contains("python3 scripts/validate_reusable_release_run.py --self-test"),
        "local change gate must self-test reusable release provenance"
    );
    assert!(
        parity.contains("scripts/validate_reusable_release_run.py"),
        "CI/Release parity validator must protect the reusable-run provenance gate"
    );
}
