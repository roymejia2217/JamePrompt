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

fn package_version() -> String {
    let cargo = read_file("Cargo.toml");
    cargo
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_string)
        .expect("Cargo.toml should define a package version")
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

#[test]
fn winget_manifests_are_ready_for_msi_submission() {
    let version = package_version();
    let base = format!("packaging/winget/manifests/r/RoyMejia/JamePrompt/{version}");
    let version_manifest = read_file(&format!("{base}/RoyMejia.JamePrompt.yaml"));
    let locale_manifest = read_file(&format!("{base}/RoyMejia.JamePrompt.locale.en-US.yaml"));
    let installer_manifest = read_file(&format!("{base}/RoyMejia.JamePrompt.installer.yaml"));

    assert_contains_all(
        &version_manifest,
        &[
            "PackageIdentifier: RoyMejia.JamePrompt",
            &format!("PackageVersion: {version}"),
            "DefaultLocale: en-US",
            "ManifestType: version",
        ],
    );
    assert_contains_all(
        &locale_manifest,
        &[
            "PackageIdentifier: RoyMejia.JamePrompt",
            &format!("PackageVersion: {version}"),
            "Publisher: Roy Mejia",
            "PackageName: JamePrompt",
            "PackageUrl: https://github.com/roymejia2217/JamePrompt",
            "License: MIT",
            "LicenseUrl: https://github.com/roymejia2217/JamePrompt/blob/main/LICENSE",
            "ShortDescription: Lightweight and minimal local prompt manager",
            "ManifestType: defaultLocale",
        ],
    );
    assert_contains_all(
        &installer_manifest,
        &[
            "PackageIdentifier: RoyMejia.JamePrompt",
            &format!("PackageVersion: {version}"),
            "InstallerType: msi",
            "Scope: machine",
            "InstallModes:",
            "- silent",
            "UpgradeBehavior: install",
            &format!(
                "InstallerUrl: https://github.com/roymejia2217/JamePrompt/releases/download/v{version}/JamePrompt-{version}-x64.msi"
            ),
            "InstallerSha256: 740798A2A471DA3F689C42F0BC160685D6B5742CDBDF2D612C5B1049DA67F9B8",
            "AppsAndFeaturesEntries:",
            "DisplayName: JamePrompt",
            "Publisher: Roy Mejia",
            "ManifestType: installer",
        ],
    );
}

#[test]
fn chocolatey_package_metadata_uses_official_release_msi() {
    let version = package_version();
    let nuspec = read_file("packaging/chocolatey/jame-prompt.nuspec");
    let install = read_file("packaging/chocolatey/tools/chocolateyInstall.ps1");
    let uninstall = read_file("packaging/chocolatey/tools/chocolateyUninstall.ps1");

    assert_contains_all(
        &nuspec,
        &[
            "<id>jame-prompt</id>",
            &format!("<version>{version}</version>"),
            "<title>JamePrompt</title>",
            "<authors>Roy Mejia</authors>",
            "<projectUrl>https://github.com/roymejia2217/JamePrompt</projectUrl>",
            "<licenseUrl>https://github.com/roymejia2217/JamePrompt/blob/main/LICENSE</licenseUrl>",
            "<packageSourceUrl>https://github.com/roymejia2217/JamePrompt/tree/main/packaging/chocolatey</packageSourceUrl>",
            "<tags>jame-prompt prompt-manager prompts ai productivity local sqlite hotkeys</tags>",
        ],
    );
    assert_contains_all(
        &install,
        &[
            "$packageName = 'jame-prompt'",
            "$softwareName = 'JamePrompt*'",
            "$installerType = 'msi'",
            "$silentArgs = '/qn /norestart'",
            &format!(
                "$url64 = 'https://github.com/roymejia2217/JamePrompt/releases/download/v{version}/JamePrompt-{version}-x64.msi'"
            ),
            "$checksum64 = '740798A2A471DA3F689C42F0BC160685D6B5742CDBDF2D612C5B1049DA67F9B8'",
            "$checksumType64 = 'sha256'",
            "Install-ChocolateyPackage",
        ],
    );
    assert_contains_all(
        &uninstall,
        &[
            "$packageName = 'jame-prompt'",
            "$softwareName = 'JamePrompt*'",
            "$installerType = 'msi'",
            "$silentArgs = '/qn /norestart'",
            "Get-UninstallRegistryKey",
            "Uninstall-ChocolateyPackage",
        ],
    );
}

#[test]
fn chocolatey_powershell_scripts_use_utf8_bom_encoding() {
    for relative in [
        "packaging/chocolatey/tools/chocolateyInstall.ps1",
        "packaging/chocolatey/tools/chocolateyUninstall.ps1",
    ] {
        let path = repo_path(relative);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("Expected {} to be readable: {}", path.display(), error));

        assert!(
            bytes.starts_with(&[0xef, 0xbb, 0xbf]),
            "{} should use UTF-8 with BOM for Chocolatey PowerShell compatibility",
            relative
        );
    }
}

#[test]
fn windows_distribution_docs_capture_submission_and_release_contract() {
    let docs = read_file("docs/distribution/windows.md");

    assert_contains_all(
        &docs,
        &[
            "# Windows Distribution",
            "GitHub Releases are the canonical binary source",
            "Winget",
            "Chocolatey",
            "JamePrompt-1.0.0-x64.msi",
            "SHA256SUMS",
            "winget validate",
            "Windows Sandbox",
            "choco pack",
            "choco install jame-prompt",
            "Do not announce Winget or Chocolatey availability until the package has been accepted by the target repository.",
        ],
    );
}

#[test]
fn release_workflow_publishes_checksums_with_artifacts() {
    let workflow = read_file(".github/workflows/release.yml");

    assert_contains_all(
        &workflow,
        &[
            "find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS",
            "RELEASE_FILES+=(\"release-artifacts/SHA256SUMS\")",
        ],
    );
}
