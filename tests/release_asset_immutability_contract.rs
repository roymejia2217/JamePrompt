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
fn existing_release_assets_are_validated_before_recovery_upload() {
    let validator = read_file("scripts/validate_existing_release_assets.py");
    let release = read_file(".github/workflows/release.yml");

    for required in [
        "validate_existing_assets",
        "sha256:",
        "digest",
        "size",
        "state",
        "uploaded",
        "--release-json",
        "--asset-dir",
        "--write-missing",
        "published release asset mismatch",
        "unexpected published release asset",
        "duplicate published release asset",
    ] {
        assert!(
            validator.contains(required),
            "asset immutability validator missing contract: {}",
            required
        );
    }

    for required in [
        "validate_existing_release_assets.py",
        "--release-json",
        "--asset-dir release-artifacts",
        "--write-missing",
        "MISSING_RELEASE_FILES",
    ] {
        assert!(
            release.contains(required),
            "Release recovery must enforce immutable assets: {}",
            required
        );
    }
}

#[test]
fn release_recovery_never_clobbers_published_assets() {
    let release = read_file(".github/workflows/release.yml");

    assert!(
        !release.contains("--clobber"),
        "published Release assets must never be deleted and replaced"
    );
    assert!(
        release.contains("gh release upload \"$TAG_NAME\" \"${MISSING_RELEASE_FILES[@]}\""),
        "recovery must upload only validated missing assets"
    );
    assert!(
        release.contains("No missing release assets to upload."),
        "idempotent recovery must explicitly handle a complete release"
    );
}

#[test]
fn asset_immutability_has_self_test_and_parity_protection() {
    let ci = read_file(".github/workflows/ci.yml");
    let local_gate = read_file("scripts/verify_change_gate.sh");
    let parity = read_file("scripts/validate_ci_release_parity.py");

    for source in [&ci, &local_gate] {
        assert!(
            source.contains("validate_existing_release_assets.py --self-test"),
            "protected and local gates must run asset immutability self-test"
        );
    }

    for required in [
        "scripts/validate_existing_release_assets.py",
        "Release workflow must not clobber published assets",
        "--clobber",
    ] {
        assert!(
            parity.contains(required),
            "CI/Release parity must protect published asset immutability: {}",
            required
        );
    }
}
