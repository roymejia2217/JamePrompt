use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_file(relative: &str) -> String {
    let path = repo_path(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Expected {} to be readable: {}", path.display(), error))
}

fn assert_contains_all(content: &str, expected: &[&str]) {
    for value in expected {
        assert!(
            content.contains(value),
            "Expected content to include `{}`",
            value
        );
    }
}

#[test]
fn release_workflow_includes_windows_artifacts_in_the_shared_release_pipeline() {
    let workflow = read_file(".github/workflows/release.yml");

    assert_contains_all(
        &workflow,
        &[
            "runs-on: windows-latest",
            "Install WiX Toolset",
            "Install cargo-wix",
            "Build Windows binaries",
            "RUSTFLAGS: -C target-feature=+crt-static",
            "Validate Windows runtime dependencies",
            "Get-Command dumpbin.exe",
            "vswhere.exe",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "Create portable archive",
            "Build MSI",
            "windows-portable",
            "windows-msi",
            "x86_64-pc-windows-msvc",
            "cargo build --release --locked --target x86_64-pc-windows-msvc --bins",
            "cargo metadata --format-version 1 --no-deps",
            "Where-Object { $_.kind -contains \"bin\" }",
            "Copy-Item $source \"$portableRoot/$binaryName.exe\"",
            "gh release download --repo linuxdeploy/linuxdeploy",
            "gh release download --repo AppImage/appimagetool",
            "cargo wix --no-build --target x86_64-pc-windows-msvc",
            "--target-bin-dir \"target/$env:WINDOWS_TARGET/release\"",
            "--nocapture",
        ],
    );
    assert!(
        !workflow.contains("cargo wix init --force"),
        "Release workflow must use tracked WiX sources instead of regenerating MSI metadata"
    );
    assert!(
        !workflow.contains("name: windows-exe"),
        "Raw application executables should not be published as installer artifacts"
    );
    assert!(
        workflow.contains(
            "needs:\n      - deb\n      - arch\n      - rpm\n      - appimage\n      - windows"
        ),
        "Release job should wait for the Windows artifact job"
    );
    assert!(
        workflow.contains("needs.windows.result == 'success'"),
        "Release job should require successful Windows artifacts before publishing"
    );
}

#[test]
fn windows_release_binary_uses_gui_subsystem() {
    let main = read_file("src/main.rs");

    assert_contains_all(
        &main,
        &["windows_subsystem = \"windows\"", "not(debug_assertions)"],
    );
}

#[test]
fn windows_autostart_sync_uses_registry_run_key() {
    let autostart = read_file("src/autostart.rs");

    assert_contains_all(
        &autostart,
        &[
            "#[cfg(all(target_os = \"windows\", not(test)))]",
            "pub fn sync(enabled: bool) -> Result<(), AutostartError>",
            "sync_windows(enabled)",
            "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            "set_value(APP_ID, &command)",
            "delete_value(APP_ID)",
        ],
    );
    assert!(
        !autostart.contains("#[cfg(all(not(target_os = \"linux\"), not(test)))]\npub fn sync(_enabled: bool) -> Result<(), AutostartError> {\n    Ok(())\n}"),
        "Windows autostart must not be swallowed by the generic non-Linux no-op sync path"
    );
}

#[test]
fn settings_save_applies_autostart_as_background_task() {
    let ui = read_file("src/ui.rs");

    assert_contains_all(
        &ui,
        &[
            "SettingsSaved(Result<(), String>)",
            "Task::perform(",
            "SettingsService::save_and_apply",
            "Message::SettingsSaved",
        ],
    );
}

#[test]
fn wix_source_defines_production_windows_installer_contract() {
    let wix = read_file("wix/main.wxs");

    assert_contains_all(
        &wix,
        &[
            "<Product",
            "Name=\"JamePrompt\"",
            "Manufacturer=\"Roy Mejia\"",
            "InstallScope=\"perMachine\"",
            "ProgramFiles64Folder",
            "Win64=\"yes\"",
            "ProgramMenuFolder",
            "ApplicationProgramsFolder",
            "Name=\"JamePrompt\"",
            "Target=\"[INSTALLFOLDER]jame-prompt.exe\"",
            "WorkingDirectory=\"INSTALLFOLDER\"",
            "Icon=\"AppIcon.ico\"",
            "<RemoveFolder",
            "On=\"uninstall\"",
            "<RegistryValue",
            "Root=\"HKCU\"",
            "KeyPath=\"yes\"",
            "<ComponentRef Id=\"ApplicationShortcut\"",
            "<MajorUpgrade",
        ],
    );
    assert!(
        !wix.contains("Id=\"ALLUSERS\""),
        "cargo-wix already defines ALLUSERS for per-machine packages; main.wxs must not duplicate it"
    );
}
