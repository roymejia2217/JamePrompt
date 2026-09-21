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
fn arch_lifecycle_helper_installs_verifies_and_removes_package() {
    let lifecycle = read_file("scripts/validate_arch_install_lifecycle.sh");

    for required in [
        "set -euo pipefail",
        "bash scripts/build_arch_package.sh --configure-snapshot",
        "pacman -Qp",
        "pacman -U --noconfirm",
        "pacman -Q ",
        "pacman -Qoq",
        "EXPECTED_BINARY=\"/usr/bin/jame-prompt\"",
        "EXPECTED_DESKTOP=\"/usr/share/applications/io.github.roymejia2217.JamePrompt.desktop\"",
        "pacman -Rns --noconfirm",
        "package remained registered after removal",
        "package payload binary remained after removal",
        "package desktop entry remained after removal",
        "Arch install lifecycle validation passed",
    ] {
        assert!(
            lifecycle.contains(required),
            "Arch lifecycle helper must include contract: {}",
            required
        );
    }

    for forbidden in [
        "--nodeps",
        "--noscriptlet",
        "SigLevel = Never",
        "SigLevel=Never",
        "|| true",
        "archive.archlinux.org",
        "ARCHIVE_DATE=",
    ] {
        assert!(
            !lifecycle.contains(forbidden),
            "Arch lifecycle must reuse snapshot authority and remain fail-closed: {}",
            forbidden
        );
    }
}

#[test]
fn arch_builder_exposes_single_snapshot_configuration_entrypoint() {
    let builder = read_file("scripts/build_arch_package.sh");
    let lifecycle = read_file("scripts/validate_arch_install_lifecycle.sh");

    for required in [
        "configure_snapshot()",
        "sync_snapshot_keyring()",
        "--configure-snapshot",
        "ARCHIVE_DATE=\"2026/09/13\"",
        "ARCHIVE_SERVER=",
        "pacman-key --init",
        "pacman-key --populate archlinux",
    ] {
        assert!(
            builder.contains(required),
            "Arch builder must expose shared snapshot configuration contract: {}",
            required
        );
    }

    assert!(
        lifecycle.contains("bash scripts/build_arch_package.sh --configure-snapshot"),
        "Arch lifecycle must delegate snapshot setup to the builder authority"
    );
}

#[test]
fn ci_and_release_run_the_same_pinned_arch_lifecycle_before_acceptance() {
    let ci = read_file(".github/workflows/ci.yml");
    let release = read_file(".github/workflows/release.yml");
    let image =
        "archlinux:base-devel-20260913.0.592969@sha256:70d777aaeb45befc04150df137c4d7c1b5042be442b4c904c38c6f6880bb7844";
    let invocation = "bash scripts/validate_arch_install_lifecycle.sh \"$ARCH_PACKAGE\"";

    let ci_arch = ci
        .split("\n  test_arch:\n")
        .nth(1)
        .expect("protected CI must define test_arch")
        .split("\n  test_rpm:\n")
        .next()
        .expect("test_arch must precede test_rpm");

    for required in ["Validate Arch install lifecycle", image, invocation] {
        assert!(
            ci_arch.contains(required),
            "protected CI Arch job must include lifecycle contract: {}",
            required
        );
    }

    let release_arch = release
        .split("\n  arch:\n")
        .nth(1)
        .expect("release workflow must define arch")
        .split("\n  rpm:\n")
        .next()
        .expect("arch must precede rpm");

    for required in ["Validate Arch install lifecycle", image, invocation] {
        assert!(
            release_arch.contains(required),
            "release Arch job must include lifecycle contract: {}",
            required
        );
    }

    let package_validation = release_arch
        .find("Validate Arch package artifact")
        .expect("release Arch package validation must exist");
    let lifecycle = release_arch
        .find("Validate Arch install lifecycle")
        .expect("release Arch lifecycle validation must exist");
    let upload = release_arch
        .find("Upload Arch package")
        .expect("release Arch upload must exist");

    assert!(
        package_validation < lifecycle && lifecycle < upload,
        "release must validate the Arch artifact and install lifecycle before upload"
    );
}
