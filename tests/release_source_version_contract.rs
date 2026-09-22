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
