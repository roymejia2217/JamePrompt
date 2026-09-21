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
fn protected_ci_builds_and_validates_real_debian_package() {
    let ci = read_file(".github/workflows/ci.yml");

    let deb_job = ci
        .split("\n  test_deb:\n")
        .nth(1)
        .expect("protected CI must define a test_deb job")
        .split("\n  test_arch:\n")
        .next()
        .expect("test_deb job must precede test_arch");

    for required in [
        "runs-on: ubuntu-latest",
        "timeout-minutes: 45",
        "Install Debian packaging dependencies",
        "lintian",
        "packaging/linux/build-deb.sh",
        "Verify Debian package artifact",
        "scripts/validate_deb_package.sh",
        "find target/debian -maxdepth 1 -type f",
        "-name '*.deb'",
        "test -s \"$DEB_PACKAGE\"",
    ] {
        assert!(
            deb_job.contains(required),
            "protected Debian preflight must include contract: {}",
            required
        );
    }

    for forbidden in ["upload-artifact", "gh release", "contents: write"] {
        assert!(
            !deb_job.contains(forbidden),
            "Debian preflight must not publish artifacts: {}",
            forbidden
        );
    }
}

#[test]
fn debian_package_validator_checks_metadata_payload_dependencies_and_lintian() {
    let validator = read_file("scripts/validate_deb_package.sh");

    for required in [
        "EXPECTED_NAME=\"jame-prompt\"",
        "dpkg-deb -f",
        "dpkg-deb --extract",
        "dpkg-deb --control",
        "dpkg --print-architecture",
        "libxkbcommon-x11-0",
        "xdg-desktop-portal",
        "xdg-desktop-portal-gnome",
        "usr/bin/jame-prompt",
        "usr/share/applications/io.github.roymejia2217.JamePrompt.desktop",
        "DEBIAN/postinst",
        "DEBIAN/postrm",
        "lintian --fail-on error",
        "Debian package validation passed",
    ] {
        assert!(
            validator.contains(required),
            "Debian package validator must include contract: {}",
            required
        );
    }
}

#[test]
fn release_validates_debian_package_before_upload() {
    let workflow = read_file(".github/workflows/release.yml");
    let deb_job = workflow
        .split("\n  deb:\n")
        .nth(1)
        .expect("release workflow must define a deb job")
        .split("\n  arch:\n")
        .next()
        .expect("deb job must precede arch");

    let build = deb_job
        .find("- name: Build Debian package")
        .expect("release Debian job must build the package");
    let validate = deb_job
        .find("- name: Validate Debian package artifact")
        .expect("release Debian job must validate the package");
    let upload = deb_job
        .find("- name: Upload Debian package")
        .expect("release Debian job must upload the package");

    assert!(
        build < validate && validate < upload,
        "Debian validation must run after build and before upload"
    );
    assert!(
        deb_job.contains("scripts/validate_deb_package.sh"),
        "release Debian validation must use the shared validator"
    );
}

#[test]
fn aggregate_test_requires_debian_preflight() {
    let ci = read_file(".github/workflows/ci.yml");
    let aggregate = ci
        .split("\n  test:\n")
        .nth(1)
        .expect("CI must define the protected aggregate test job");

    for required in [
        "- test-linux",
        "- test-windows",
        "- test_deb",
        "- test_arch",
        "- test_rpm",
        "DEB_RESULT: ${{ needs.test_deb.result }}",
        "test \"$DEB_RESULT\" = \"success\"",
    ] {
        assert!(
            aggregate.contains(required),
            "protected aggregate test must require Debian preflight: {}",
            required
        );
    }
}
