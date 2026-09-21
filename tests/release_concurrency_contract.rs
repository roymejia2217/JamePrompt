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
fn release_workflow_serializes_same_tag_without_global_locking_or_cancellation() {
    let release = read_file(".github/workflows/release.yml");

    for required in [
        "concurrency:",
        "group: release-${{ github.event_name == 'workflow_dispatch' && inputs.tag || github.ref_name }}",
        "cancel-in-progress: false",
        "queue: max",
    ] {
        assert!(
            release.contains(required),
            "Release workflow must include same-tag concurrency contract: {}",
            required
        );
    }

    for forbidden in [
        "group: release-global",
        "group: release-${{ github.run_id }}",
        "cancel-in-progress: true",
    ] {
        assert!(
            !release.contains(forbidden),
            "Release concurrency must not cancel in-flight work or serialize unrelated tags: {}",
            forbidden
        );
    }
}

#[test]
fn release_concurrency_group_uses_the_same_tag_identity_as_release_ref() {
    let release = read_file(".github/workflows/release.yml");

    let release_ref =
        "RELEASE_REF: ${{ github.event_name == 'workflow_dispatch' && inputs.tag || github.ref_name }}";
    let concurrency =
        "group: release-${{ github.event_name == 'workflow_dispatch' && inputs.tag || github.ref_name }}";

    assert!(
        release.contains(release_ref),
        "Release workflow must retain canonical RELEASE_REF expression"
    );
    assert!(
        release.contains(concurrency),
        "Release concurrency must derive from the same canonical tag expression"
    );
}
