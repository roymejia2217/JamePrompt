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
fn release_publication_binds_to_existing_remote_annotated_tag_target() {
    let release = read_file(".github/workflows/release.yml");

    for required in [
        "EXPECTED_TARGET=\"$(git rev-parse \"$TAG_NAME^{commit}\")\"",
        "git ls-remote --exit-code origin \"refs/tags/$TAG_NAME^{}\"",
        "REMOTE_TARGET=",
        "[ \"$REMOTE_TARGET\" = \"$EXPECTED_TARGET\" ]",
        "Remote release tag target mismatch",
        "--verify-tag",
    ] {
        assert!(
            release.contains(required),
            "release publication must preserve remote tag identity contract: {}",
            required
        );
    }

    for forbidden in [
        "--target \"$GITHUB_SHA\"",
        "git tag -f",
        "git push --force",
        "|| true",
    ] {
        assert!(
            !release.contains(forbidden),
            "release publication must not synthesize or force release tag identity: {}",
            forbidden
        );
    }
}

#[test]
fn remote_tag_identity_is_checked_before_release_upload_or_creation() {
    let release = read_file(".github/workflows/release.yml");

    let remote_target = release
        .find("git ls-remote --exit-code origin \"refs/tags/$TAG_NAME^{}\"")
        .expect("release publication must query the exact remote annotated tag target");
    let release_view = release
        .find("gh release view \"$TAG_NAME\"")
        .expect("release publication must retain release existence check");
    let release_upload = release
        .find("gh release upload \"$TAG_NAME\"")
        .expect("release publication must retain recovery upload");
    let release_create = release
        .find("gh release create \"$TAG_NAME\"")
        .expect("release publication must retain release creation");

    assert!(
        remote_target < release_view
            && remote_target < release_upload
            && remote_target < release_create,
        "remote tag identity must be verified before any GitHub Release mutation"
    );
}
