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
fn debian_lifecycle_helper_installs_verifies_and_purges_package() {
    let lifecycle = read_file("scripts/validate_deb_install_lifecycle.sh");

    for required in [
        "set -euo pipefail",
        "DEBIAN_FRONTEND=noninteractive",
        "apt-get update",
        "apt-get install -y",
        "dpkg-query -W",
        "install ok installed",
        "EXPECTED_BINARY=\"/usr/bin/jame-prompt\"",
        "EXPECTED_DESKTOP=\"/usr/share/applications/io.github.roymejia2217.JamePrompt.desktop\"",
        "apt-get purge -y",
        "package remained registered after purge",
        "package payload binary remained after purge",
        "package desktop entry remained after purge",
        "Debian install lifecycle validation passed",
    ] {
        assert!(
            lifecycle.contains(required),
            "Debian lifecycle helper must include contract: {}",
            required
        );
    }

    for forbidden in ["|| true", "--force-yes", "dpkg --force"] {
        assert!(
            !lifecycle.contains(forbidden),
            "Debian lifecycle validation must remain fail-closed: {}",
            forbidden
        );
    }
}

#[test]
fn ci_and_release_run_the_same_pinned_debian_lifecycle_before_acceptance() {
    let ci = read_file(".github/workflows/ci.yml");
    let release = read_file(".github/workflows/release.yml");
    let image = "debian:trixie-20260713-slim@sha256:020c0d20b9880058cbe785a9db107156c3c75c2ac944a6aa7ab59f2add76a7bd";
    let invocation = "bash scripts/validate_deb_install_lifecycle.sh \"$DEB_PACKAGE\"";

    let ci_deb = ci
        .split("\n  test_deb:\n")
        .nth(1)
        .expect("protected CI must define test_deb")
        .split("\n  test_arch:\n")
        .next()
        .expect("test_deb must precede test_arch");

    for required in [
        "Validate Debian install lifecycle",
        "--platform linux/amd64",
        image,
        invocation,
    ] {
        assert!(
            ci_deb.contains(required),
            "protected CI Debian job must include lifecycle contract: {}",
            required
        );
    }

    let release_deb = release
        .split("\n  deb:\n")
        .nth(1)
        .expect("release workflow must define deb")
        .split("\n  arch:\n")
        .next()
        .expect("deb must precede arch");

    for required in [
        "Validate Debian install lifecycle",
        "--platform linux/amd64",
        image,
        invocation,
    ] {
        assert!(
            release_deb.contains(required),
            "release Debian job must include lifecycle contract: {}",
            required
        );
    }

    let package_validation = release_deb
        .find("Validate Debian package artifact")
        .expect("release Debian package validation must exist");
    let lifecycle = release_deb
        .find("Validate Debian install lifecycle")
        .expect("release Debian lifecycle validation must exist");
    let upload = release_deb
        .find("Upload Debian package")
        .expect("release Debian upload must exist");

    assert!(
        package_validation < lifecycle && lifecycle < upload,
        "release must validate the Debian artifact and install lifecycle before upload"
    );
}
