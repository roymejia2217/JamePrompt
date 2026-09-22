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
fn published_asset_validator_can_require_complete_remote_state() {
    let validator = read_file("scripts/validate_existing_release_assets.py");

    for required in [
        "--require-complete",
        "published release is incomplete",
        "missing release asset",
        "if args.require_complete and missing",
    ] {
        assert!(
            validator.contains(required),
            "asset validator must support complete-state attestation: {}",
            required
        );
    }
}

#[test]
fn release_workflow_attests_remote_state_after_create_or_recovery() {
    let release = read_file(".github/workflows/release.yml");

    for required in [
        "FINAL_RELEASE_JSON",
        "FINAL_RELEASE_STATE_FILE",
        "FINAL_RELEASE_STATE",
        "Post-publication release attestation failed",
        "--existing-release-json \"$FINAL_RELEASE_JSON\"",
        "--release-json \"$FINAL_RELEASE_JSON\"",
        "--require-complete",
        "Published release attestation passed.",
    ] {
        assert!(
            release.contains(required),
            "Release workflow missing post-publication attestation: {}",
            required
        );
    }

    let create = release
        .find("gh release create \"$TAG_NAME\"")
        .expect("release creation must remain present");
    let upload = release
        .find("gh release upload \"$TAG_NAME\"")
        .expect("release recovery upload must remain present");
    let final_probe = release
        .rfind("python3 scripts/probe_github_release.py")
        .expect("final release state must be probed");

    assert!(
        final_probe > create && final_probe > upload,
        "final release probe must execute after every publication mutation path"
    );
}

#[test]
fn parity_contract_requires_post_publication_attestation() {
    let parity = read_file("scripts/validate_ci_release_parity.py");

    for required in [
        "FINAL_RELEASE_STATE",
        "--require-complete",
        "Published release attestation passed.",
        "Release workflow must attest published state after mutation",
    ] {
        assert!(
            parity.contains(required),
            "CI/Release parity must protect publication attestation: {}",
            required
        );
    }
}
