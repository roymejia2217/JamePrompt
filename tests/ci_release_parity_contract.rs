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
fn tracked_ci_release_parity_validator_covers_release_facing_paths() {
    let validator = read_file("scripts/validate_ci_release_parity.py");

    for required in [
        "PARITY_CONTRACTS",
        "debian",
        "arch",
        "rpm",
        "appimage",
        "windows",
        "validate_deb_package.sh",
        "validate_deb_install_lifecycle.sh",
        "validate_arch_package.sh",
        "validate_arch_install_lifecycle.sh",
        "validate_rpm_package.sh",
        "install_appimage_tools.sh",
        "native-hotkey-x11.sh",
        "validate_windows_runtime.ps1",
        "native-hotkey-windows.ps1",
        "validate_windows_msi.ps1",
        "release-gate",
        "--self-test",
        "CI/Release parity validation passed",
    ] {
        assert!(
            validator.contains(required),
            "CI/Release parity validator must include contract: {}",
            required
        );
    }

    for forbidden in ["|| true", "except Exception:", "pass  # ignore"] {
        assert!(
            !validator.contains(forbidden),
            "CI/Release parity validator must remain fail-closed: {}",
            forbidden
        );
    }
}

#[test]
fn protected_ci_executes_parity_validator_before_build_and_test_work() {
    let ci = read_file(".github/workflows/ci.yml");

    let parity = ci
        .find("- name: Validate CI and Release parity")
        .expect("protected CI must execute the CI/Release parity validator");
    let formatting = ci
        .find("- name: Check formatting")
        .expect("protected CI must retain formatting gate");

    assert!(
        parity < formatting,
        "CI/Release parity must be checked before compilation-oriented gates"
    );

    for required in [
        "python3 scripts/validate_ci_release_parity.py --self-test",
        "python3 scripts/validate_ci_release_parity.py",
    ] {
        assert!(
            ci.contains(required),
            "protected CI must execute parity contract: {}",
            required
        );
    }
}
