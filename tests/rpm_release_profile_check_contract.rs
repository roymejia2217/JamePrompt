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
fn rpm_check_reuses_release_profile_without_skipping_validation() {
    let spec = read_file("packaging/rpm/jame-prompt.spec");

    for required in [
        "cargo build --release --locked",
        "cargo test --release --locked --bin %{name}",
        "desktop-file-validate packaging/linux/jame-prompt.desktop",
    ] {
        assert!(
            spec.contains(required),
            "RPM spec missing release-profile check contract: {required}"
        );
    }
}

#[test]
fn rpm_workflows_keep_full_build_and_check_path() {
    let script = read_file("scripts/build_rpm_fedora.sh");
    let ci = read_file(".github/workflows/ci.yml");
    let release = read_file(".github/workflows/release.yml");

    for source in [script.as_str(), ci.as_str(), release.as_str()] {
        assert!(
            !source.contains("--nocheck"),
            "RPM validation must never bypass rpmbuild %check"
        );
    }

    assert!(
        ci.contains("bash scripts/build_rpm_fedora.sh"),
        "CI must retain the Fedora RPM builder"
    );
    assert!(
        release.contains("bash scripts/build_rpm_fedora.sh"),
        "Release must retain the Fedora RPM builder"
    );
}


#[test]
fn rpm_check_does_not_regress_to_duplicate_debug_compilation() {
    let spec = read_file("packaging/rpm/jame-prompt.spec");

    assert!(
        !spec.contains("cargo test --locked --bin %{name}"),
        "RPM %check must not trigger a second debug-profile dependency build"
    );
    assert!(
        spec.contains("%check"),
        "RPM package must retain an explicit %check phase"
    );
}
