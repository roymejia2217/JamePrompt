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

fn release_job(workflow: &str) -> &str {
    workflow
        .split("\n  release:\n")
        .nth(1)
        .expect("release workflow must define final release job")
}

#[test]
fn write_capable_release_job_checks_out_exact_workflow_tooling_identity() {
    let workflow = read_file(".github/workflows/release.yml");
    let release = release_job(&workflow);

    for required in [
        "- name: Checkout trusted release tooling",
        "ref: ${{ github.workflow_sha }}",
        "fetch-depth: 0",
        "persist-credentials: false",
        "- name: Verify trusted release tooling identity",
        "EXPECTED_WORKFLOW_SHA: ${{ github.workflow_sha }}",
        "Trusted release tooling identity mismatch",
    ] {
        assert!(
            release.contains(required),
            "final write-capable release job missing trusted-tooling contract: {}",
            required
        );
    }

    assert!(
        !release.contains("ref: ${{ env.RELEASE_REF }}"),
        "final write-capable release job must never checkout RELEASE_REF as executable tooling"
    );
}

#[test]
fn release_tag_is_consumed_as_data_not_executable_publication_tooling() {
    let workflow = read_file(".github/workflows/release.yml");
    let release = release_job(&workflow);

    for required in [
        "TAG_CHANGELOG_FILE=\"$RUNNER_TEMP/release-changelog.md\"",
        "git show \"${TAG_NAME}:CHANGELOG.md\" > \"$TAG_CHANGELOG_FILE\"",
        "--changelog \"$TAG_CHANGELOG_FILE\"",
    ] {
        assert!(
            release.contains(required),
            "final publication must read release metadata from tag data: {}",
            required
        );
    }

    let checkout = release
        .find("- name: Checkout trusted release tooling")
        .expect("trusted tooling checkout must exist");
    let changelog = release
        .find("git show \"${TAG_NAME}:CHANGELOG.md\"")
        .expect("tag changelog extraction must exist");
    let publish = release
        .find("gh release create \"$TAG_NAME\"")
        .expect("release publication must remain present");

    assert!(
        checkout < changelog && changelog < publish,
        "trusted tooling must be established before tag data is consumed and published"
    );
}

#[test]
fn parity_validator_protects_write_token_tooling_boundary() {
    let parity = read_file("scripts/validate_ci_release_parity.py");

    for required in [
        "github.workflow_sha",
        "persist-credentials: false",
        "fetch-depth: 0",
        "TAG_CHANGELOG_FILE",
        "Release/release: write-capable tooling must use workflow identity",
        "ref: ${{ env.RELEASE_REF }}",
    ] {
        assert!(
            parity.contains(required),
            "CI/Release parity must protect trusted-tooling boundary: {}",
            required
        );
    }
}
