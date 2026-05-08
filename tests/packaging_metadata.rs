use std::fs;
use std::path::{Path, PathBuf};

const APP_ID: &str = "jame-prompt";
const APP_NAME: &str = "JamePrompt";

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_file(relative: &str) -> String {
    let path = repo_path(relative);
    fs::read_to_string(&path)
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
fn linux_desktop_file_exposes_required_launcher_metadata() {
    let desktop = read_file("packaging/linux/jame-prompt.desktop");

    assert_contains_all(
        &desktop,
        &[
            "[Desktop Entry]",
            "Type=Application",
            "Name=JamePrompt",
            "Exec=jame-prompt",
            "Icon=jame-prompt",
            "Categories=Utility;",
        ],
    );
}

#[test]
fn arch_pkgbuild_declares_complete_native_package_contract() {
    let pkgbuild = read_file("packaging/arch/PKGBUILD");

    assert_contains_all(
        &pkgbuild,
        &[
            "pkgname=jame-prompt",
            "pkgver=1.0.0",
            "arch=('x86_64')",
            "depends=(",
            "makedepends=(",
            "build()",
            "check()",
            "package()",
            "cargo build --release --locked",
            "cargo test --locked",
            "install -Dm755",
            "packaging/linux/jame-prompt.desktop",
            "packaging/linux/jame-prompt.1",
            "LICENSE",
        ],
    );
}

#[test]
fn rpm_spec_declares_fedora_and_rhel_package_contract() {
    let spec = read_file("packaging/rpm/jame-prompt.spec");

    assert_contains_all(
        &spec,
        &[
            "Name:           jame-prompt",
            "Version:        1.0.0",
            "License:        MIT",
            "BuildRequires:  cargo",
            "BuildRequires:  rust",
            "BuildRequires:  desktop-file-utils",
            "Requires:       gtk3",
            "Requires:       libxdo",
            "%build",
            "cargo build --release --locked",
            "%check",
            "cargo test --locked",
            "%install",
            "desktop-file-validate",
            "%files",
            "%{_bindir}/jame-prompt",
            "%{_datadir}/applications/jame-prompt.desktop",
            "%{_mandir}/man1/jame-prompt.1*",
        ],
    );
}

#[test]
fn appimage_builder_declares_complete_appdir_layout() {
    let script = read_file("packaging/appimage/build-appimage.sh");

    assert_contains_all(
        &script,
        &[
            "APP_ID=\"jame-prompt\"",
            "APP_NAME=\"JamePrompt\"",
            "APPDIR=",
            "cargo build --release --locked",
            "usr/bin/${APP_ID}",
            "usr/lib",
            "AppRun",
            "${APP_ID}.desktop",
            "${APP_ID}.png",
            "linuxdeploy",
            "appimagetool",
        ],
    );
}

#[test]
fn packaging_requirements_document_supported_targets_and_flatpak_exclusion() {
    let doc = read_file("packaging/linux/README.md");

    assert_contains_all(
        &doc,
        &[
            "# Linux Packaging",
            "Debian",
            "Arch",
            "Fedora",
            "RHEL",
            "AppImage",
            "Flatpak is not supported",
            "global hotkeys",
            "paste simulation",
            "system tray",
            "autostart",
        ],
    );
}

#[test]
fn readme_documents_supported_package_targets_without_todo_markers() {
    let readme = read_file("README.md");

    assert_contains_all(
        &readme,
        &[
            "Linux packaging",
            "Windows distribution",
            "Debian",
            "Arch",
            "Fedora",
            "RHEL",
            "AppImage",
            "Windows",
            "Flatpak is not supported",
        ],
    );
    assert!(
        !readme.contains("TODO"),
        "README.md must not contain TODO markers"
    );
    assert!(
        !readme.contains("agregar"),
        "README.md must keep generated text in English"
    );
}

#[test]
fn basic_build_script_orchestrates_all_supported_builds() {
    let script = read_file("build.sh");

    assert_contains_all(
        &script,
        &[
            "CLEAN target",
            "BUILD cargo",
            "BUILD debian",
            "BUILD arch",
            "BUILD rpm",
            "BUILD appimage",
            "JAME_PROMPT_REUSE_RELEASE_BUILD=1",
            "ALL BUILDS COMPLETED",
        ],
    );
    assert!(
        !script.contains("Compilando") && !script.contains("completo"),
        "build.sh must keep generated text in English"
    );
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
            "Create portable archive",
            "Initialize WiX source",
            "Build MSI",
            "windows-exe",
            "windows-portable",
            "windows-msi",
            "x86_64-pc-windows-msvc",
            "cargo build --release --locked --target x86_64-pc-windows-msvc --bins",
            "cargo metadata --format-version 1 --no-deps",
            "Where-Object { $_.kind -contains \"bin\" }",
            "Copy-Item $source \"$portableRoot/$binaryName.exe\"",
            "gh release download --repo linuxdeploy/linuxdeploy",
            "gh release download --repo AppImage/appimagetool",
            "cargo wix init --force",
        ],
    );
    assert!(
        workflow.contains("path: target/x86_64-pc-windows-msvc/release/*.exe"),
        "Windows upload should collect all built executables"
    );
    assert!(
        workflow.contains(
            "needs:\n      - deb\n      - arch\n      - rpm\n      - appimage\n      - windows"
        ),
        "Release job should wait for the Windows artifact job"
    );
}

#[test]
fn package_metadata_uses_consistent_identity() {
    let files = [
        "packaging/linux/jame-prompt.desktop",
        "packaging/arch/PKGBUILD",
        "packaging/rpm/jame-prompt.spec",
        "packaging/appimage/build-appimage.sh",
        "packaging/linux/README.md",
    ];

    for file in files {
        let content = read_file(file);
        assert!(
            content.contains(APP_ID),
            "{} should reference {}",
            file,
            APP_ID
        );
        assert!(
            content.contains(APP_NAME),
            "{} should reference {}",
            file,
            APP_NAME
        );
    }
}
