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
fn protected_ci_runs_release_profile_native_artifact_smokes() {
    let ci = read_file(".github/workflows/ci.yml");

    for required in [
        "- name: Install verified AppImage tools",
        "scripts/install_appimage_tools.sh",
        "- name: Build release AppImage",
        "packaging/appimage/build-appimage.sh",
        "- name: Run release AppImage X11 native hotkey auto-paste smoke",
        "APPIMAGE_EXTRACT_AND_RUN: \"1\"",
        "scripts/smoke/native-hotkey-x11.sh \"$APPIMAGE\"",
        "- name: Build Windows release native hotkey binary",
        "cargo build --release --locked --target x86_64-pc-windows-msvc --bin jame-prompt",
        "- name: Validate Windows release runtime dependencies",
        "scripts/validate_windows_runtime.ps1",
        "- name: Run Windows release native hotkey auto-paste smoke",
        "./target/x86_64-pc-windows-msvc/release/jame-prompt.exe",
    ] {
        assert!(
            ci.contains(required),
            "protected CI must include release-profile artifact contract: {}",
            required
        );
    }

    assert!(
        ci.contains("needs:\n      - test-linux\n      - test-windows"),
        "protected aggregate test must continue to require both platform jobs"
    );
}

#[test]
fn ci_artifact_validation_has_no_release_publication_capability() {
    let ci = read_file(".github/workflows/ci.yml");

    assert!(
        ci.contains("permissions:\n  contents: read"),
        "CI must retain repository read-only permissions"
    );
    for forbidden in ["contents: write", "gh release create", "gh release upload"] {
        assert!(
            !ci.contains(forbidden),
            "artifact validation CI must not gain release publication capability: {}",
            forbidden
        );
    }
}

#[test]
fn ci_and_release_share_artifact_verification_helpers() {
    let ci = read_file(".github/workflows/ci.yml");
    let release = read_file(".github/workflows/release.yml");

    for helper in [
        "scripts/install_appimage_tools.sh",
        "scripts/validate_windows_runtime.ps1",
    ] {
        assert!(ci.contains(helper), "CI must use shared helper: {}", helper);
        assert!(
            release.contains(helper),
            "release workflow must use shared helper: {}",
            helper
        );
    }

    let appimage_tools = read_file("scripts/install_appimage_tools.sh");
    for required in [
        "LINUXDEPLOY_ASSET_ID",
        "LINUXDEPLOY_SHA256",
        "APPIMAGETOOL_ASSET_ID",
        "APPIMAGETOOL_SHA256",
        "sha256sum --check --strict",
    ] {
        assert!(
            appimage_tools.contains(required),
            "AppImage installer must preserve immutable tool contract: {}",
            required
        );
    }

    let windows_runtime = read_file("scripts/validate_windows_runtime.ps1");
    for required in [
        "Get-Command dumpbin.exe",
        "vswhere.exe",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "VCRUNTIME140.dll",
        "MSVCP140.dll",
        "No Windows executables were found",
    ] {
        assert!(
            windows_runtime.contains(required),
            "Windows runtime validator must preserve release dependency contract: {}",
            required
        );
    }
}
