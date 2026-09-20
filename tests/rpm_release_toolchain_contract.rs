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
fn rpm_release_uses_supported_digest_pinned_fedora_builder() {
    let workflow = read_file(".github/workflows/release.yml");

    for required in [
        "fedora:44@sha256:43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80",
        "bash scripts/build_rpm_fedora.sh",
    ] {
        assert!(
            workflow.contains(required),
            "RPM release workflow must include supported pinned builder contract: {}",
            required
        );
    }

    for forbidden in [
        "fedora:40",
        "sh.rustup.rs",
        "rustup toolchain",
        "rustup default",
        "source /root/.cargo/env",
        "curl --proto",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "RPM release workflow must not use obsolete or mutable Rust bootstrap path: {}",
            forbidden
        );
    }
}

#[test]
fn rpm_builder_uses_fedora_rust_and_enforces_project_minimum() {
    let builder = read_file("scripts/build_rpm_fedora.sh");

    for required in [
        "MINIMUM_RUST_VERSION=\"1.88.0\"",
        "dnf -y install",
        "rpm-build",
        "cargo",
        "rust",
        "rustc --version",
        "cargo --version",
        "sort -V",
        "Rust toolchain is below the required minimum",
        "rpmbuild",
        "--self-test",
    ] {
        assert!(
            builder.contains(required),
            "RPM builder must include toolchain contract: {}",
            required
        );
    }

    for forbidden in ["curl ", "sh.rustup.rs", "rustup "] {
        assert!(
            !builder.contains(forbidden),
            "RPM builder must not bootstrap Rust from mutable remote script: {}",
            forbidden
        );
    }
}

#[test]
fn protected_ci_self_tests_rpm_version_gate() {
    let ci = read_file(".github/workflows/ci.yml");

    assert!(
        ci.contains("bash scripts/build_rpm_fedora.sh --self-test"),
        "protected CI must exercise the RPM builder version gate without publishing artifacts"
    );
}
