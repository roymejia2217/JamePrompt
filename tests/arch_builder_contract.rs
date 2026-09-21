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
fn release_arch_builder_uses_pinned_image_and_archive_snapshot() {
    let workflow = read_file(".github/workflows/release.yml");

    for required in [
        "archlinux:base-devel-20260913.0.592969@sha256:70d777aaeb45befc04150df137c4d7c1b5042be442b4c904c38c6f6880bb7844",
        "bash scripts/build_arch_package.sh",
    ] {
        assert!(
            workflow.contains(required),
            "Arch release workflow must include immutable builder contract: {}",
            required
        );
    }

    assert!(
        !workflow.contains("archlinux:base-devel \\"),
        "release workflow must not use the moving Arch base-devel tag"
    );
}

#[test]
fn arch_builder_uses_single_dated_archive_without_live_mirror_fallback() {
    let builder = read_file("scripts/build_arch_package.sh");

    for required in [
        "ARCHIVE_DATE=\"2026/09/13\"",
        r"https://archive.archlinux.org/repos/${ARCHIVE_DATE}/\$repo/os/\$arch",
        "/etc/pacman.d/mirrorlist",
        "pacman-key --init",
        "pacman-key --populate archlinux",
        "pacman -Syy --noconfirm --needed",
        "makepkg --nodeps --noconfirm --cleanbuild --clean",
        "--self-test",
    ] {
        assert!(
            builder.contains(required),
            "Arch builder must include snapshot contract: {}",
            required
        );
    }

    for forbidden in [
        "geo.mirror.pkgbuild.com",
        "mirror.rackspace.com",
        "Server = https://mirror",
        "archlinux.org/mirrorlist",
    ] {
        assert!(
            !builder.contains(forbidden),
            "Arch builder must not fall back to live mirrors: {}",
            forbidden
        );
    }
}

#[test]
fn protected_ci_self_tests_arch_snapshot_builder_contract() {
    let ci = read_file(".github/workflows/ci.yml");

    assert!(
        ci.contains("bash scripts/build_arch_package.sh --self-test"),
        "protected CI must exercise the Arch snapshot builder self-test"
    );
}


#[test]
fn arch_snapshot_downloads_use_bounded_retry_without_weakening_integrity() {
    let builder = read_file("scripts/build_arch_package.sh");

    for required in [
        "MAX_DOWNLOAD_ATTEMPTS=3",
        "RETRY_DELAY_SECONDS=5",
        "retry_with_backoff()",
        "retry_with_backoff \"$MAX_DOWNLOAD_ATTEMPTS\" \"$RETRY_DELAY_SECONDS\" pacman -Syy",
        "transient_failure_then_success",
        "permanent_failure",
        "retry self-test passed",
    ] {
        assert!(
            builder.contains(required),
            "Arch snapshot retry contract must include: {}",
            required
        );
    }

    for forbidden in [
        "DisableDownloadTimeout",
        "SigLevel = Never",
        "SigLevel=Never",
        "|| true",
    ] {
        assert!(
            !builder.contains(forbidden),
            "Arch retry must not weaken integrity or fail-open: {}",
            forbidden
        );
    }
}
