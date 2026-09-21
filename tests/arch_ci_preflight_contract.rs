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
fn protected_ci_builds_and_validates_real_arch_package() {
    let ci = read_file(".github/workflows/ci.yml");

    let arch_job = ci
        .split("\n  test_arch:\n")
        .nth(1)
        .expect("protected CI must define a test_arch job")
        .split("\n  test_rpm:\n")
        .next()
        .expect("test_arch job must precede test_rpm");

    for required in [
        "runs-on: ubuntu-latest",
        "timeout-minutes: 45",
        "archlinux:base-devel-20260913.0.592969@sha256:70d777aaeb45befc04150df137c4d7c1b5042be442b4c904c38c6f6880bb7844",
        "bash scripts/build_arch_package.sh",
        "Verify Arch package artifact",
        "scripts/validate_arch_package.sh",
        "find packaging/arch -maxdepth 1 -type f",
        "-name '*.pkg.tar.zst'",
        "test -s \"$ARCH_PACKAGE\"",
    ] {
        assert!(
            arch_job.contains(required),
            "protected Arch preflight must include contract: {}",
            required
        );
    }

    for forbidden in ["upload-artifact", "gh release", "contents: write"] {
        assert!(
            !arch_job.contains(forbidden),
            "Arch preflight must not publish artifacts: {}",
            forbidden
        );
    }
}

#[test]
fn arch_package_validator_checks_identity_architecture_and_payload() {
    let validator = read_file("scripts/validate_arch_package.sh");

    for required in [
        "pacman -Qp",
        "pacman -Qip",
        "bsdtar -tf",
        "EXPECTED_NAME=\"jame-prompt\"",
        "pkgver",
        "pkgrel",
        "EXPECTED_VERSION",
        "x86_64",
        "usr/bin/jame-prompt",
        "Arch package validation passed",
    ] {
        assert!(
            validator.contains(required),
            "Arch package validator must include contract: {}",
            required
        );
    }
}

#[test]
fn release_validates_arch_package_before_upload() {
    let workflow = read_file(".github/workflows/release.yml");
    let arch_job = workflow
        .split("\n  arch:\n")
        .nth(1)
        .expect("release workflow must define an arch job")
        .split("\n  rpm:\n")
        .next()
        .expect("arch job must precede rpm");

    let build = arch_job
        .find("- name: Build Arch package in pinned snapshot container")
        .expect("release Arch job must build the package");
    let validate = arch_job
        .find("- name: Validate Arch package artifact")
        .expect("release Arch job must validate the package");
    let upload = arch_job
        .find("- name: Upload Arch package")
        .expect("release Arch job must upload the package");

    assert!(
        build < validate && validate < upload,
        "Arch package validation must run after build and before upload"
    );
    assert!(
        arch_job.contains("scripts/validate_arch_package.sh"),
        "release Arch validation must use the shared validator"
    );
}

#[test]
fn aggregate_test_requires_arch_preflight() {
    let ci = read_file(".github/workflows/ci.yml");
    let aggregate = ci
        .split("\n  test:\n")
        .nth(1)
        .expect("CI must define the protected aggregate test job");

    for required in [
        "- test-linux",
        "- test-windows",
        "- test_arch",
        "- test_rpm",
        "ARCH_RESULT: ${{ needs.test_arch.result }}",
        "test \"$ARCH_RESULT\" = \"success\"",
    ] {
        assert!(
            aggregate.contains(required),
            "protected aggregate test must require Arch preflight: {}",
            required
        );
    }
}
