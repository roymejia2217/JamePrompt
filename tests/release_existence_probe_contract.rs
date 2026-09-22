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
fn release_probe_distinguishes_existing_absent_and_api_failure() {
    let probe = read_file("scripts/probe_github_release.py");
    let transport = read_file("scripts/github_api.py");

    for required in [
        "classify_status",
        "status == 200",
        "status == 404",
        "unexpected GitHub Release API status",
        "request_json",
        "accepted_statuses=(200, 404)",
        "--write-status",
        "--write-json",
        "--self-test",
    ] {
        assert!(
            probe.contains(required),
            "release existence probe missing contract: {}",
            required
        );
    }

    for required in [
        "API_VERSION = \"2026-03-10\"",
        "HTTPError",
        "URLError",
        "X-GitHub-Api-Version",
    ] {
        assert!(
            transport.contains(required),
            "shared GitHub REST transport missing probe dependency: {}",
            required
        );
    }
}

#[test]
fn release_probe_normalizes_one_payload_for_metadata_and_assets() {
    let probe = read_file("scripts/probe_github_release.py");
    let metadata = read_file("scripts/validate_release_metadata.py");

    for required in [
        "\"tagName\"",
        "\"name\"",
        "\"body\"",
        "\"isDraft\"",
        "\"isPrerelease\"",
        "\"assets\"",
        "draft GitHub Release is not a published release",
    ] {
        assert!(
            probe.contains(required),
            "release probe must normalize field: {}",
            required
        );
    }

    assert!(
        metadata.contains("\"isDraft\": False"),
        "existing release metadata validation must reject draft state"
    );
}

#[test]
fn release_workflow_uses_explicit_probe_state_before_create_or_recovery() {
    let release = read_file(".github/workflows/release.yml");

    for required in [
        "scripts/probe_github_release.py",
        "--repository \"${{ github.repository }}\"",
        "--tag \"$TAG_NAME\"",
        "--write-status \"$RELEASE_STATE_FILE\"",
        "--write-json \"$EXISTING_RELEASE_JSON\"",
        "RELEASE_STATE=\"$(cat \"$RELEASE_STATE_FILE\")\"",
        "if [ \"$RELEASE_STATE\" = \"existing\" ]; then",
        "elif [ \"$RELEASE_STATE\" = \"absent\" ]; then",
        "Unexpected release probe state",
    ] {
        assert!(
            release.contains(required),
            "Release workflow missing deterministic existence state: {}",
            required
        );
    }

    for forbidden in [
        "gh release view \"$TAG_NAME\"",
        "2>/dev/null",
        "gh api \"repos/${{ github.repository }}/releases/tags/$TAG_NAME\"",
    ] {
        assert!(
            !release.contains(forbidden),
            "Release workflow must not use ambiguous existence probe: {}",
            forbidden
        );
    }
}

#[test]
fn release_probe_is_exercised_by_protected_local_and_parity_gates() {
    let ci = read_file(".github/workflows/ci.yml");
    let local = read_file("scripts/verify_change_gate.sh");
    let parity = read_file("scripts/validate_ci_release_parity.py");

    for source in [&ci, &local] {
        assert!(
            source.contains("probe_github_release.py --self-test"),
            "release existence probe self-test must run in protected/local gates"
        );
    }

    for required in [
        "scripts/probe_github_release.py",
        "RELEASE_STATE",
        "Release workflow must use deterministic release existence probe",
        "gh release view",
    ] {
        assert!(
            parity.contains(required),
            "parity validator must protect deterministic release probing: {}",
            required
        );
    }
}
