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
fn protected_ci_builds_and_validates_real_msi() {
    let ci = read_file(".github/workflows/ci.yml");
    let windows = ci
        .split("\n  test-windows:\n")
        .nth(1)
        .expect("CI must define test-windows")
        .split("\n  test_deb:\n")
        .next()
        .expect("test-windows must precede test_deb");

    for required in [
        "Install pinned WiX Toolset",
        "wixtoolset --version 3.14.1.20250415",
        "Install pinned cargo-wix",
        "cargo install cargo-wix --version 0.3.9 --locked",
        "Build MSI",
        "scripts/build_windows_msi.ps1",
        "Validate MSI install and uninstall",
        "scripts/validate_windows_msi.ps1",
    ] {
        assert!(
            windows.contains(required),
            "Windows CI must include MSI preflight contract: {}",
            required
        );
    }

    for forbidden in ["upload-artifact", "gh release", "contents: write"] {
        assert!(
            !windows.contains(forbidden),
            "Windows MSI preflight must not publish artifacts: {}",
            forbidden
        );
    }
}

#[test]
fn release_validates_msi_before_uploading_it() {
    let workflow = read_file(".github/workflows/release.yml");
    let windows = workflow
        .split("\n  windows:\n")
        .nth(1)
        .expect("release workflow must define windows job")
        .split("\n  release:\n")
        .next()
        .expect("windows job must precede release job");

    for required in [
        "wixtoolset --version 3.14.1.20250415",
        "cargo install cargo-wix --version 0.3.9 --locked",
        "scripts/build_windows_msi.ps1",
        "scripts/validate_windows_msi.ps1",
    ] {
        assert!(
            windows.contains(required),
            "release Windows job must include deterministic MSI contract: {}",
            required
        );
    }

    let build = windows
        .find("- name: Build MSI")
        .expect("release must build MSI");
    let validate = windows
        .find("- name: Validate MSI install and uninstall")
        .expect("release must validate MSI");
    let upload = windows
        .find("- name: Upload MSI")
        .expect("release must upload MSI");

    assert!(
        build < validate && validate < upload,
        "MSI must be validated after build and before upload"
    );
}

#[test]
fn msi_builder_reuses_release_binary_without_rebuilding_product() {
    let builder = read_file("scripts/build_windows_msi.ps1");

    for required in [
        "& cargo @cargoArgs",
        "\"wix\"",
        "--no-build",
        "--target",
        "--target-bin-dir",
        "JamePrompt-$Version-x64.msi",
        "Test-Path",
        "MSI build did not produce",
    ] {
        assert!(
            builder.contains(required),
            "MSI builder must include contract: {}",
            required
        );
    }
}

#[test]
fn msi_validator_exercises_install_runtime_and_uninstall_lifecycle() {
    let validator = read_file("scripts/validate_windows_msi.ps1");

    for required in [
        "msiexec.exe",
        "/i",
        "/x",
        "/qn",
        "/norestart",
        "$env:ProgramFiles",
        "JamePrompt",
        "jame-prompt.exe",
        "scripts/validate_windows_runtime.ps1",
        "scripts/smoke/native-hotkey-windows.ps1",
        "Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "HKLM:\\SOFTWARE\\JamePrompt",
        "HKCU:\\SOFTWARE\\JamePrompt",
        "Start Menu",
        "MSI install did not create user shortcut registry marker",
        "MSI uninstall left installed binary behind",
        "MSI uninstall left machine registry marker behind",
        "MSI uninstall left user shortcut registry marker behind",
    ] {
        assert!(
            validator.contains(required),
            "MSI validator must exercise lifecycle contract: {}",
            required
        );
    }
}

#[test]
fn msi_builder_accepts_supported_release_profile_prereleases() {
    let builder = read_file("scripts/build_windows_msi.ps1");

    for required in [
        "$SupportedVersionPattern",
        "(?:alpha|beta)",
        "JamePrompt-$Version-x64.msi",
    ] {
        assert!(
            builder.contains(required),
            "MSI builder must support repository prerelease identity: {}",
            required
        );
    }

    assert!(
        !builder.contains("MSI build requires a stable numeric package version"),
        "MSI builder must not reject supported alpha/beta source versions"
    );
}

#[test]
fn msi_builder_prerelease_support_remains_fail_closed() {
    let builder = read_file("scripts/build_windows_msi.ps1");

    for required in [
        "$Version -match $SupportedVersionPattern",
        "$InstallerVersion = $Matches['base']",
        "supported stable/alpha/beta SemVer package version",
        "\"--no-build\"",
        "JamePrompt-$Version-x64.msi",
    ] {
        assert!(
            builder.contains(required),
            "MSI prerelease support must preserve fail-closed builder behavior: {}",
            required
        );
    }

    assert!(
        builder.contains("(?:alpha|beta)"),
        "MSI prerelease support must remain scoped to the repository release profile"
    );
    assert!(
        !builder.contains("(?:alpha|beta|rc)"),
        "MSI builder must not silently expand the release profile to rc"
    );
}

#[test]
fn protected_ci_resolves_msi_path_from_tracked_package_version() {
    let ci = read_file(".github/workflows/ci.yml");
    let windows = ci
        .split("\n  test-windows:\n")
        .nth(1)
        .expect("CI must define test-windows")
        .split("\n  test_deb:\n")
        .next()
        .expect("test-windows must precede test_deb");

    for required in [
        "$version = (Select-String -Path Cargo.toml",
        "JamePrompt-$version-x64.msi",
    ] {
        assert!(
            windows.contains(required),
            "Windows CI must derive MSI validation identity from tracked package metadata: {}",
            required
        );
    }

    assert!(
        !windows.contains("JamePrompt-1.1.0-x64.msi"),
        "Windows CI must not pin MSI validation to a historical stable version"
    );
}

#[test]
fn msi_builder_has_one_execution_path_and_numeric_product_version() {
    let builder = read_file("scripts/build_windows_msi.ps1");

    for required in [
        "$InstallerVersion = $Matches['base']",
        "\"--install-version\", $InstallerVersion",
        "JamePrompt-$Version-x64.msi",
    ] {
        assert!(
            builder.contains(required),
            "MSI builder must separate artifact SemVer from numeric ProductVersion: {}",
            required
        );
    }

    assert_eq!(
        builder.matches("& cargo @cargoArgs").count(),
        1,
        "MSI builder must invoke cargo-wix exactly once"
    );
    assert_eq!(
        builder.matches("Write-Host \"Built MSI:").count(),
        1,
        "MSI builder must have exactly one successful completion path"
    );
}

#[test]
fn windows_packaging_uses_shared_fail_fast_powershell_syntax_gate() {
    let validator = read_file("scripts/validate_windows_packaging_syntax.ps1");

    for required in [
        "System.Management.Automation.Language.Parser",
        "ParseFile",
        "scripts/build_windows_msi.ps1",
        "scripts/validate_windows_msi.ps1",
        "scripts/validate_windows_runtime.ps1",
        "scripts/smoke/native-hotkey-windows.ps1",
        "PowerShell syntax validation failed",
    ] {
        assert!(
            validator.contains(required),
            "shared Windows packaging syntax validator missing contract: {}",
            required
        );
    }

    let ci = read_file(".github/workflows/ci.yml");
    let ci_windows = ci
        .split("\n  test-windows:\n")
        .nth(1)
        .expect("CI must define test-windows")
        .split("\n  test_deb:\n")
        .next()
        .expect("test-windows must precede test_deb");
    let ci_syntax = ci_windows
        .find("Validate Windows packaging PowerShell syntax")
        .expect("CI must validate Windows packaging PowerShell syntax");
    let ci_tests = ci_windows
        .find("Run Windows tests")
        .expect("CI must run Windows tests");
    assert!(
        ci_syntax < ci_tests,
        "PowerShell syntax validation must fail fast before expensive Windows tests"
    );

    let release = read_file(".github/workflows/release.yml");
    let release_windows = release
        .split("\n  windows:\n")
        .nth(1)
        .expect("Release workflow must define windows job")
        .split("\n  release:\n")
        .next()
        .expect("windows job must precede release job");
    let release_syntax = release_windows
        .find("Validate Windows packaging PowerShell syntax")
        .expect("Release must validate Windows packaging PowerShell syntax");
    let release_build = release_windows
        .find("Build Windows binaries")
        .expect("Release must build Windows binaries");
    assert!(
        release_syntax < release_build,
        "Release syntax validation must run before expensive Windows builds"
    );

    for windows in [ci_windows, release_windows] {
        assert!(
            windows.contains("scripts/validate_windows_packaging_syntax.ps1"),
            "CI and Release must reuse the same PowerShell syntax validator"
        );
    }
}

#[test]
fn prerelease_msi_distribution_policy_remains_stable_only() {
    let release = read_file(".github/workflows/release.yml");
    let windows = release
        .split("\n  windows:\n")
        .nth(1)
        .expect("Release workflow must define windows job")
        .split("\n  release:\n")
        .next()
        .expect("windows job must precede release job");

    for step in [
        "Install pinned WiX Toolset",
        "Install pinned cargo-wix",
        "Build MSI",
        "Validate MSI install and uninstall",
        "Upload MSI",
    ] {
        let start = windows
            .find(&format!("- name: {step}"))
            .unwrap_or_else(|| panic!("Release Windows job missing step: {step}"));
        let suffix = &windows[start..];
        let boundary = suffix
            .find("\n      - name:")
            .map(|index| &suffix[..index])
            .unwrap_or(suffix);
        assert!(
            boundary.contains("if: env.JAMEPROMPT_IS_PRERELEASE != 'true'"),
            "prerelease policy must keep MSI step stable-only: {step}"
        );
    }

    let staging = read_file("scripts/stage_release_assets.py");
    assert!(
        staging.contains("EXPECTED_STABLE = EXPECTED_PRERELEASE | {\"windows-msi\"}"),
        "asset staging must keep windows-msi outside prerelease assets"
    );

    let reusable = read_file("scripts/validate_reusable_release_run.py");
    assert!(
        reusable.contains(
            "EXPECTED_STABLE_ARTIFACTS = EXPECTED_PRERELEASE_ARTIFACTS | {\"windows-msi\"}"
        ),
        "reusable release provenance must keep windows-msi stable-only"
    );
}
