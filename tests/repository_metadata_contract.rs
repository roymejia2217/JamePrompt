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


fn release_block_for<'a>(changelog: &'a str, heading: &str) -> &'a str {
    changelog
        .split(heading)
        .nth(1)
        .unwrap_or_else(|| panic!("CHANGELOG must define release heading: {heading}"))
        .split("\n## [")
        .next()
        .expect("release block must terminate")
}

fn release_fixed_section_for<'a>(changelog: &'a str, heading: &str) -> &'a str {
    release_block_for(changelog, heading)
        .split("### Fixed")
        .nth(1)
        .expect("release block must define Fixed")
        .split("### Security")
        .next()
        .expect("Fixed must precede Security")
}

#[test]
fn commit_messages_use_conventional_commits_with_required_body() {
    let config = read_file("commitlint.config.cjs");
    let title_config = read_file("commitlint.title.config.cjs");
    let tests = read_file("scripts/test-commitlint.sh");

    for required in [
        "'body-empty': [2, 'never']",
        "'body-min-length': [2, 'always', 20]",
        "'body-leading-blank': [2, 'always']",
        "'footer-leading-blank': [2, 'always']",
    ] {
        assert!(
            config.contains(required),
            "commit contract must include rule: {}",
            required
        );
    }

    assert!(
        title_config.contains("'body-empty': [0]"),
        "PR titles must validate the Conventional Commit header without requiring a body"
    );
    assert!(
        title_config.contains("'body-min-length': [0]"),
        "PR title validation must disable commit-body length requirements"
    );
    assert!(
        tests.contains("assert_rejected 'missing body'"),
        "commitlint self-test must prove header-only commits are rejected"
    );
}

#[test]
fn pull_request_template_and_validator_enforce_exact_ordered_schema() {
    let template = read_file(".github/pull_request_template.md");
    let validator = read_file("scripts/validate_pr_description.py");
    let governance = read_file(".github/workflows/pr-governance.yml");

    let headings = [
        "## Summary",
        "## Motivation",
        "## Changes",
        "## Verification",
        "## Risk and rollback",
        "## Release impact",
    ];
    let mut previous = 0usize;
    for heading in headings {
        let position = template
            .find(heading)
            .unwrap_or_else(|| panic!("PR template missing heading: {}", heading));
        assert!(position >= previous, "PR template headings must be ordered");
        previous = position;
    }

    for required in [
        "REQUIRED_HEADINGS",
        "unexpected level-2 section",
        "required sections are out of order",
        "required section contains only a placeholder",
    ] {
        assert!(
            validator.contains(required),
            "PR validator must include strict schema behavior: {}",
            required
        );
    }

    assert!(
        governance.contains("commitlint.title.config.cjs"),
        "PR title governance must use the header-only Conventional Commit profile"
    );
}

#[test]
fn release_metadata_uses_semver_and_keep_a_changelog_as_executable_contracts() {
    let changelog = read_file("CHANGELOG.md");
    let metadata = read_file("scripts/validate_release_metadata.py");
    let release_gate = read_file("scripts/validate_release_gate.py");
    let tagger = read_file("scripts/create_release_tag.sh");
    let release = read_file(".github/workflows/release.yml");

    for required in [
        "Keep a Changelog",
        "Semantic Versioning",
        "## [Unreleased]",
        "### Added",
        "### Changed",
        "### Deprecated",
        "### Removed",
        "### Fixed",
        "### Security",
    ] {
        assert!(
            changelog.contains(required),
            "tracked changelog contract missing: {}",
            required
        );
    }

    for required in [
        "CATEGORIES",
        "alpha",
        "beta",
        "stable",
        "validate_changelog_for_tag",
        "--write-notes",
        "--existing-release-json",
        "release metadata contract: ok",
    ] {
        assert!(
            metadata.contains(required),
            "release metadata validator missing contract: {}",
            required
        );
    }

    for required in [
        "alpha.",
        "beta.",
        "stable",
        "--changelog",
        "validate_changelog_for_tag",
    ] {
        assert!(
            release_gate.contains(required),
            "release gate must enforce release metadata/type contract: {}",
            required
        );
    }

    assert!(
        tagger.contains("vMAJOR.MINOR.PATCH[-alpha.N|-beta.N]"),
        "tag creation usage must advertise the accepted SemVer prerelease profile"
    );
    assert!(
        tagger.contains("validate_release_metadata.py"),
        "tag creation must fail before tagging when release metadata is invalid"
    );

    for required in [
        "validate_release_metadata.py",
        "--write-notes",
        "--notes-file",
        "python3 scripts/probe_github_release.py",
        "--write-json \"$EXISTING_RELEASE_JSON\"",
        "--existing-release-json",
        "--title \"$TAG_NAME\"",
    ] {
        assert!(
            release.contains(required),
            "Release workflow must enforce metadata before publication: {}",
            required
        );
    }

    assert!(
        !release.contains("NOTES=\"Automated release for $TAG_NAME\""),
        "Release body must not be synthesized from an inline ad-hoc note"
    );
}

#[test]
fn release_version_self_test_covers_alpha_packaging_metadata() {
    let versioner = read_file("scripts/prepare_release_version.py");

    for required in [
        "alpha = parse_tag(\"v1.2.0-alpha.1\")",
        "assert alpha.canonical == \"1.2.0-alpha.1\"",
        "assert alpha.debian == \"1.2.0~alpha.1\"",
        "assert alpha.arch == \"1.2.0alpha.1\"",
        "assert alpha.rpm_release == \"0.1.alpha.1\"",
        "apply_release_version(root, alpha)",
    ] {
        assert!(
            versioner.contains(required),
            "release version contract must prove alpha packaging behavior: {}",
            required
        );
    }
}

#[test]
fn protected_and_local_gates_self_test_repository_metadata_contracts() {
    let ci = read_file(".github/workflows/ci.yml");
    let local_gate = read_file("scripts/verify_change_gate.sh");
    let package = read_file("package.json");

    for required in [
        "python3 scripts/validate_pr_description.py --self-test",
        "python3 scripts/validate_release_metadata.py --self-test",
        "bash scripts/test-commitlint.sh",
    ] {
        assert!(
            ci.contains(required) || local_gate.contains(required),
            "metadata contract must be executable from protected/local gates: {}",
            required
        );
    }

    for required in [
        "test:commitlint",
        "test:pr-governance",
        "test:release-metadata",
    ] {
        assert!(
            package.contains(required),
            "package scripts must expose metadata harness: {}",
            required
        );
    }
}

#[test]
fn readme_documents_published_backup_and_current_architecture() {
    let readme = read_file("README.md");

    for required in [
        "Prompt backup",
        "Export prompts",
        "Import prompts",
        "JSON",
        "src/application/",
        "src/domain/",
        "platform.rs",
        "prompt_backup.rs",
        "window_lifecycle.rs",
    ] {
        assert!(
            readme.contains(required),
            "README must document published product/architecture contract: {}",
            required
        );
    }
}

#[test]
fn readme_backup_scope_matches_serialized_product_contract() {
    let readme = read_file("README.md");
    let backup = read_file("src/prompt_backup.rs");

    for required in [
        "pub app_name: String",
        "pub app_version: String",
        "pub schema_version: u32",
        "pub prompts: Vec<Prompt>",
    ] {
        assert!(
            backup.contains(required),
            "backup schema contract missing serialized field: {}",
            required
        );
    }

    assert!(
        readme.contains(
            "Prompt backups contain prompt records only; settings remain in `settings.json` and are not included in the JSON backup."
        ),
        "README must not imply that prompt backups include application settings"
    );
}


#[test]
fn release_metadata_tracks_native_autopaste_fix_as_fixed() {
    let changelog = read_file("CHANGELOG.md");
    let release = release_block_for(&changelog, "## [1.2.0-beta.10] - 2026-09-23");
    let fixed = release_fixed_section_for(&changelog, "## [1.2.0-beta.10] - 2026-09-23");
    let entry = "Fix native global-hotkey auto-paste to verify the prompt is on the clipboard before Ctrl+V injection and report injection failures instead of assuming success.";

    assert!(
        fixed.contains(entry),
        "beta.10/Fixed must record the native auto-paste behavior correction"
    );
    for category in ["### Added", "### Changed"] {
        let section = release
            .split(category)
            .nth(1)
            .expect("release category must exist")
            .split("###")
            .next()
            .expect("category must have a bounded section");
        assert!(
            !section.contains(entry),
            "native auto-paste correction must be classified as Fixed, not {category}"
        );
    }
}

#[test]
fn unreleased_native_autopaste_fix_claim_matches_runtime_contract() {
    let ui = read_file("src/ui.rs");
    let native = read_file("src/hotkeys/native.rs");

    for required in [
        "NativeClipboardPrepared",
        "clipboard_matches_expected_prompt",
        "iced::clipboard::write(content).chain(",
        "paste_from_prepared_clipboard()",
    ] {
        assert!(
            ui.contains(required),
            "native auto-paste changelog claim requires UI runtime behavior: {}",
            required
        );
    }

    for required in [
        "simulate(&EventType::KeyPress(RdevKey::ControlLeft)).is_ok()",
        "simulate(&EventType::KeyPress(RdevKey::KeyV)).is_ok()",
        "simulate(&EventType::KeyRelease(RdevKey::KeyV)).is_ok()",
        "simulate(&EventType::KeyRelease(RdevKey::ControlLeft)).is_ok()",
        "PasteOutcome::Completed",
        "PasteOutcome::Failed",
    ] {
        assert!(
            native.contains(required),
            "native auto-paste changelog claim requires injection outcome tracking: {}",
            required
        );
    }
}


#[test]
fn release_metadata_tracks_packaged_linux_x11_runtime_fix_as_fixed() {
    let changelog = read_file("CHANGELOG.md");
    let release = release_block_for(&changelog, "## [1.2.0-beta.10] - 2026-09-23");
    let fixed = release_fixed_section_for(&changelog, "## [1.2.0-beta.10] - 2026-09-23");
    let entry = "Fix packaged Linux X11 hotkeys and auto-paste by declaring or bundling the libxkbcommon-x11 runtime required for X11 keyboard mapping.";

    assert!(
        fixed.contains(entry),
        "beta.10/Fixed must record the packaged Linux X11 runtime correction"
    );
    for category in ["### Added", "### Changed"] {
        let section = release
            .split(category)
            .nth(1)
            .expect("release category must exist")
            .split("###")
            .next()
            .expect("category must have a bounded section");
        assert!(
            !section.contains(entry),
            "Linux X11 runtime correction must be classified as Fixed, not {category}"
        );
    }
}

#[test]
fn unreleased_linux_x11_runtime_fix_claim_matches_packaging_contract() {
    let deb = read_file("packaging/linux/build-deb.sh");
    let arch = read_file("packaging/arch/PKGBUILD");
    let rpm = read_file("packaging/rpm/jame-prompt.spec");
    let appimage = read_file("packaging/appimage/build-appimage.sh");

    assert!(
        deb.contains("libxkbcommon-x11-0"),
        "Debian package must declare the X11 keyboard runtime"
    );
    assert!(
        arch.contains("'libxkbcommon-x11'"),
        "Arch package must declare the X11 keyboard runtime"
    );
    assert!(
        rpm.contains("Requires:       libxkbcommon-x11"),
        "RPM package must declare the X11 keyboard runtime"
    );

    for required in [
        "libxkbcommon-x11.so.0",
        "--library \"$XKBCOMMON_X11_LIB\"",
        "AppImage is missing bundled libxkbcommon-x11",
    ] {
        assert!(
            appimage.contains(required),
            "AppImage must bundle and verify the X11 keyboard runtime: {}",
            required
        );
    }
}


#[test]
fn release_metadata_tracks_windows_msi_lifecycle_fix_as_fixed() {
    let changelog = read_file("CHANGELOG.md");
    let release = release_block_for(&changelog, "## [1.2.0-beta.10] - 2026-09-23");
    let fixed = release_fixed_section_for(&changelog, "## [1.2.0-beta.10] - 2026-09-23");
    let entry = "Fix the Windows MSI install lifecycle by separating machine installation state from the per-user Start Menu shortcut state and verifying both are removed on uninstall.";

    assert!(
        fixed.contains(entry),
        "beta.10/Fixed must record the Windows MSI lifecycle correction"
    );
    for category in ["### Added", "### Changed"] {
        let section = release
            .split(category)
            .nth(1)
            .expect("release category must exist")
            .split("###")
            .next()
            .expect("category must have a bounded section");
        assert!(
            !section.contains(entry),
            "Windows MSI lifecycle correction must be classified as Fixed, not {category}"
        );
    }
}

#[test]
fn unreleased_windows_msi_lifecycle_fix_claim_matches_installer_contract() {
    let wix = read_file("wix/main.wxs");
    let validator = read_file("scripts/validate_windows_msi.ps1");

    assert!(
        wix.contains("InstallScope=\"perMachine\""),
        "MSI lifecycle claim requires a per-machine package"
    );

    let application_files = wix
        .split("<Component Id=\"ApplicationFiles\"")
        .nth(1)
        .expect("WiX must define ApplicationFiles")
        .split("</Component>")
        .next()
        .expect("ApplicationFiles component must terminate");
    assert!(
        application_files.contains("Root=\"HKLM\"")
            && application_files.contains("Key=\"Software\\JamePrompt\"")
            && application_files.contains("Name=\"installed\""),
        "per-machine application component must own the HKLM installation marker"
    );
    assert!(
        !application_files.contains("Root=\"HKCU\""),
        "per-machine application component must not own per-user registry state"
    );

    let shortcut = wix
        .split("<Component Id=\"ApplicationShortcut\"")
        .nth(1)
        .expect("WiX must define ApplicationShortcut")
        .split("</Component>")
        .next()
        .expect("ApplicationShortcut component must terminate");
    assert!(
        shortcut.contains("Root=\"HKCU\"")
            && shortcut.contains("Key=\"Software\\JamePrompt\"")
            && shortcut.contains("Name=\"installed\"")
            && shortcut.contains("KeyPath=\"yes\""),
        "Start Menu shortcut component must preserve its HKCU registry KeyPath"
    );
    assert!(
        !shortcut.contains("Root=\"HKLM\""),
        "shortcut component must not mix per-user resources with machine registry state"
    );

    for required in [
        "$MachineMarker = \"HKLM:\\SOFTWARE\\JamePrompt\"",
        "$UserShortcutMarker = \"HKCU:\\SOFTWARE\\JamePrompt\"",
        "MSI uninstall left machine registry marker behind",
        "MSI uninstall left user shortcut registry marker behind",
        "MSI uninstall left Start Menu shortcut behind",
    ] {
        assert!(
            validator.contains(required),
            "MSI lifecycle validator must enforce install/uninstall state: {}",
            required
        );
    }
}
