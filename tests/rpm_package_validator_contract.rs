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
fn rpm_validator_enforces_source_driven_metadata_dependencies_and_payload() {
    let validator = read_file("scripts/validate_rpm_package.sh");

    for required in [
        "set -euo pipefail",
        "SPEC_PATH=",
        "packaging/rpm/jame-prompt.spec",
        "EXPECTED_RELEASE=\"$(rpm --eval \"$SPEC_RELEASE\")\"",
        "EXPECTED_ARCH=\"$(rpm --eval '%{_arch}')\"",
        "rpm -qp --queryformat",
        "%{REQUIRENAME}",
        "%{FILEMODES:perms}",
        "%{FILENAMES}",
        "Requires:",
        "EXPECTED_BINARY=\"/usr/bin/jame-prompt\"",
        "EXPECTED_DESKTOP=\"/usr/share/applications/io.github.roymejia2217.JamePrompt.desktop\"",
        "unexpected package name",
        "unexpected package version",
        "unexpected package release",
        "unexpected package architecture",
        "unexpected package summary",
        "unexpected package license",
        "unexpected package URL",
        "required runtime dependency is missing",
        "package payload is missing executable",
        "package payload binary is not executable",
        "package payload is missing desktop entry",
    ] {
        assert!(
            validator.contains(required),
            "RPM validator must include strong artifact contract: {}",
            required
        );
    }

    for forbidden in ["libxkbcommon-x11", "xdg-desktop-portal", "|| true"] {
        assert!(
            !validator.contains(forbidden),
            "RPM validator must derive dependency authority from the spec and stay fail-closed: {}",
            forbidden
        );
    }
}

#[test]
fn ci_and_release_use_the_same_tracked_rpm_validator() {
    let ci = read_file(".github/workflows/ci.yml");
    let release = read_file(".github/workflows/release.yml");
    let image = "fedora:44@sha256:43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80";
    let invocation = "bash scripts/validate_rpm_package.sh \"$RPM_PATH\"";

    let ci_rpm = ci
        .split("\n  test_rpm:\n")
        .nth(1)
        .expect("protected CI must define test_rpm")
        .split("\n  test:\n")
        .next()
        .expect("test_rpm must precede aggregate test");
    for required in [image, "Verify RPM package artifact", invocation] {
        assert!(
            ci_rpm.contains(required),
            "protected CI RPM preflight must use shared validator contract: {}",
            required
        );
    }

    let release_rpm = release
        .split("\n  rpm:\n")
        .nth(1)
        .expect("release workflow must define rpm")
        .split("\n  appimage:\n")
        .next()
        .expect("rpm must precede appimage");
    for required in [image, "Validate RPM package artifact", invocation] {
        assert!(
            release_rpm.contains(required),
            "release RPM job must use shared validator contract: {}",
            required
        );
    }

    let validate = release_rpm
        .find("Validate RPM package artifact")
        .expect("release RPM validation step must exist");
    let upload = release_rpm
        .find("Upload RPM package")
        .expect("release RPM upload step must exist");
    assert!(
        validate < upload,
        "RPM artifact must be validated before it can be uploaded by the release build job"
    );
}
