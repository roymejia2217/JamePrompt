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
fn release_gate_enforces_monotonic_maturity_for_same_base_version() {
    let gate = read_file("scripts/validate_release_gate.py");

    for required in [
        "validate_release_progression",
        "stable_exists_for",
        "alpha release is not allowed after beta or stable",
        "beta release is not allowed after stable",
        "a stable release requires an existing beta for the same version",
        "alpha number must not be older than an existing alpha",
        "beta number must not be older than an existing beta",
    ] {
        assert!(
            gate.contains(required),
            "release gate must enforce maturity invariant: {}",
            required
        );
    }
}

#[test]
fn release_gate_self_test_covers_forward_and_regressive_transitions() {
    let gate = read_file("scripts/validate_release_gate.py");

    for required in [
        "v1.2.0-alpha.2",
        "v1.2.0-beta.10",
        "v1.2.0",
        "alpha_after_beta",
        "alpha_after_stable",
        "beta_after_stable",
        "old_alpha",
        "old_beta",
        "stable_without_beta",
    ] {
        assert!(
            gate.contains(required),
            "release gate self-test must cover transition case: {}",
            required
        );
    }
}

#[test]
fn release_progression_remains_scoped_to_same_semver_base() {
    let gate = read_file("scripts/validate_release_gate.py");

    for required in [
        "prefix = f\"v{version.base}-{kind}.\"",
        "stable_tag = f\"v{version.base}\"",
    ] {
        assert!(
            gate.contains(required),
            "release progression must only inspect tags for the same base: {}",
            required
        );
    }
}
