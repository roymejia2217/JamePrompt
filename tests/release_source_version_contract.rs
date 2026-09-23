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
fn release_source_version_check_covers_all_tracked_package_metadata() {
    let versioner = read_file("scripts/prepare_release_version.py");

    for required in [
        "validate_release_version",
        "ReleaseVersionError",
        "--check",
        "Cargo.toml",
        "Cargo.lock",
        "packaging/arch/PKGBUILD",
        "packaging/rpm/jame-prompt.spec",
        "release source version mismatch",
    ] {
        assert!(
            versioner.contains(required),
            "release version checker must include contract: {}",
            required
        );
    }
}

#[test]
fn tag_creation_and_release_gate_require_committed_version_state() {
    let tagger = read_file("scripts/create_release_tag.sh");
    let gate = read_file("scripts/validate_release_gate.py");

    for required in ["scripts/prepare_release_version.py", "--check"] {
        assert!(
            tagger.contains(required),
            "tag creation must verify committed release version state: {}",
            required
        );
    }
    assert!(
        !tagger.contains("--apply"),
        "tag creation must never mutate package versions"
    );

    for required in ["validate_release_version", "Path.cwd()"] {
        assert!(
            gate.contains(required),
            "release gate must verify source version state: {}",
            required
        );
    }

    let check = tagger
        .find("scripts/prepare_release_version.py")
        .expect("tagger must check release source version");
    let tag = tagger
        .find("git tag -a")
        .expect("tagger must retain annotated tag creation");
    assert!(
        check < tag,
        "source version check must run before tag creation"
    );
}

#[test]
fn release_workflow_is_read_only_for_tracked_version_metadata() {
    let release = read_file(".github/workflows/release.yml");

    assert!(
        !release.contains("--apply"),
        "Release workflow must not mutate tracked version metadata"
    );
    assert!(
        release.matches("--check").count() >= 5,
        "each platform Release job must validate source version state"
    );
    assert!(
        release.matches("Validate release source version").count() >= 5,
        "each platform Release job must expose the read-only version gate"
    );
}

#[test]
fn parity_validator_prevents_release_version_mutation_from_returning() {
    let parity = read_file("scripts/validate_ci_release_parity.py");

    for required in [
        "scripts/prepare_release_version.py",
        "--check",
        "Release workflow must not mutate tracked version metadata",
    ] {
        assert!(
            parity.contains(required),
            "CI/Release parity must protect version immutability: {}",
            required
        );
    }
}

#[test]
fn contributor_contract_separates_preparation_from_publication() {
    let contributing = read_file("CONTRIBUTING.md");

    for required in [
        "prepare_release_version.py --tag",
        "--apply",
        "before the release preparation commit",
        "--check",
        "Release jobs never rewrite tracked version metadata",
    ] {
        assert!(
            contributing.contains(required),
            "release contributor contract missing phase boundary: {}",
            required
        );
    }
}

#[test]
fn tracked_release_candidate_identity_is_v1_2_0_beta_10() {
    let cargo = read_file("Cargo.toml");
    let lock = read_file("Cargo.lock");
    let arch = read_file("packaging/arch/PKGBUILD");
    let rpm = read_file("packaging/rpm/jame-prompt.spec");
    let changelog = read_file("CHANGELOG.md");

    assert!(
        cargo.contains("version = \"1.2.0-beta.10\""),
        "Cargo package version must match v1.2.0-beta.10"
    );
    assert!(
        lock.contains("[[package]]\nname = \"jame-prompt\"\nversion = \"1.2.0-beta.10\""),
        "Cargo.lock package version must match v1.2.0-beta.10"
    );
    assert!(
        arch.contains("pkgver=1.2.0beta.10") && arch.contains("pkgrel=1"),
        "Arch metadata must match v1.2.0-beta.10"
    );
    assert!(
        rpm.contains("Version:        1.2.0")
            && rpm.contains("Release:        0.1.beta.10%{?dist}"),
        "RPM metadata must match v1.2.0-beta.10"
    );
    assert!(
        changelog.contains("## [1.2.0-beta.10] - 2026-09-23"),
        "CHANGELOG must contain the prepared beta.10 release block"
    );
}

#[test]
fn beta_10_release_notes_freeze_fixed_entries_and_reset_unreleased() {
    let changelog = read_file("CHANGELOG.md");
    let beta_heading = "## [1.2.0-beta.10] - 2026-09-23";
    assert_eq!(
        changelog.matches(beta_heading).count(),
        1,
        "beta.10 release block must exist exactly once"
    );

    let unreleased = changelog
        .split("## [Unreleased]")
        .nth(1)
        .expect("CHANGELOG must define Unreleased")
        .split(beta_heading)
        .next()
        .expect("Unreleased must precede beta.10");
    let beta = changelog
        .split(beta_heading)
        .nth(1)
        .expect("CHANGELOG must define beta.10");

    let fixed_entries = [
        "Fix native global-hotkey auto-paste to verify the prompt is on the clipboard before Ctrl+V injection and report injection failures instead of assuming success.",
        "Fix packaged Linux X11 hotkeys and auto-paste by declaring or bundling the libxkbcommon-x11 runtime required for X11 keyboard mapping.",
        "Fix the Windows MSI install lifecycle by separating machine installation state from the per-user Start Menu shortcut state and verifying both are removed on uninstall.",
    ];

    for entry in fixed_entries {
        assert!(
            !unreleased.contains(entry),
            "released beta.10 fix must not remain duplicated in Unreleased: {entry}"
        );
        assert!(
            beta.contains(entry),
            "beta.10 release notes must include prepared fix: {entry}"
        );
    }

    let unreleased_fixed = unreleased
        .split("### Fixed")
        .nth(1)
        .expect("Unreleased must define Fixed")
        .split("### Security")
        .next()
        .expect("Unreleased Fixed must precede Security");
    assert!(
        unreleased_fixed.contains("- None."),
        "Unreleased/Fixed must reset after freezing beta.10 notes"
    );

    let mut previous = 0usize;
    for category in [
        "### Added",
        "### Changed",
        "### Deprecated",
        "### Removed",
        "### Fixed",
        "### Security",
    ] {
        let position = beta
            .find(category)
            .unwrap_or_else(|| panic!("beta.10 release notes missing category: {category}"));
        assert!(
            position >= previous,
            "beta.10 Keep a Changelog categories must remain ordered"
        );
        previous = position;
    }
}
